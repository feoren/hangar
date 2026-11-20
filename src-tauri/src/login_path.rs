//! The `PATH` of the user's login shell, for Hangar's own process on macOS and Linux (ADR 0013,
//! ADR 0017).
//!
//! A program started from the desktop (an application menu, the Dock, an AppImage) gets the
//! environment of the desktop session, not of the user's shell: whatever `~/.profile`,
//! `~/.zshrc` or `~/.bashrc` add to `PATH` (`~/.local/bin`, nvm, npm's global folder, Homebrew)
//! is missing, and agent CLIs installed there count as not installed. At startup Hangar therefore
//! asks the login shell once for its `PATH` and puts it in front of its own ([`adopt`]). Every
//! program search (`resolve.rs` through `manager.rs`, `git.rs`, `arena.rs`, `install_source.rs`)
//! and every child process reads the process environment, so this one change reaches all of
//! them, the ones added later included. Windows hands every process the `PATH` from the registry
//! and needs none of this.

use std::collections::HashSet;
use std::env::JoinPathsError;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::time::Duration;

/// How long the login shell may take. Slow dotfiles (a version manager that compiles, a network
/// mount) must not hold the window back for longer; Hangar then keeps the inherited `PATH`.
pub const TIMEOUT: Duration = Duration::from_secs(3);

/// Printed right before and after the `PATH`, so text the dotfiles print around it (a greeting,
/// `fortune`, a logout message) is not taken for part of it.
pub const START: &str = "__HANGAR_LOGIN_PATH_START__";
pub const END: &str = "__HANGAR_LOGIN_PATH_END__";

/// The shell when neither `$SHELL` nor the account names an absolute path. zsh is the default
/// login shell of every account created since macOS 10.15, and every macOS from 13 on has it.
pub const FALLBACK_SHELL: &str = if cfg!(target_os = "macos") {
    "/bin/zsh"
} else {
    "/bin/sh"
};

/// The command the login shell runs. POSIX shells and fish both have `printf`; fish joins a path
/// variable such as `PATH` with colons when it is quoted.
pub const COMMAND: &str =
    "printf '%s%s%s' '__HANGAR_LOGIN_PATH_START__' \"$PATH\" '__HANGAR_LOGIN_PATH_END__'";

#[derive(Debug, thiserror::Error)]
pub enum LoginPathError {
    #[error("the login shell {shell} could not be started: {source}")]
    Spawn {
        shell: String,
        #[source]
        source: std::io::Error,
    },
    #[error("the login shell {shell} did not answer within {seconds} s")]
    Timeout { shell: String, seconds: u64 },
    #[error("the login shell {shell} printed no PATH")]
    NoPath { shell: String },
}

/// The bytes between the last [`START`] and the [`END`] after it; `None` without both or when
/// nothing is in between. The last start counts: the command runs after every startup file, and
/// only logout files can print after it.
pub fn between_markers(output: &[u8]) -> Option<&[u8]> {
    let start = rfind(output, START.as_bytes())? + START.len();
    let rest = &output[start..];
    let end = find(rest, END.as_bytes())?;
    let value = &rest[..end];
    (!value.is_empty()).then_some(value)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn rfind(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .rposition(|window| window == needle)
}

/// The login shell's entries first, then Hangar's own ones it lacks; each folder once. The login
/// shell's order is the one the user set in the dotfiles. Only absolute entries are kept: an
/// empty, `.`, `bin` or `./node_modules/.bin` entry from a dotfile would resolve against the
/// working folder of whatever searches it, and a session's working folder is a foreign project.
pub fn merge(login: &OsStr, own: Option<&OsStr>) -> Result<OsString, JoinPathsError> {
    let mut seen = HashSet::new();
    let entries: Vec<PathBuf> = std::env::split_paths(login)
        .chain(own.map(std::env::split_paths).into_iter().flatten())
        .filter(|entry| entry.is_absolute())
        .filter(|entry| seen.insert(entry.clone()))
        .collect();
    std::env::join_paths(entries)
}

/// The first absolute path of `$SHELL` (`shell`) and the account's login shell from the user
/// database (`account`), otherwise [`FALLBACK_SHELL`]. The account comes second: a program
/// started from the Dock or Finder may get no `$SHELL`, and the shell the user chose with
/// `chsh` is the one a terminal would start.
pub fn login_shell(shell: Option<&OsStr>, account: Option<&OsStr>) -> PathBuf {
    [shell, account]
        .into_iter()
        .flatten()
        .map(PathBuf::from)
        .find(|path| path.is_absolute())
        .unwrap_or_else(|| PathBuf::from(FALLBACK_SHELL))
}

#[cfg(unix)]
pub use unix::{account_shell, adopt, query, run};

#[cfg(unix)]
mod unix {
    use std::ffi::{CStr, OsString};
    use std::io::{self, Read};
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use super::{between_markers, login_shell, merge, LoginPathError, COMMAND};

    /// More output than this is not read: the answer comes first and is far shorter.
    const MAX_OUTPUT: usize = 1024 * 1024;

    /// How long the shell may take to exit after it printed the answer (logout files) before
    /// its process group is ended.
    const EXIT_GRACE: Duration = Duration::from_millis(500);

    /// How long the shell may take to go away after SIGKILL before a thread takes over reaping.
    const REAP_WAIT: Duration = Duration::from_millis(200);

    /// Puts the login shell's `PATH` in front of Hangar's own. Called once at the start of
    /// [`crate::run`], right after [`crate::child_env::scrub_own_environment`] and for the same
    /// reason: changing the environment is only sound while no other thread reads it. The threads
    /// [`run`] may leave behind only read a pipe or wait for a child and never touch the
    /// environment. On any
    /// failure the inherited `PATH` stays, as before this existed.
    pub fn adopt() {
        let shell = login_shell(
            std::env::var_os("SHELL").as_deref(),
            account_shell().as_deref(),
        );
        let login = match query(&shell, super::TIMEOUT) {
            Ok(login) => login,
            Err(error) => {
                eprintln!("hangar: {error}; programs are searched on the inherited PATH");
                return;
            }
        };
        match merge(&login, std::env::var_os("PATH").as_deref()) {
            Ok(path) => std::env::set_var("PATH", path),
            Err(error) => eprintln!("hangar: the login shell's PATH is unusable: {error}"),
        }
    }

    /// The login shell of the account Hangar runs as, from the user database (`getpwuid_r`), or
    /// `None` when there is no entry or no shell in it.
    pub fn account_shell() -> Option<OsString> {
        // Large enough for any real entry; a larger one fails with ERANGE and yields `None`.
        let mut buffer = vec![0 as libc::c_char; 16 * 1024];
        // SAFETY: an all-zero `passwd` is a valid value of this plain C struct (null pointers,
        // zero ids); getpwuid_r overwrites it before it is read.
        let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
        let mut found: *mut libc::passwd = std::ptr::null_mut();
        // SAFETY: getuid has no preconditions. getpwuid_r writes only into `entry` and `buffer`,
        // whose sizes are passed, and sets `found` to `&mut entry` or null; the strings it
        // stores point into `buffer`, which outlives every read below.
        let status = unsafe {
            libc::getpwuid_r(
                libc::getuid(),
                &mut entry,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut found,
            )
        };
        if status != 0 || found.is_null() || entry.pw_shell.is_null() {
            return None;
        }
        // SAFETY: `pw_shell` is non-null and points to a NUL-terminated string inside `buffer`.
        let shell = unsafe { CStr::from_ptr(entry.pw_shell) };
        let shell = OsString::from_vec(shell.to_bytes().to_vec());
        (!shell.is_empty()).then_some(shell)
    }

    /// The `PATH` of `shell` as a login and interactive shell (`-ilc`: interactive, because many
    /// users extend `PATH` in `~/.bashrc` or `~/.zshrc`, which only interactive shells read).
    /// stdin is empty and stderr discarded, so a prompt or a warning cannot block it.
    pub fn query(shell: &Path, timeout: Duration) -> Result<OsString, LoginPathError> {
        let mut command = Command::new(shell);
        command.args(["-ilc", COMMAND]);
        // No calling session's markers or gate token, and none of an AppImage's variables: the
        // dotfiles see the environment a terminal session would.
        crate::child_env::scrub(&mut command);
        run(command, timeout)
    }

    /// Runs `command` in a session of its own and returns what it prints between the markers,
    /// or ends its whole process group after `timeout`.
    pub fn run(mut command: Command, timeout: Duration) -> Result<OsString, LoginPathError> {
        let shell = command.get_program().to_string_lossy().into_owned();
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // SAFETY: the closure runs in the forked child before exec and only calls setsid, which
        // is async-signal-safe and touches no memory of the parent. A session of its own has no
        // controlling terminal: an interactive shell then does not try to take over the terminal
        // Hangar was started from (zsh would stop on SIGTTOU), and the child is the leader of
        // its own process group, which killpg below ends as a whole.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().map_err(|source| LoginPathError::Spawn {
            shell: shell.clone(),
            source,
        })?;
        let Some(mut stdout) = child.stdout.take() else {
            end(child, Duration::ZERO);
            return Err(LoginPathError::NoPath { shell });
        };
        let (sender, receiver) = mpsc::channel();
        // Answers as soon as the end marker arrives: a program the dotfiles leave running in the
        // background may keep the pipe open long after the shell itself is gone.
        let reader = std::thread::Builder::new()
            .name("login-path".into())
            .spawn(move || {
                let mut output = Vec::new();
                let mut chunk = [0u8; 4096];
                loop {
                    match stdout.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            output.extend_from_slice(&chunk[..read]);
                            if let Some(value) = between_markers(&output) {
                                let _ = sender.send(Some(value.to_vec()));
                                return;
                            }
                            if output.len() > MAX_OUTPUT {
                                break;
                            }
                        }
                    }
                }
                let _ = sender.send(None);
            });
        if reader.is_err() {
            end(child, Duration::ZERO);
            return Err(LoginPathError::NoPath { shell });
        }
        let answer = receiver.recv_timeout(timeout);
        match answer {
            Ok(Some(value)) => {
                end(child, EXIT_GRACE);
                Ok(OsString::from_vec(value))
            }
            Ok(None) => {
                end(child, EXIT_GRACE);
                Err(LoginPathError::NoPath { shell })
            }
            Err(_) => {
                end(child, Duration::ZERO);
                Err(LoginPathError::Timeout {
                    shell,
                    seconds: timeout.as_secs(),
                })
            }
        }
    }

    /// Waits up to `grace` for the shell to exit, then ends its process group and reaps the
    /// shell. A shell that is still not reaped after [`REAP_WAIT`] (stuck in uninterruptible
    /// sleep on a hung network mount, where even SIGKILL waits) goes to a thread of its own that
    /// reaps it whenever it ends, so it cannot hold the window back. That thread only waits for
    /// the child and never touches the environment.
    pub(super) fn end(mut child: Child, grace: Duration) {
        let exited = wait_for(&mut child, grace);
        if let Ok(group) = libc::pid_t::try_from(child.id()) {
            // SAFETY: killpg is a plain system call without memory access. The id is the
            // shell's process group (setsid above). If the shell is reaped already, the id stays
            // reserved for as long as any member of its group lives, so the signal reaches only
            // those; with no member left the call fails with ESRCH, which is fine to ignore.
            unsafe {
                libc::killpg(group, libc::SIGKILL);
            }
        }
        if exited || wait_for(&mut child, REAP_WAIT) {
            return;
        }
        let _ = std::thread::Builder::new()
            .name("login-path-reap".into())
            .spawn(move || {
                let _ = child.wait();
            });
    }

    /// Whether `child` exited (and was reaped) within `limit`.
    fn wait_for(child: &mut Child, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        loop {
            if matches!(child.try_wait(), Ok(Some(_))) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An absolute folder on this platform: `/name` on Unix, `C:\name` on Windows.
    fn abs(name: &str) -> String {
        if cfg!(windows) {
            format!("C:\\{name}")
        } else {
            format!("/{name}")
        }
    }

    fn joined(entries: &[&str]) -> OsString {
        std::env::join_paths(entries).unwrap()
    }

    fn entries(path: &OsStr) -> Vec<PathBuf> {
        std::env::split_paths(path).collect()
    }

    #[test]
    fn the_path_between_the_markers_survives_noise_around_it() {
        let output = format!(
            "Welcome back!\n{START}/home/x/.local/bin:/usr/bin{END}\nlogout\n",
            START = START,
            END = END
        );
        assert_eq!(
            between_markers(output.as_bytes()),
            Some(b"/home/x/.local/bin:/usr/bin".as_slice())
        );
    }

    #[test]
    fn the_last_start_marker_counts() {
        // A dotfile that echoes the marker itself comes before the command's own output.
        let output = format!("{START} echoed by a dotfile\n{START}/usr/bin{END}");
        assert_eq!(
            between_markers(output.as_bytes()),
            Some(b"/usr/bin".as_slice())
        );
    }

    #[test]
    fn missing_or_empty_answers_are_none() {
        assert_eq!(between_markers(b""), None);
        assert_eq!(between_markers(b"no markers at all"), None);
        assert_eq!(between_markers(format!("{START}/usr/bin").as_bytes()), None);
        assert_eq!(between_markers(format!("/usr/bin{END}").as_bytes()), None);
        assert_eq!(between_markers(format!("{START}{END}").as_bytes()), None);
        // The end marker must come after the start marker.
        assert_eq!(
            between_markers(format!("{END}/usr/bin{START}").as_bytes()),
            None
        );
    }

    #[test]
    fn the_command_prints_both_markers() {
        assert!(COMMAND.contains(START) && COMMAND.contains(END));
        assert!(COMMAND.find(START) < COMMAND.find(END));
    }

    #[test]
    fn the_login_path_comes_first_and_each_folder_once() {
        let (local, usr, own_bin) = (abs("local-bin"), abs("usr-bin"), abs("own-bin"));
        let login = joined(&[&local, &usr, &local]);
        let own = joined(&[&usr, "", &own_bin]);
        let merged = merge(&login, Some(&own)).unwrap();
        assert_eq!(
            entries(&merged),
            [
                PathBuf::from(&local),
                PathBuf::from(&usr),
                PathBuf::from(&own_bin)
            ]
        );
    }

    #[test]
    fn relative_entries_from_dotfiles_are_dropped() {
        let usr = abs("usr-bin");
        let login = joined(&[".", "bin", &usr, "./node_modules/.bin", ""]);
        let own = joined(&["..", &usr, "tools"]);
        let merged = merge(&login, Some(&own)).unwrap();
        assert_eq!(entries(&merged), [PathBuf::from(&usr)]);
    }

    #[test]
    fn without_an_own_path_the_login_path_stays() {
        let login = joined(&[&abs("usr-local-bin"), &abs("usr-bin")]);
        assert_eq!(merge(&login, None).unwrap(), login);
    }

    #[test]
    fn the_shell_falls_back_to_the_platform_default() {
        let fallback = PathBuf::from(FALLBACK_SHELL);
        assert_eq!(login_shell(None, None), fallback);
        assert_eq!(login_shell(Some(OsStr::new("")), None), fallback);
        assert_eq!(
            login_shell(Some(OsStr::new("zsh")), Some(OsStr::new("bash"))),
            fallback
        );
    }

    #[test]
    fn the_fallback_is_zsh_on_macos_and_sh_elsewhere() {
        let expected = if cfg!(target_os = "macos") {
            "/bin/zsh"
        } else {
            "/bin/sh"
        };
        assert_eq!(FALLBACK_SHELL, expected);
    }

    #[cfg(unix)]
    #[test]
    fn shell_beats_the_account_and_the_account_beats_the_fallback() {
        let (env, account) = (OsStr::new("/usr/bin/fish"), OsStr::new("/bin/bash"));
        assert_eq!(
            login_shell(Some(env), Some(account)),
            PathBuf::from("/usr/bin/fish")
        );
        // A Dock or Finder start without `$SHELL`, or a relative one: the account decides.
        assert_eq!(login_shell(None, Some(account)), PathBuf::from("/bin/bash"));
        assert_eq!(
            login_shell(Some(OsStr::new("zsh")), Some(account)),
            PathBuf::from("/bin/bash")
        );
        assert_eq!(
            login_shell(None, Some(OsStr::new("nologin"))),
            PathBuf::from(FALLBACK_SHELL)
        );
    }

    #[cfg(unix)]
    mod unix {
        use std::path::Path;
        use std::process::Command;
        use std::time::{Duration, Instant};

        use super::super::*;

        fn sh(script: &str) -> Command {
            let mut command = Command::new("/bin/sh");
            command.args(["-c", script]);
            command
        }

        #[test]
        fn the_answer_is_read_between_noise() {
            let script = format!("echo greeting; {COMMAND}; echo farewell");
            let path = run(sh(&script), TIMEOUT).unwrap();
            assert!(!path.is_empty());
            assert_eq!(Some(path), std::env::var_os("PATH"));
        }

        #[test]
        fn the_answer_comes_before_a_lingering_child_ends() {
            // The background sleep keeps the pipe open; the answer must not wait for it.
            let script = format!("{COMMAND}; sleep 30 & wait");
            let started = Instant::now();
            assert!(run(sh(&script), TIMEOUT).is_ok());
            assert!(started.elapsed() < Duration::from_secs(5));
        }

        #[test]
        fn a_slow_shell_times_out_and_is_ended() {
            let started = Instant::now();
            let error = run(sh("sleep 30"), Duration::from_millis(200)).unwrap_err();
            assert!(matches!(error, LoginPathError::Timeout { .. }), "{error:?}");
            assert!(started.elapsed() < Duration::from_secs(5));
        }

        #[test]
        fn a_shell_without_markers_has_no_path() {
            let error = run(sh("echo only noise"), TIMEOUT).unwrap_err();
            assert!(matches!(error, LoginPathError::NoPath { .. }), "{error:?}");
        }

        #[test]
        fn a_child_that_outlives_the_kill_is_left_to_a_reaper() {
            // Spawned without a session of its own, the child's pid names no process group, so
            // killpg misses it and it keeps running, as a shell stuck in uninterruptible sleep
            // would: ending must not wait for it.
            let child = Command::new("/bin/sh")
                .args(["-c", "sleep 3"])
                .spawn()
                .unwrap();
            let pid = libc::pid_t::try_from(child.id()).unwrap();
            let started = Instant::now();
            super::super::unix::end(child, Duration::ZERO);
            assert!(started.elapsed() < Duration::from_secs(1));
            // SAFETY: kill with signal 0 only checks that the process still exists; the reaper
            // thread has not reaped it yet, so the pid is still this child's.
            assert_eq!(unsafe { libc::kill(pid, 0) }, 0, "the child was waited for");
            // SAFETY: as above; ends the child early so the reaper finishes.
            unsafe {
                libc::kill(pid, libc::SIGKILL);
            }
        }

        #[test]
        fn the_account_has_an_absolute_login_shell() {
            // CI runners and developer machines run as an account with a shell in the user
            // database; a missing entry would yield `None` and leave the choice to `$SHELL`.
            let shell = account_shell().expect("no login shell in the user database");
            assert!(Path::new(&shell).is_absolute(), "{shell:?}");
        }

        /// zsh is the default shell on macOS: as a login and interactive shell in a session of
        /// its own and without a terminal it must still print the markers, and its `PATH` must
        /// keep the system folders.
        #[cfg(target_os = "macos")]
        #[test]
        fn zsh_answers_as_a_login_shell_on_macos() {
            let path = query(Path::new("/bin/zsh"), TIMEOUT).unwrap();
            let entries: Vec<_> = std::env::split_paths(&path).collect();
            assert!(
                entries.iter().any(|entry| entry == Path::new("/usr/bin")),
                "{path:?}"
            );
        }

        #[test]
        fn a_missing_shell_is_a_spawn_error() {
            let error = query(Path::new("/nonexistent/hangar-test-shell"), TIMEOUT).unwrap_err();
            assert!(matches!(error, LoginPathError::Spawn { .. }), "{error:?}");
        }
    }
}
