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
            // The version shown is the distribution's release instead ([`release`]).
            version_args: Some(Vec::new()),
            resume_args: None,
            session_id_args: None,
            resume_id_args: None,
            sub_agents: None,
            latest: None,
            update_hint: None,
            packages: None,
            self_update: None,
            agent_commands: Vec::new(),
            models: None,
            model_args: None,
            effort_args: None,
            local: None,
        },
        source: Source::Catalog,
        candidates: vec![command],
        homepage: None,
        install_hint: None,
        cwd_flag: Some("--cd"),
        capabilities: crate::agent_catalog::Capabilities::default(),
        job_breakaway: false,
        prompting: crate::agent_catalog::Prompting::default(),
        npm_variants: Vec::new(),
    }
}

/// The distribution an entry from [`agent`] starts; `None` for every other entry, including a
/// user profile that took over a WSL id (its program is the user's own).
pub fn distro_of(agent: &Agent) -> Option<&str> {
    if agent.source != Source::Catalog {
        return None;
    }
    let suffix = agent.id().strip_prefix(ID_PREFIX)?;
    let [flag, name] = agent.profile.args.as_slice() else {
        return None;
    };
    (flag == "-d" && is_valid_name(name) && suffix == name.to_ascii_lowercase())
        .then_some(name.as_str())
}

/// A distribution as WSL registered it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    pub name: String,
    /// `OsVersion`, the `VERSION_ID` WSL took from the distribution at install.
    pub os_version: Option<String>,
}

/// Release of `distro` (`26.04`, `13`, `rolling`) as WSL recorded it at install, without starting
/// anything: running `cat /etc/os-release` in a stopped distribution would boot it (and the WSL
/// VM) for one number, which takes seconds and keeps it running afterwards. The recorded value
/// does not follow an in-place release upgrade, and distributions installed without it (older
/// Store packages, `wsl --import`) have none.
pub fn release(distro: &str) -> Option<String> {
    release_of(&imp::registered(), distro)
}

/// Shown versions are short and plain; anything else from the registry is not shown.
const MAX_RELEASE_CHARS: usize = 32;

fn release_of(registered: &[Registered], distro: &str) -> Option<String> {
    let found = registered
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(distro))?;
    let text = found.os_version.as_deref()?.trim();
    let plain = !text.is_empty()
        && text.len() <= MAX_RELEASE_CHARS
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'+'));
    plain.then(|| text.to_owned())
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
    /// One subkey per registered distribution, named by its GUID.
    const LXSS: &str = r"Software\Microsoft\Windows\CurrentVersion\Lxss";

    /// The distributions registered for this user, read from [`LXSS`]; empty without WSL.
    pub fn registered() -> Vec<super::Registered> {
        use crate::install_source::registry::{open, string_value, subkeys};
        use windows_sys::Win32::System::Registry::HKEY_CURRENT_USER;

        let Some(list) = open(HKEY_CURRENT_USER, LXSS, 0) else {
            return Vec::new();
        };
        subkeys(&list)
            .iter()
            .filter_map(|guid| {
                let key = open(list.0, guid, 0)?;
                Some(super::Registered {
                    name: string_value(&key, "DistributionName")?,
                    os_version: string_value(&key, "OsVersion"),
                })
            })
            .collect()
    }

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

    pub fn registered() -> Vec<super::Registered> {
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

    fn registered(name: &str, os_version: Option<&str>) -> Registered {
        Registered {
            name: name.to_owned(),
            os_version: os_version.map(str::to_owned),
        }
    }

    #[test]
    fn only_found_entries_name_their_distribution() {
        assert_eq!(distro_of(&agent("Ubuntu-24.04")), Some("Ubuntu-24.04"));
        // A user profile under a WSL id is the user's own program, not this distribution.
        let mut own = agent("Debian");
        own.source = Source::User;
        assert_eq!(distro_of(&own), None);
        // A catalog entry whose id only looks like one.
        let mut other = agent("Arch");
        other.profile.args = vec!["--exec".to_owned(), "Arch".to_owned()];
        assert_eq!(distro_of(&other), None);
        let mut renamed = agent("Arch");
        renamed.profile.id = "wsl-kali".to_owned();
        assert_eq!(distro_of(&renamed), None);
    }

    #[test]
    fn release_comes_from_the_registered_distribution() {
        // Recorded on Windows 11 Pro 10.0.26300 (WSL 2.7.14.0), 2026-11-21: `wsl --install`
        // writes `Flavor` and `OsVersion`; Docker Desktop's distribution has neither.
        let found = [
            registered("docker-desktop", None),
            registered("Ubuntu", Some("26.04")),
            registered("Debian", Some(" 13 ")),
            registered("Arch", Some("rolling")),
            registered("Odd", Some("1.0; rm")),
            registered("Long", Some(&"9".repeat(40))),
        ];
        assert_eq!(release_of(&found, "Ubuntu").as_deref(), Some("26.04"));
        // Distribution names ignore case, as wsl.exe does.
        assert_eq!(release_of(&found, "ubuntu").as_deref(), Some("26.04"));
        assert_eq!(release_of(&found, "Debian").as_deref(), Some("13"));
        assert_eq!(release_of(&found, "Arch").as_deref(), Some("rolling"));
        assert_eq!(release_of(&found, "docker-desktop"), None);
        assert_eq!(release_of(&found, "Odd"), None);
        assert_eq!(release_of(&found, "Long"), None);
        assert_eq!(release_of(&found, "Missing"), None);
    }

    /// Reads the real registry; by hand: `cargo test --lib -- --ignored release_on_this_machine`.
    #[test]
    #[ignore = "reads this machine's WSL registration"]
    fn release_on_this_machine() {
        for entry in imp::registered() {
            eprintln!("{}: {:?}", entry.name, release(&entry.name));
        }
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
