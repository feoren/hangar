//! Versions of the installed programs (ADR 0003).
//!
//! The program is started without a PTY with fixed version arguments: empty stdin, stdout and
//! stderr redirected, working folder = temporary folder, environment as for sessions. After the
//! time limit, or at the latest once the output is collected, the process tree is killed.
//! The arguments come only from the catalog, the profile or the built-in exceptions, never from
//! frontend input.

use std::collections::{BTreeMap, HashMap};
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crate::agent_catalog::Agent;
use crate::filecache::{FileCache, FileStamp};
use crate::manager::INHERITED_SESSION_MARKERS;
use crate::proctree::ProcTree;

pub const TIMEOUT: Duration = Duration::from_secs(5);
/// A version output is short; nothing beyond this is read. Then Hangar closes the pipe.
const OUTPUT_LIMIT: u64 = 64 * 1024;
/// How long to keep waiting for remaining output after killing the process tree.
const DRAIN_GRACE: Duration = Duration::from_millis(500);

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Key: program file and arguments. Two profiles with the same program but their own
/// `version_args` get separate entries.
pub type VersionCache = FileCache<(FileStamp, Vec<String>), Option<String>>;

/// First match of the pattern `\d+(\.\d+){1,3}`, greedy like a regex match.
pub fn parse_version(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let digits_end = |mut at: usize| {
        while bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        at
    };
    let mut at = 0;
    while at < bytes.len() {
        if !bytes.get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
            continue;
        }
        let start = at;
        let mut end = digits_end(at);
        let mut groups = 0;
        while groups < 3
            && bytes.get(end) == Some(&b'.')
            && bytes.get(end + 1).is_some_and(u8::is_ascii_digit)
        {
            end = digits_end(end + 1);
            groups += 1;
        }
        if groups > 0 {
            // Start and end lie on ASCII characters, so on valid UTF-8 boundaries.
            return text.get(start..end).map(str::to_owned);
        }
        // A match cannot start in the middle of the same digit run; continue after it.
        at = end;
    }
    None
}

/// Arguments of the query. A configured `version_args` wins; an empty list means
/// "never query". Otherwise the exceptions by program name apply, then `--version`.
pub fn version_args(configured: Option<&[String]>, program: &Path) -> Vec<String> {
    if let Some(args) = configured {
        return args.to_vec();
    }
    let stem = program
        .file_stem()
        .and_then(OsStr::to_str)
        .map(str::to_ascii_lowercase);
    let args: &[&str] = match stem.as_deref() {
        // The file version of powershell.exe is the Windows version, and 5.1 does not know `--version`.
        Some("powershell") => &[
            "-NoProfile",
            "-Command",
            "$PSVersionTable.PSVersion.ToString()",
        ],
        Some("cmd") => &["/d", "/c", "ver"],
        _ => &["--version"],
    };
    args.iter().map(|a| (*a).to_owned()).collect()
}

/// Starts `program` with `args` and reads the version from stdout, otherwise from stderr.
/// The exit code does not matter. Any error yields `None`.
pub fn probe(
    program: &Path,
    args: &[String],
    env: &BTreeMap<String, String>,
    timeout: Duration,
) -> Option<String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(std::env::temp_dir());
    // The same environment as a session, so a profile `env` (a config folder, say) applies.
    command.env("TERM", "xterm-256color");
    command.env("COLORTERM", "truecolor");
    for key in INHERITED_SESSION_MARKERS {
        command.env_remove(key);
    }
    command.envs(env);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group, so ProcTree (killpg) hits exactly this tree.
        command.process_group(0);
    }

    let deadline = Instant::now() + timeout;
    let mut child = command.spawn().ok()?;
    // Without a Job Object at least the direct child stays killable (child.kill).
    let tree = ProcTree::attach(child.id()).ok();

    let (tx, rx) = mpsc::channel();
    let mut pending = 0;
    if let Some(stdout) = child.stdout.take() {
        pending += usize::from(drain(stdout, 0, tx.clone()));
    }
    if let Some(stderr) = child.stderr.take() {
        pending += usize::from(drain(stderr, 1, tx));
    }

    let mut outputs = [Vec::new(), Vec::new()];
    let mut collect = |limit: Instant, pending: &mut usize| {
        while *pending > 0 {
            let remaining = limit.saturating_duration_since(Instant::now());
            match rx.recv_timeout(remaining) {
                Ok((index, data)) => {
                    if let Some(slot) = outputs.get_mut(index) {
                        *slot = data;
                    }
                    *pending -= 1;
                }
                Err(_) => break,
            }
        }
    };
    collect(deadline, &mut pending);
    // Always kill, even after a regular exit: descendants that keep the pipes open or
    // look for updates in the background must not keep running.
    terminate(tree.as_ref(), &mut child);
    // Only now does the EOF arrive on a timeout; the partial output still counts.
    collect(Instant::now() + DRAIN_GRACE, &mut pending);

    let [stdout, stderr] = outputs;
    parse_version(&String::from_utf8_lossy(&stdout))
        .or_else(|| parse_version(&String::from_utf8_lossy(&stderr)))
}

/// Reads a stream until EOF or [`OUTPUT_LIMIT`] on a thread of its own. Both streams must be
/// read at the same time, otherwise a child writing into a full pipe blocks.
fn drain(stream: impl Read + Send + 'static, index: usize, tx: Sender<(usize, Vec<u8>)>) -> bool {
    thread::Builder::new()
        .name(format!("version-read-{index}"))
        .spawn(move || {
            let mut data = Vec::new();
            let _ = stream.take(OUTPUT_LIMIT).read_to_end(&mut data);
            let _ = tx.send((index, data));
        })
        .is_ok()
}

fn terminate(tree: Option<&ProcTree>, child: &mut Child) {
    if let Some(tree) = tree {
        // Failure usually means everything has already ended.
        let _ = tree.terminate();
    }
    // On Unix ProcTree sends SIGHUP; SIGKILL for the direct child makes sure it ends.
    let _ = child.kill();
    // Reap it so no zombie remains on Unix. After kill, wait returns immediately.
    let _ = child.wait();
}

/// Version of an entry, from the cache or by querying.
fn cached_version(agent: &Agent, program: &Path, cache: &VersionCache) -> Option<String> {
    let args = version_args(agent.profile.version_args.as_deref(), program);
    if args.is_empty() {
        return None;
    }
    let key = (FileStamp::of(program)?, args);
    if let Some(version) = cache.get(&key) {
        return version;
    }
    let version = probe(program, &key.1, &agent.profile.env, TIMEOUT);
    cache.insert(key, version.clone());
    version
}

/// Queries all entries in parallel: profileId → version or `None`.
pub fn detect(
    installed: &[(Agent, PathBuf)],
    cache: &VersionCache,
) -> HashMap<String, Option<String>> {
    thread::scope(|scope| {
        let jobs: Vec<_> = installed
            .iter()
            .map(|(agent, program)| {
                let job = thread::Builder::new()
                    .name(format!("version-{}", agent.id()))
                    .spawn_scoped(scope, move || cached_version(agent, program, cache));
                (agent.id().to_owned(), job)
            })
            .collect();
        jobs.into_iter()
            .map(|(id, job)| {
                let version = job.ok().and_then(|job| job.join().ok()).flatten();
                (id, version)
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(text: &str) -> Option<String> {
        parse_version(text)
    }

    #[test]
    fn parser_finds_first_version() {
        assert_eq!(v("PowerShell 7.6.9").as_deref(), Some("7.6.9"));
        assert_eq!(
            v("Microsoft Windows [Version 10.0.26300.1234]").as_deref(),
            Some("10.0.26300.1234")
        );
        assert_eq!(v("2.1.3 (Claude Code)").as_deref(), Some("2.1.3"));
        assert_eq!(v("codex-cli 0.46.0").as_deref(), Some("0.46.0"));
        assert_eq!(v("aider 0.86.1.dev42").as_deref(), Some("0.86.1"));
        assert_eq!(v("v1.2").as_deref(), Some("1.2"));
    }

    #[test]
    fn parser_at_most_four_parts_and_no_trailing_dot() {
        assert_eq!(v("1.2.3.4.5").as_deref(), Some("1.2.3.4"));
        assert_eq!(v("Version 3.").as_deref(), None);
        assert_eq!(v("Version 3.x, 4.5.").as_deref(), Some("4.5"));
        assert_eq!(v("1..2 3.4").as_deref(), Some("3.4"));
    }

    #[test]
    fn parser_without_match() {
        assert_eq!(v(""), None);
        assert_eq!(v("unknown option --version"), None);
        assert_eq!(v("build 42"), None);
        // Non-ASCII digits do not count, umlauts before them do no harm.
        assert_eq!(v("äöü ١.٢"), None);
        assert_eq!(v("äöü 1.2").as_deref(), Some("1.2"));
    }

    #[test]
    fn arguments_default_and_exceptions() {
        let p = |s: &str| PathBuf::from(s);
        let powershell = [
            "-NoProfile",
            "-Command",
            "$PSVersionTable.PSVersion.ToString()",
        ];
        assert_eq!(version_args(None, &p("/usr/bin/claude")), ["--version"]);
        assert_eq!(version_args(None, &p("codex.cmd")), ["--version"]);
        assert_eq!(version_args(None, &p("cmd.exe")), ["/d", "/c", "ver"]);
        assert_eq!(version_args(None, &p("PowerShell.EXE")), powershell);
        // pwsh knows --version; only Windows PowerShell 5.1 needs the exception.
        assert_eq!(version_args(None, &p("pwsh.exe")), ["--version"]);
    }

    /// Backslash paths only split into folder and file name on Windows.
    #[cfg(windows)]
    #[test]
    fn arguments_for_full_windows_paths() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(version_args(None, &p(r"C:\npm\codex.cmd")), ["--version"]);
        assert_eq!(
            version_args(None, &p(r"C:\Windows\System32\cmd.exe")),
            ["/d", "/c", "ver"]
        );
        assert_eq!(
            version_args(
                None,
                &p(r"C:\Windows\System32\WindowsPowerShell\v1.0\PowerShell.EXE")
            ),
            [
                "-NoProfile",
                "-Command",
                "$PSVersionTable.PSVersion.ToString()"
            ]
        );
    }

    #[test]
    fn arguments_from_profile_win() {
        let own = vec!["-V".to_owned()];
        assert_eq!(version_args(Some(&own), Path::new("cmd.exe")), ["-V"]);
        assert!(version_args(Some(&[]), Path::new("claude")).is_empty());
    }

    #[test]
    fn version_args_are_read_from_catalog_and_profile() {
        let catalog = crate::agent_catalog::parse(
            "[[agent]]\nid='a'\nname='A'\ncommands=['a']\nversion_args=['-v']\n",
        )
        .unwrap();
        assert_eq!(
            catalog[0].version_args.as_deref(),
            Some(&["-v".to_owned()][..])
        );
        let agent = Agent::from_catalog(catalog[0].clone());
        assert_eq!(agent.profile.version_args, Some(vec!["-v".to_owned()]));

        let profiles =
            crate::profile::parse("[[profile]]\nid='a'\nname='A'\ncommand='a'\nversion_args=[]\n")
                .unwrap();
        assert_eq!(profiles[0].version_args, Some(Vec::new()));
    }

    #[test]
    fn empty_arguments_start_nothing() {
        let mut agent = Agent::from_catalog(
            crate::agent_catalog::parse("[[agent]]\nid='a'\nname='A'\ncommands=['a']\n")
                .unwrap()
                .remove(0),
        );
        agent.profile.version_args = Some(Vec::new());
        let cache = VersionCache::default();
        // A path that would surely fail at startup; empty arguments do not get that far.
        let result = detect(&[(agent, PathBuf::from("does-not-exist"))], &cache);
        assert_eq!(result.get("a"), Some(&None));
    }
}
