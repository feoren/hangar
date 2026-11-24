//! Whether an agent session runs without asking before tools, from its start arguments and from
//! the settings files its CLI reads (ADR 0014, "Agents without prompts"). A link group with such
//! a member asks before pasting, since another agent's message goes straight into its work.
//!
//! What the CLIs read (docs retrieved 2026-11-20, wiki page `unattended-settings-reading`):
//!
//! - Claude Code: `permissions.defaultMode` in the managed settings, `--settings`,
//!   `.claude/settings.local.json`, `.claude/settings.json` and `<config>/settings.json`
//!   (`CLAUDE_CONFIG_DIR` or `~/.claude`).
//! - Codex CLI: `approval_policy` in `<codex home>/config.toml` (`CODEX_HOME` or `~/.codex`),
//!   the chosen profile (`<name>.config.toml`, or `[profiles.<name>]` in older versions) and
//!   `.codex/config.toml` of the working folder and its parents.
//! - Aider: `yes-always` in `.aider.conf.yml` of the home, the git root and the working folder
//!   (or only the `--config` file), `AIDER_YES_ALWAYS` in the environment and in `.env` files.
//! - Gemini CLI and GitHub Copilot CLI cannot be set to run without prompts by a file; their
//!   start arguments decide alone.
//!
//! Every place is read and any one that says so counts, without working out which file wins
//! over which. A wrong "yes" only turns "Ask before pasting" on, which the person can turn off;
//! a wrong "no" would let messages run into an agent that never asks.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::links::runs_without_prompts;
use crate::session::SessionId;

/// Largest settings file read; a bigger one is skipped.
pub const MAX_SETTINGS_BYTES: u64 = 1024 * 1024;
/// How many parent folders are searched for a git root or a Codex project file.
const MAX_PARENTS: usize = 64;

/// What a session started: the program, its arguments, the profile's environment and the
/// working folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Launch {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: PathBuf,
}

/// Where the files are, and how they are read: injected so the rules are tested without the
/// real home folder.
pub struct Places<'a> {
    pub home: &'a Path,
    /// Claude Code's managed settings file of this platform ([`claude_managed_settings`]).
    pub claude_managed: Option<&'a Path>,
    /// The file's text, `None` when it is missing, unreadable or too big.
    pub read: &'a dyn Fn(&Path) -> Option<String>,
    /// Whether a folder exists (for the git root).
    pub is_dir: &'a dyn Fn(&Path) -> bool,
    /// Hangar's own environment, which the child inherits below the profile's.
    pub var: &'a dyn Fn(&str) -> Option<String>,
}

/// The agent CLIs whose settings files can turn prompts off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cli {
    Claude,
    Codex,
    Aider,
}

/// The CLI a command starts, from its file name (`claude`, `claude.exe`, `C:\..\codex.cmd`).
fn cli_of(command: &str) -> Option<Cli> {
    let file = command
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name = [".exe", ".cmd", ".bat", ".ps1"]
        .iter()
        .find_map(|ext| file.strip_suffix(ext))
        .unwrap_or(&file);
    match name {
        "claude" => Some(Cli::Claude),
        "codex" => Some(Cli::Codex),
        "aider" => Some(Cli::Aider),
        _ => None,
    }
}

/// Claude Code's managed settings file on this platform.
pub fn claude_managed_settings() -> Option<PathBuf> {
    if cfg!(windows) {
        Some(PathBuf::from(
            r"C:\Program Files\ClaudeCode\managed-settings.json",
        ))
    } else if cfg!(target_os = "macos") {
        Some(PathBuf::from(
            "/Library/Application Support/ClaudeCode/managed-settings.json",
        ))
    } else if cfg!(unix) {
        Some(PathBuf::from("/etc/claude-code/managed-settings.json"))
    } else {
        None
    }
}

/// `true` when the session's agent runs without asking before tools, by its arguments or by a
/// settings file of its CLI.
pub fn runs_unattended(launch: &Launch, places: &Places<'_>) -> bool {
    if runs_without_prompts(&launch.args) {
        return true;
    }
    match cli_of(&launch.command) {
        Some(Cli::Claude) => claude(launch, places),
        Some(Cli::Codex) => codex(launch, places),
        Some(Cli::Aider) => aider(launch, places),
        None => false,
    }
}

/// A variable as the child sees it: the profile's value, else Hangar's own.
fn variable(launch: &Launch, places: &Places<'_>, name: &str) -> Option<String> {
    launch
        .env
        .get(name)
        .cloned()
        .or_else(|| (places.var)(name))
        .filter(|value| !value.trim().is_empty())
}

/// A folder variable, relative values taken from the working folder as the child would.
fn folder(launch: &Launch, places: &Places<'_>, name: &str, fallback: &str) -> PathBuf {
    match variable(launch, places, name).map(PathBuf::from) {
        Some(path) if path.is_absolute() => path,
        Some(path) => launch.cwd.join(path),
        None => places.home.join(fallback),
    }
}

/// The value after `flag` in `args`, as `--flag value` or `--flag=value`.
fn arg_value<'a>(args: &'a [String], flags: &[&str]) -> Option<&'a str> {
    args.iter().enumerate().find_map(|(index, arg)| {
        flags.iter().find_map(|flag| {
            if arg == flag {
                args.get(index + 1).map(String::as_str)
            } else {
                arg.strip_prefix(flag)?.strip_prefix('=')
            }
        })
    })
}

fn relative_to(cwd: &Path, path: &str) -> PathBuf {
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

/// `permissions.defaultMode` names a mode without prompts. `dontAsk` refuses instead of asking,
/// yet runs what the allow rules let through without a question, so it counts as the argument
/// does.
fn claude_mode_unattended(text: &str) -> bool {
    let Ok(settings) = serde_json::from_str::<Value>(text) else {
        return false;
    };
    settings
        .get("permissions")
        .and_then(|permissions| permissions.get("defaultMode"))
        .and_then(Value::as_str)
        .is_some_and(|mode| {
            let mode = mode.trim().to_ascii_lowercase();
            mode == "bypasspermissions" || mode == "dontask"
        })
}

fn claude(launch: &Launch, places: &Places<'_>) -> bool {
    let config = folder(launch, places, "CLAUDE_CONFIG_DIR", ".claude");
    let project = launch.cwd.join(".claude");
    let mut files = vec![
        config.join("settings.json"),
        project.join("settings.json"),
        project.join("settings.local.json"),
    ];
    files.extend(places.claude_managed.map(Path::to_path_buf));
    if let Some(value) = arg_value(&launch.args, &["--settings"]) {
        if value.trim_start().starts_with('{') {
            if claude_mode_unattended(value) {
                return true;
            }
        } else {
            files.push(relative_to(&launch.cwd, value));
        }
    }
    files
        .iter()
        .filter_map(|file| (places.read)(file))
        .any(|text| claude_mode_unattended(&text))
}

/// `approval_policy = "never"` in a Codex config table.
fn codex_never(table: &toml::Table) -> bool {
    table
        .get("approval_policy")
        .and_then(toml::Value::as_str)
        .is_some_and(|policy| policy.trim().eq_ignore_ascii_case("never"))
}

fn codex(launch: &Launch, places: &Places<'_>) -> bool {
    let home = folder(launch, places, "CODEX_HOME", ".codex");
    let read_table = |path: &Path| -> Option<toml::Table> {
        (places.read)(path).and_then(|text| text.parse::<toml::Table>().ok())
    };
    let user = read_table(&home.join("config.toml"));
    let profile = arg_value(&launch.args, &["--profile", "-p"])
        .map(str::to_owned)
        .or_else(|| user.as_ref()?.get("profile")?.as_str().map(str::to_owned));
    if let Some(user) = &user {
        if codex_never(user) {
            return true;
        }
        let old_profile = profile.as_deref().and_then(|name| {
            user.get("profiles")?
                .as_table()?
                .get(name)?
                .as_table()
                .cloned()
        });
        if old_profile.as_ref().is_some_and(codex_never) {
            return true;
        }
    }
    // A profile name is a file name here: one with a path or a drive (`C:x`) in it is not followed.
    if let Some(name) =
        profile.filter(|name| !name.is_empty() && !name.contains(['/', '\\', '.', ':']))
    {
        if read_table(&home.join(format!("{name}.config.toml"))).is_some_and(|t| codex_never(&t)) {
            return true;
        }
    }
    let ancestors = launch.cwd.ancestors().take(MAX_PARENTS);
    ancestors
        .filter_map(|dir| read_table(&dir.join(".codex").join("config.toml")))
        .any(|table| codex_never(&table))
}

/// A YAML or `.env` truth value as Aider's option parser takes it.
fn truthy(value: &str) -> bool {
    let value = value.trim().trim_matches(['"', '\'']).to_ascii_lowercase();
    matches!(value.as_str(), "true" | "yes" | "on" | "1")
}

/// Strips a trailing ` # comment` from a YAML or `.env` value.
fn without_comment(value: &str) -> &str {
    value.split(" #").next().unwrap_or(value)
}

/// `yes-always: true` as a top-level key of `.aider.conf.yml`. Only that one line is read, so no
/// YAML parser is needed: a top-level key starts in the first column.
fn aider_conf_yes(text: &str) -> bool {
    text.lines().any(|line| {
        line.strip_prefix("yes-always")
            .and_then(|rest| rest.trim_start().strip_prefix(':'))
            .is_some_and(|value| truthy(without_comment(value)))
    })
}

/// `AIDER_YES_ALWAYS=true` in a `.env` file.
fn aider_env_yes(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        let line = line.strip_prefix("export ").unwrap_or(line);
        line.strip_prefix("AIDER_YES_ALWAYS")
            .and_then(|rest| rest.trim_start().strip_prefix('='))
            .is_some_and(|value| truthy(without_comment(value)))
    })
}

fn aider(launch: &Launch, places: &Places<'_>) -> bool {
    if variable(launch, places, "AIDER_YES_ALWAYS").is_some_and(|value| truthy(&value)) {
        return true;
    }
    let git_root = launch
        .cwd
        .ancestors()
        .take(MAX_PARENTS)
        .find(|dir| (places.is_dir)(&dir.join(".git")))
        .map(Path::to_path_buf);
    let mut folders = vec![places.home.to_path_buf()];
    folders.extend(git_root);
    folders.push(launch.cwd.clone());
    let confs: Vec<PathBuf> = match arg_value(&launch.args, &["--config", "-c"]) {
        Some(file) => vec![relative_to(&launch.cwd, file)],
        None => folders
            .iter()
            .map(|dir| dir.join(".aider.conf.yml"))
            .collect(),
    };
    let mut envs: Vec<PathBuf> = folders.iter().map(|dir| dir.join(".env")).collect();
    envs.extend(
        arg_value(&launch.args, &["--env-file"]).map(|file| relative_to(&launch.cwd, file)),
    );
    confs
        .iter()
        .filter_map(|file| (places.read)(file))
        .any(|text| aider_conf_yes(&text))
        || envs
            .iter()
            .filter_map(|file| (places.read)(file))
            .any(|text| aider_env_yes(&text))
}

/// A session whose settings files are still to be read, with the generation of its process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub session: SessionId,
    pub generation: u64,
    pub launch: Launch,
}

/// Reads the real disk for every probe. Blocking file access: call it off the main thread.
pub fn probe(home: &Path, probes: Vec<Probe>) -> Vec<(Probe, bool)> {
    let managed = claude_managed_settings();
    let var = |name: &str| std::env::var(name).ok();
    let is_dir = |path: &Path| is_real_dir(path);
    let places = Places {
        home,
        claude_managed: managed.as_deref(),
        read: &read_settings,
        is_dir: &is_dir,
        var: &var,
    };
    probes
        .into_iter()
        .map(|probe| {
            let value = runs_unattended(&probe.launch, &places);
            (probe, value)
        })
        .collect()
}

/// [`Places::is_dir`] on the real disk: a folder itself, never a symbolic link or junction to
/// one, so a link a repository holds as `.git` does not make its parent a git root.
pub fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// [`Places::read`] on the real disk: a regular file of at most [`MAX_SETTINGS_BYTES`], UTF-8
/// only. A link or reparse point at the file is not followed and the size is bounded while
/// reading ([`crate::persist::read_limited`]): these files come from repositories. A leading
/// byte order mark (some Windows editors write one) is dropped, so the key is still found.
pub fn read_settings(path: &Path) -> Option<String> {
    let max = usize::try_from(MAX_SETTINGS_BYTES).unwrap_or(usize::MAX);
    let bytes = crate::persist::read_limited(path, max).ok()??;
    let text = String::from_utf8(bytes).ok()?;
    Some(
        text.strip_prefix('\u{feff}')
            .map(str::to_owned)
            .unwrap_or(text),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    struct Disk {
        files: BTreeMap<PathBuf, String>,
        dirs: BTreeSet<PathBuf>,
        vars: BTreeMap<String, String>,
    }

    impl Disk {
        fn new() -> Self {
            Self {
                files: BTreeMap::new(),
                dirs: BTreeSet::new(),
                vars: BTreeMap::new(),
            }
        }

        fn file(mut self, path: &str, text: &str) -> Self {
            self.files.insert(PathBuf::from(path), text.to_owned());
            self
        }

        fn dir(mut self, path: &str) -> Self {
            self.dirs.insert(PathBuf::from(path));
            self
        }

        fn var(mut self, name: &str, value: &str) -> Self {
            self.vars.insert(name.to_owned(), value.to_owned());
            self
        }

        fn unattended(&self, launch: &Launch) -> bool {
            let read = |path: &Path| self.files.get(path).cloned();
            let is_dir = |path: &Path| self.dirs.contains(path);
            let var = |name: &str| self.vars.get(name).cloned();
            let places = Places {
                home: Path::new(HOME),
                claude_managed: Some(Path::new(MANAGED)),
                read: &read,
                is_dir: &is_dir,
                var: &var,
            };
            runs_unattended(launch, &places)
        }
    }

    const HOME: &str = "/home/me";
    const MANAGED: &str = "/etc/claude-code/managed-settings.json";
    const CWD: &str = "/work/repo/app";

    fn launch(command: &str, args: &[&str]) -> Launch {
        Launch {
            command: command.to_owned(),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            env: BTreeMap::new(),
            cwd: PathBuf::from(CWD),
        }
    }

    const BYPASS: &str = r#"{"permissions":{"defaultMode":"bypassPermissions"}}"#;
    const ASKING: &str = r#"{"permissions":{"defaultMode":"acceptEdits","allow":["Read"]}}"#;

    #[test]
    fn the_arguments_still_count_for_every_program() {
        let disk = Disk::new();
        assert!(disk.unattended(&launch("gemini", &["--yolo"])));
        assert!(disk.unattended(&launch("claude", &["--dangerously-skip-permissions"])));
        assert!(!disk.unattended(&launch("gemini", &[])));
        assert!(!disk.unattended(&launch("pwsh.exe", &[])));
    }

    #[test]
    fn claude_reads_every_settings_file() {
        let claude = launch(r"C:\tools\claude.exe", &[]);
        for file in [
            "/home/me/.claude/settings.json",
            "/work/repo/app/.claude/settings.json",
            "/work/repo/app/.claude/settings.local.json",
            MANAGED,
        ] {
            assert!(Disk::new().file(file, BYPASS).unattended(&claude), "{file}");
        }
        let dont_ask = r#"{"permissions":{"defaultMode":"dontAsk"}}"#;
        assert!(Disk::new()
            .file("/home/me/.claude/settings.json", dont_ask)
            .unattended(&claude));
        let asking = Disk::new()
            .file("/home/me/.claude/settings.json", ASKING)
            .file("/work/repo/app/.claude/settings.json", "{ not json")
            .file(
                "/work/repo/app/.claude/settings.local.json",
                r#"{"defaultMode":"bypassPermissions"}"#,
            );
        assert!(!asking.unattended(&claude));
        // Another program does not read Claude's files.
        assert!(!Disk::new()
            .file("/home/me/.claude/settings.json", BYPASS)
            .unattended(&launch("codex", &[])));
    }

    #[test]
    fn claude_follows_its_config_folder_and_the_settings_argument() {
        let mut moved = launch("claude", &[]);
        moved
            .env
            .insert("CLAUDE_CONFIG_DIR".into(), "/cfg/claude".into());
        let disk = Disk::new().file("/cfg/claude/settings.json", BYPASS);
        assert!(disk.unattended(&moved));
        assert!(!disk.unattended(&launch("claude", &[])));
        // Hangar's own environment reaches the child too.
        let inherited = Disk::new()
            .var("CLAUDE_CONFIG_DIR", "/cfg/claude")
            .file("/cfg/claude/settings.json", BYPASS);
        assert!(inherited.unattended(&launch("claude", &[])));

        assert!(Disk::new().unattended(&launch("claude", &["--settings", BYPASS])));
        let file = Disk::new().file("/work/repo/app/my.json", BYPASS);
        assert!(file.unattended(&launch("claude", &["--settings=my.json"])));
        assert!(!file.unattended(&launch("claude", &["--settings", "other.json"])));
    }

    #[test]
    fn codex_reads_its_home_profiles_and_project_files() {
        let codex = launch("codex.cmd", &[]);
        let never = "approval_policy = \"never\"\n";
        assert!(Disk::new()
            .file("/home/me/.codex/config.toml", never)
            .unattended(&codex));
        assert!(!Disk::new()
            .file(
                "/home/me/.codex/config.toml",
                "approval_policy = \"on-request\"\n"
            )
            .unattended(&codex));

        let mut moved = launch("codex", &[]);
        moved.env.insert("CODEX_HOME".into(), "/cfg/codex".into());
        assert!(Disk::new()
            .file("/cfg/codex/config.toml", never)
            .unattended(&moved));

        // A project file in the folder or a parent.
        assert!(Disk::new()
            .file("/work/repo/.codex/config.toml", never)
            .unattended(&codex));

        // The chosen profile, as its own file or as an older `[profiles.x]` table.
        let profile_file = Disk::new().file("/home/me/.codex/fast.config.toml", never);
        assert!(profile_file.unattended(&launch("codex", &["--profile", "fast"])));
        assert!(!profile_file.unattended(&codex));
        let table = "profile = \"fast\"\n[profiles.fast]\napproval_policy = \"never\"\n\
                     [profiles.slow]\napproval_policy = \"on-request\"\n";
        assert!(Disk::new()
            .file("/home/me/.codex/config.toml", table)
            .unattended(&codex));
        let unchosen = "[profiles.fast]\napproval_policy = \"never\"\n";
        let disk = Disk::new().file("/home/me/.codex/config.toml", unchosen);
        assert!(!disk.unattended(&codex));
        assert!(disk.unattended(&launch("codex", &["-p", "fast"])));
        // A profile name with a path is not followed.
        let outside = Disk::new().file("/home/me/x.config.toml", never);
        assert!(!outside.unattended(&launch("codex", &["--profile", "../x"])));
        let drive = Disk::new().file("C:x.config.toml", never);
        assert!(!drive.unattended(&launch("codex", &["--profile", "C:x"])));
        assert!(!Disk::new()
            .file("/home/me/.codex/config.toml", "approval_policy = [")
            .unattended(&codex));
    }

    #[test]
    fn aider_reads_its_config_and_env_files() {
        let aider = launch("aider", &[]);
        for file in [
            "/home/me/.aider.conf.yml",
            "/work/repo/.aider.conf.yml",
            "/work/repo/app/.aider.conf.yml",
        ] {
            let disk = Disk::new()
                .dir("/work/repo/.git")
                .file(file, "yes-always: true\n");
            assert!(disk.unattended(&aider), "{file}");
        }
        // Without a git root the parent's file is not read.
        assert!(!Disk::new()
            .file("/work/repo/.aider.conf.yml", "yes-always: true\n")
            .unattended(&aider));
        for line in [
            "yes-always: yes",
            "yes-always : True # ok",
            "yes-always: \"true\"",
        ] {
            assert!(Disk::new()
                .file("/home/me/.aider.conf.yml", line)
                .unattended(&aider));
        }
        for line in [
            "yes-always: false",
            "  yes-always: true",
            "# yes-always: true",
            "auto-commits: true",
        ] {
            assert!(
                !Disk::new()
                    .file("/home/me/.aider.conf.yml", line)
                    .unattended(&aider),
                "{line}"
            );
        }
        // `--config` reads only that file.
        let disk = Disk::new()
            .file("/home/me/.aider.conf.yml", "yes-always: true")
            .file("/work/repo/app/my.yml", "model: x");
        assert!(!disk.unattended(&launch("aider", &["--config", "my.yml"])));

        assert!(Disk::new().var("AIDER_YES_ALWAYS", "1").unattended(&aider));
        let mut profile = launch("aider", &[]);
        profile.env.insert("AIDER_YES_ALWAYS".into(), "true".into());
        assert!(Disk::new().unattended(&profile));
        assert!(Disk::new()
            .file(
                "/work/repo/app/.env",
                "OPENAI_KEY=x\nexport AIDER_YES_ALWAYS=true\n"
            )
            .unattended(&aider));
        assert!(Disk::new()
            .file("/work/repo/app/team.env", "AIDER_YES_ALWAYS=on")
            .unattended(&launch("aider", &["--env-file", "team.env"])));
        assert!(!Disk::new()
            .file("/work/repo/app/.env", "AIDER_YES_ALWAYS=false")
            .unattended(&aider));
    }

    #[test]
    fn programs_are_told_apart_by_file_name() {
        assert_eq!(cli_of("claude"), Some(Cli::Claude));
        assert_eq!(
            cli_of(r"C:\Users\me\AppData\Roaming\npm\codex.cmd"),
            Some(Cli::Codex)
        );
        assert_eq!(cli_of("/usr/local/bin/aider"), Some(Cli::Aider));
        assert_eq!(cli_of("claude-wrapper"), None);
        assert_eq!(cli_of("gemini"), None);
    }

    #[test]
    fn big_files_are_not_read() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/.tmp/unattended");
        std::fs::create_dir_all(&dir).unwrap();
        let small = dir.join("small.json");
        let big = dir.join("big.json");
        std::fs::write(&small, BYPASS).unwrap();
        let marked = dir.join("marked.json");
        std::fs::write(&marked, format!("\u{feff}{BYPASS}")).unwrap();
        assert_eq!(read_settings(&marked).as_deref(), Some(BYPASS));
        std::fs::write(&big, vec![b' '; (MAX_SETTINGS_BYTES + 1) as usize]).unwrap();
        assert_eq!(read_settings(&small).as_deref(), Some(BYPASS));
        assert_eq!(read_settings(&big), None);
        assert_eq!(read_settings(&dir), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A repository may hold a link where a settings file is expected; it could lead to a device,
    /// a FIFO or, on Windows, a network share, so it is never followed.
    #[test]
    fn linked_settings_files_are_not_read() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join(".tmp")
            .join("unattended-link");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("target.json");
        std::fs::write(&target, BYPASS).unwrap();
        let link = dir.join("settings.json");
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&target, &link);
        // File symbolic links need Developer Mode or an elevated process on Windows.
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&target, &link);
        match made {
            Ok(()) => {
                assert_eq!(read_settings(&target).as_deref(), Some(BYPASS));
                assert_eq!(read_settings(&link), None, "a link was followed");
            }
            Err(error) => eprintln!("symbolic link not possible here, case skipped: {error}"),
        }
        // A `.git` link (a junction on Windows, which needs no privilege) is no git root for
        // Aider's files either.
        let repo = dir.join("repo");
        std::fs::create_dir_all(repo.join("app")).unwrap();
        let elsewhere = dir.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        #[cfg(windows)]
        {
            let status = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(repo.join(".git"))
                .arg(&elsewhere)
                .stdout(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "mklink /J failed");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&elsewhere, repo.join(".git")).unwrap();
        assert!(!is_real_dir(&repo.join(".git")), "a linked .git was taken");
        assert!(is_real_dir(&repo));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
