//! WSL distributions as terminals (contract: docs/wiki/pages/syntheses/agent-workflow-features.md,
//! "WSL").
//!
//! Windows only. `wsl.exe --list --quiet` names the installed distributions, one per line, in
//! UTF-16LE (measured: [[wsl-list-measurement]]). Each becomes an entry like a catalog
//! terminal: id `wsl-<name in lowercase>`, command `wsl.exe -d <name>`, working folder through
//! `--cd`. Docker Desktop's internal distributions are left out. Detection runs once per profile
//! reload; the caller caches it.

use crate::agent_catalog::{Agent, Source};
use crate::profile::Profile;

pub const ID_PREFIX: &str = "wsl-";
/// Distribution names are short; anything longer or with other characters is not shown.
const MAX_NAME_CHARS: usize = 64;

/// Distribution names from the output of `wsl.exe --list --quiet`. UTF-16LE as wsl.exe writes it
/// into a pipe, or UTF-8 when `WSL_UTF8=1` was set (the child does not get that variable, but
/// the parser does not rely on it).
pub fn parse_list(output: &[u8]) -> Vec<String> {
    let text = decode(output);
    let mut names: Vec<String> = Vec::new();
    for line in text.lines() {
        let name = line.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}');
        if !is_valid_name(name) || is_internal(name) {
            continue;
        }
        if !names.iter().any(|seen| seen.eq_ignore_ascii_case(name)) {
            names.push(name.to_owned());
        }
    }
    names
}

fn decode(output: &[u8]) -> String {
    // UTF-16LE text of ASCII names has a zero in every second byte.
    let utf16 = output.len() >= 2 && output.iter().skip(1).step_by(2).any(|b| *b == 0);
    if !utf16 {
        return String::from_utf8_lossy(output).into_owned();
    }
    let units: Vec<u16> = output
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes(*pair))
        .collect();
    String::from_utf16_lossy(&units)
}

/// Letters, digits, `.`, `-`, `_`: what `wsl --install` and `wsl --import` accept.
fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_CHARS
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// `docker-desktop` and `docker-desktop-data` belong to Docker Desktop and are no terminals.
fn is_internal(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("docker-desktop")
}

/// `%SystemRoot%\System32\wsl.exe`, for detection and for starting: a folder on PATH could hold
/// another `wsl.exe`. Without `SystemRoot` (not a normal Windows environment) the usual folder.
#[cfg(windows)]
pub fn program() -> std::path::PathBuf {
    let root = std::env::var_os("SystemRoot").unwrap_or_else(|| r"C:\Windows".into());
    std::path::PathBuf::from(root)
        .join("System32")
        .join("wsl.exe")
}

/// The terminal entry for one distribution.
pub fn agent(distro: &str) -> Agent {
    // Absolute on Windows, so the start resolves exactly the program detection ran. Other
    // platforms never detect a distribution; the plain name only serves tests there.
    #[cfg(windows)]
    let command = program().to_string_lossy().into_owned();
    #[cfg(not(windows))]
    let command = "wsl.exe".to_owned();
    Agent {
        profile: Profile {
            id: format!("{ID_PREFIX}{}", distro.to_ascii_lowercase()),
            name: distro.to_owned(),
            command: command.clone(),
            args: vec!["-d".to_owned(), distro.to_owned()],
            cwd: None,
            env: Default::default(),
            secret_env: Vec::new(),
            icon: Some("WSL".to_owned()),
            // Every entry is the same wsl.exe; its version says nothing about the distribution.
            version_args: Some(Vec::new()),
            resume_args: None,
            session_id_args: None,
            resume_id_args: None,
            sub_agents: None,
            latest: None,
            update_hint: None,
            packages: None,
            self_update: None,
        },
        source: Source::Catalog,
        candidates: vec![command],
        homepage: None,
        install_hint: None,
        cwd_flag: Some("--cd"),
        capabilities: crate::agent_catalog::Capabilities::default(),
    }
}

pub fn agents(distros: &[String]) -> Vec<Agent> {
    distros.iter().map(|name| agent(name)).collect()
}

/// Installed distributions as entries; empty when WSL is missing, has none or does not answer
/// within the time limit, and always on other platforms.
pub fn detect() -> Vec<Agent> {
    agents(&imp::list())
}

#[cfg(windows)]
mod imp {
    use std::io::Read;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use crate::proctree::ProcTree;

    /// The first call may start the WSL service; a few seconds are normal then.
    const TIMEOUT: Duration = Duration::from_secs(5);
    const MAX_OUTPUT: u64 = 64 * 1024;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    pub fn list() -> Vec<String> {
        let program = super::program();
        if !program.is_file() {
            return Vec::new();
        }
        match run(&program) {
            Ok(Some(output)) => super::parse_list(&output),
            Ok(None) => Vec::new(),
            Err(error) => {
                eprintln!("hangar: WSL distributions not listed: {error}");
                Vec::new()
            }
        }
    }

    /// stdout of a successful run; `None` when wsl.exe reports an error (no WSL, no distribution).
    fn run(program: &PathBuf) -> std::io::Result<Option<Vec<u8>>> {
        use std::os::windows::process::CommandExt;
        let mut child = Command::new(program)
            .args(["--list", "--quiet"])
            .env_remove("WSL_UTF8")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()?;
        let tree = match ProcTree::attach(child.id()) {
            Ok(tree) => Some(tree),
            // Ended before it could join the job: nothing left to kill, the output is still in
            // the pipe.
            Err(_) if matches!(child.try_wait(), Ok(Some(_))) => None,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
        };
        let terminate = |child: &mut std::process::Child| {
            if let Some(tree) = &tree {
                let _ = tree.terminate();
            }
            let _ = child.kill();
            let _ = child.wait();
        };
        let Some(stdout) = child.stdout.take() else {
            terminate(&mut child);
            return Ok(None);
        };
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name("wsl-list".to_owned())
            .spawn(move || {
                let mut data = Vec::new();
                let _ = stdout.take(MAX_OUTPUT).read_to_end(&mut data);
                let _ = tx.send(data);
            })?;
        let deadline = Instant::now() + TIMEOUT;
        let output = rx.recv_timeout(TIMEOUT);
        let mut status = None;
        while output.is_ok() && Instant::now() < deadline {
            match child.try_wait() {
                Ok(Some(exit)) => {
                    status = Some(exit);
                    break;
                }
                Ok(None) => thread::sleep(Duration::from_millis(5)),
                Err(_) => break,
            }
        }
        if status.is_none() {
            // Hung or too slow: the whole tree goes, then it is reaped.
            terminate(&mut child);
        }
        match (output, status) {
            (Ok(data), Some(status)) if status.success() => Ok(Some(data)),
            (Ok(_), Some(_)) => Ok(None),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "wsl.exe did not answer in time",
            )),
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn list() -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn recorded_output_of_windows_11() {
        // `wsl.exe --list --quiet` into a pipe, Windows 11 Pro 10.0.26300, 2026-11-08.
        let recorded: &[u8] = &[
            b'U', 0, b'b', 0, b'u', 0, b'n', 0, b't', 0, b'u', 0, b'\r', 0, b'\n', 0, b'd', 0,
            b'o', 0, b'c', 0, b'k', 0, b'e', 0, b'r', 0, b'-', 0, b'd', 0, b'e', 0, b's', 0, b'k',
            0, b't', 0, b'o', 0, b'p', 0, b'\r', 0, b'\n', 0,
        ];
        assert_eq!(parse_list(recorded), ["Ubuntu"]);
    }

    #[test]
    fn names_are_filtered_and_deduplicated() {
        let output = utf16(
            "\u{FEFF}Ubuntu-24.04\r\n\r\n  Debian \r\ndocker-desktop-data\r\nDocker-Desktop\r\n\
             bad name\r\nubuntu-24.04\r\nkali_linux\r\n",
        );
        assert_eq!(
            parse_list(&output),
            ["Ubuntu-24.04", "Debian", "kali_linux"]
        );
        // With WSL_UTF8=1 wsl.exe writes UTF-8.
        assert_eq!(parse_list(b"Ubuntu\nDebian\n"), ["Ubuntu", "Debian"]);
        assert!(parse_list(b"").is_empty());
        // An odd trailing byte (cut output) does not break the rest.
        let mut cut = utf16("Arch\r\n");
        cut.push(b'X');
        assert_eq!(parse_list(&cut), ["Arch"]);
    }

    /// Runs the real wsl.exe; by hand: `cargo test --lib -- --ignored wsl_on_this_machine`.
    #[test]
    #[ignore = "starts wsl.exe"]
    fn wsl_on_this_machine() {
        let started = std::time::Instant::now();
        let found = detect();
        eprintln!(
            "{:?} in {:?}",
            found.iter().map(Agent::id).collect::<Vec<_>>(),
            started.elapsed()
        );
        assert!(found
            .iter()
            .all(|a| !a.id().starts_with("wsl-docker-desktop")));
    }

    #[test]
    fn entries_start_the_distribution() {
        let agent = agent("Ubuntu-24.04");
        assert_eq!(agent.id(), "wsl-ubuntu-24.04");
        #[cfg(windows)]
        {
            let command = std::path::Path::new(&agent.profile.command);
            assert!(command.is_absolute(), "{command:?}");
            assert!(command.ends_with(r"System32\wsl.exe"), "{command:?}");
            assert_eq!(
                agent.candidates,
                std::slice::from_ref(&agent.profile.command)
            );
        }
        #[cfg(not(windows))]
        assert_eq!(agent.profile.command, "wsl.exe");
        assert_eq!(agent.profile.args, ["-d", "Ubuntu-24.04"]);
        assert_eq!(agent.cwd_flag, Some("--cd"));
        assert_eq!(agent.source, Source::Catalog);
        assert_eq!(agent.profile.version_args.as_deref(), Some(&[][..]));
        let view = agent.view(None, None);
        assert!(!view.editable);
        assert_eq!(view.name, "Ubuntu-24.04");
    }
}
