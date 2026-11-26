//! Whether the commands a Codex CLI session runs can see `HANGAR_SEND` (ADR 0012, `hangar-send`),
//! and which `-c` overrides make sure they can.
//!
//! Codex CLI builds the environment of every command its agent runs from its
//! `shell_environment_policy` (Codex 0.172.0, `codex-rs/protocol/src/shell_environment.rs`): start
//! from `inherit` (`all`, `core` or `none`), drop the default excludes (`*KEY*`, `*SECRET*`,
//! `*TOKEN*`) unless `ignore_default_excludes`, drop `exclude`, insert `set`, then keep only
//! `include_only` when it is not empty. Patterns are case-insensitive globs with `*` and `?`.
//! Without a policy of the person's own the default is `inherit = "all"` with
//! `ignore_default_excludes = true`: `HANGAR_SEND` arrives, but so do Hangar's hook tokens and the
//! profile's secrets. Codex hooks do not go through this policy; they get a snapshot of the Codex
//! process's environment, so the gate helper keeps `HANGAR_GATE` whatever the policy says.
//!
//! What a session gets ([`plan`]):
//!
//! - No policy anywhere: only an `exclude` list of Hangar's own token variables and the profile's
//!   `secret_env` names. Codex's default stays as it is otherwise.
//! - `inherit = "all"` whose filters let `HANGAR_SEND` through: the person's `exclude` patterns
//!   again (a `-c` array replaces the file's) plus the same names, when every pattern can pass a
//!   cmd.exe launcher; else nothing.
//! - `inherit = "core"` without an `include_only` of its own: `inherit=all` with an `include_only`
//!   of Codex's core list for the platform, `HANGAR_SEND` and the names the person's `set` adds
//!   (`include_only` filters those as well). The commands see what the person chose plus the one
//!   variable. A `-c` key merges into the file's table, so the person's `set` and `exclude` stay.
//! - Anything else (`inherit = "none"`, a policy that drops `HANGAR_SEND`, a `set` of it, a config
//!   that cannot be read or evaluated, a policy in the start arguments, a policy in a project's
//!   `.codex/config.toml` between the working folder and the repository root): [`Plan::NoSend`]. The
//!   session is not offered sending at all, rather than offered something that cannot work.
//!
//! The selected profile counts like the user config: `--profile`/`-p` names `<name>.config.toml`
//! in Codex's home, whose policy merges key by key over the user config's (measured). The token
//! never goes into an argument. Evidence: docs/wiki/pages/sources/codex-shell-environment-measurement.md.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use crate::agent_models::DefProgram;
use crate::partners::SEND_ENV;

/// Codex CLI's config table for the environment of the commands it runs.
pub const POLICY_KEY: &str = "shell_environment_policy";

/// Largest config file read; a bigger one cannot be evaluated.
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// Hangar's own token variables, which no command of the agent needs: the gate helper and the
/// HTTP hooks run outside the policy, `hangar-send` needs only `HANGAR_SEND`.
const HANGAR_TOKENS: [&str; 3] = [
    crate::hook_recipes::HELPER_ENV,
    crate::gate::TOKEN_ENV,
    crate::gate::NOTIFY_TOKEN_ENV,
];

/// Codex CLI 0.172.0's core variables on Windows (`WINDOWS_CORE_ENV_VARS`), with
/// `PROGRAMFILES` and `PROGRAMFILES(X86)` as one glob: a cmd.exe launcher refuses parentheses.
const WINDOWS_CORE: &[&str] = &[
    "PATH",
    "PATHEXT",
    "SHELL",
    "COMSPEC",
    "SYSTEMROOT",
    "WINDIR",
    "SYSTEMDRIVE",
    "USERNAME",
    "USERDOMAIN",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "PROGRAMFILES*",
    "PROGRAMW6432",
    "PROGRAMDATA",
    "LOCALAPPDATA",
    "APPDATA",
    "TEMP",
    "TMP",
    "TMPDIR",
    "POWERSHELL",
    "PWSH",
];

/// Codex CLI 0.172.0's core variables elsewhere (`UNIX_CORE_ENV_VARS`).
const UNIX_CORE: &[&str] = &[
    "PATH", "SHELL", "TMPDIR", "TEMP", "TMP", "HOME", "LANG", "LC_ALL", "LC_CTYPE", "LOGNAME",
    "USER",
];

fn core_list() -> &'static [&'static str] {
    if cfg!(windows) {
        WINDOWS_CORE
    } else {
        UNIX_CORE
    }
}

/// What one process start is about.
pub struct Start<'a> {
    /// Whose program the agent is ([`crate::agent_catalog::Prompting::agent_program`]).
    pub program: Option<DefProgram>,
    /// The start arguments (`--profile`, a policy override).
    pub args: &'a [String],
    /// The profile's `secret_env` names, already checked to be letters, digits and `_`.
    pub secret_env: &'a [String],
    /// The program runs through a cmd.exe launcher (npm's `codex.cmd`).
    pub batch: bool,
}

/// A config file as read from the disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigFile {
    Missing,
    Text(String),
    /// There, but not a readable regular UTF-8 file within the size limit.
    Unreadable,
}

/// Where Codex's config files are and how they are read; injected so the rules are tested
/// without the real home folder.
pub struct Disk<'a> {
    /// `CODEX_HOME`, else `~/.codex`; `None` when there is no home folder.
    pub codex_home: Option<&'a Path>,
    /// The working folder and its parents up to the repository root ([`project_dirs`]), whose
    /// `.codex/config.toml` Codex may layer over the user config.
    pub project_dirs: &'a [PathBuf],
    pub read: &'a dyn Fn(&Path) -> ConfigFile,
}

/// How many parent folders are searched for a repository root.
const MAX_PARENTS: usize = 64;

/// `cwd` and its parents up to the first one `is_repo_root` names, or up to the root when none
/// is a repository root.
pub fn project_dirs(cwd: &Path, is_repo_root: &dyn Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    for dir in cwd.ancestors().take(MAX_PARENTS) {
        dirs.push(dir.to_path_buf());
        if is_repo_root(dir) {
            break;
        }
    }
    dirs
}

/// [`project_dirs`]' repository root on the real disk: a folder holding `.git`, a folder or the
/// file a linked worktree has.
pub fn is_repo_root(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join(".git")).is_ok()
}

/// Whether a project's `.codex/config.toml` may set the policy: it holds the key at the top or
/// in a profile table, or it is there but cannot be read as TOML. Codex trusts and layers
/// project files by rules Hangar does not follow, so any such file means the plan cannot be
/// told (fail closed). The user's own config in Codex's home is not a project file.
fn project_sets_policy(disk: &Disk<'_>) -> bool {
    disk.project_dirs.iter().any(|dir| {
        let folder = dir.join(".codex");
        if disk.codex_home.is_some_and(|home| home == folder) {
            return false;
        }
        match (disk.read)(&folder.join("config.toml")) {
            ConfigFile::Missing => false,
            ConfigFile::Unreadable => true,
            ConfigFile::Text(text) => match text.parse::<toml::Table>() {
                Err(_) => true,
                Ok(table) => {
                    table.contains_key(POLICY_KEY)
                        || table
                            .get("profiles")
                            .and_then(toml::Value::as_table)
                            .is_some_and(|profiles| {
                                profiles.values().any(|profile| {
                                    profile
                                        .as_table()
                                        .is_none_or(|p| p.contains_key(POLICY_KEY))
                                })
                            })
                }
            },
        }
    })
}

/// What a start of a session that would send gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plan {
    /// Sending works with these arguments before the start arguments (none for any program but
    /// Codex CLI); they must never be dropped, since without them the policy may hand the
    /// person's secrets on or leave `HANGAR_SEND` out.
    Send(Vec<String>),
    /// The commands would not see `HANGAR_SEND`, or that cannot be told: no sending.
    NoSend,
}

/// [`Plan`] for `start`. Reads the user config and the selected profile's file only for Codex.
pub fn plan(start: &Start<'_>, disk: &Disk<'_>) -> Plan {
    if start.program != Some(DefProgram::Codex) {
        return Plan::Send(Vec::new());
    }
    if args_set_policy(start.args) || project_sets_policy(disk) {
        return Plan::NoSend;
    }
    match effective_policy(start.args, disk) {
        Some(policy) => decide(policy.as_ref(), start),
        None => Plan::NoSend,
    }
}

/// The policy table that applies: the user config's, with the selected profile's merged over
/// it. `Some(None)`: no policy anywhere. `None`: not to be evaluated.
fn effective_policy(args: &[String], disk: &Disk<'_>) -> Option<Option<toml::Table>> {
    let read_table = |path: &Path| -> Option<toml::Table> {
        match (disk.read)(path) {
            ConfigFile::Missing => Some(toml::Table::new()),
            ConfigFile::Text(text) => text.parse::<toml::Table>().ok(),
            ConfigFile::Unreadable => None,
        }
    };
    let user = match disk.codex_home {
        Some(home) => read_table(&home.join("config.toml"))?,
        None => toml::Table::new(),
    };
    // Codex 0.172.0 refuses to start with a legacy profile selector or profile table (measured).
    if user.contains_key("profile") || user.contains_key("profiles") {
        return None;
    }
    let mut policy = policy_table(&user)?;
    if let Some(name) = arg_value(args, &["--profile", "-p"]) {
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return None;
        }
        let home = disk.codex_home?;
        let profile = read_table(&home.join(format!("{name}.config.toml")))?;
        if let Some(over) = policy_table(&profile)? {
            let mut merged = policy.unwrap_or_default();
            merge(&mut merged, over);
            policy = Some(merged);
        }
    }
    Some(policy)
}

/// The `shell_environment_policy` table of a config; `Some(None)` without one, `None` when it
/// is not a table.
fn policy_table(config: &toml::Table) -> Option<Option<toml::Table>> {
    match config.get(POLICY_KEY) {
        None => Some(None),
        Some(toml::Value::Table(table)) => Some(Some(table.clone())),
        Some(_) => None,
    }
}

/// `over` merged into `base` key by key, tables recursively, as Codex layers its configs.
fn merge(base: &mut toml::Table, over: toml::Table) {
    for (key, value) in over {
        match (base.get_mut(&key), value) {
            (Some(toml::Value::Table(inner)), toml::Value::Table(over_inner)) => {
                merge(inner, over_inner);
            }
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Inherit {
    All,
    Core,
    None,
}

/// The parts of a policy that decide what reaches a command.
struct Policy {
    inherit: Inherit,
    exclude: Vec<String>,
    include_only: Vec<String>,
    set_names: Vec<String>,
    /// The keyed `filters` form; it cannot be mixed with an `exclude` list Hangar would add.
    filters: bool,
}

fn strings(value: &toml::Value) -> Option<Vec<String>> {
    value
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

/// The policy table read as Codex reads it; `None` for anything it would not take or Hangar
/// cannot tell.
fn parse_policy(table: &toml::Table) -> Option<Policy> {
    let mut policy = Policy {
        inherit: Inherit::All,
        exclude: Vec::new(),
        include_only: Vec::new(),
        set_names: Vec::new(),
        filters: false,
    };
    for (key, value) in table {
        match key.as_str() {
            "inherit" => {
                policy.inherit = match value.as_str()? {
                    "all" => Inherit::All,
                    "core" => Inherit::Core,
                    "none" => Inherit::None,
                    _ => return None,
                }
            }
            "ignore_default_excludes" | "experimental_use_profile" => {
                value.as_bool()?;
            }
            "exclude" => policy.exclude.extend(strings(value)?),
            "include_only" => policy.include_only.extend(strings(value)?),
            "set" => {
                for (name, value) in value.as_table()? {
                    value.as_str()?;
                    policy.set_names.push(name.clone());
                }
            }
            "filters" => {
                policy.filters = true;
                for (pattern, filter) in value.as_table()? {
                    match filter.as_str()? {
                        "include" => policy.include_only.push(pattern.clone()),
                        "exclude" => policy.exclude.push(pattern.clone()),
                        _ => return None,
                    }
                }
            }
            _ => return None,
        }
    }
    // Codex refuses the keyed form next to the legacy lists.
    if policy.filters && (table.contains_key("exclude") || table.contains_key("include_only")) {
        return None;
    }
    Some(policy)
}

/// A name or pattern that passes a cmd.exe launcher and a TOML literal string unchanged.
fn plain(pattern: &str, glob: bool) -> bool {
    !pattern.is_empty()
        && pattern
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || (glob && (b == b'*' || b == b'?')))
}

/// `-c` pairs of `values`, or `None` when one cannot pass a cmd.exe launcher.
fn pairs(values: Vec<String>, batch: bool) -> Option<Vec<String>> {
    if batch && !values.iter().all(|value| crate::batch::argument_ok(value)) {
        return None;
    }
    Some(
        values
            .into_iter()
            .flat_map(|value| ["-c".to_owned(), value])
            .collect(),
    )
}

/// `[ 'a','b' ]` as TOML literal strings: no `"` for a cmd.exe launcher.
fn literal_list<'a>(items: impl IntoIterator<Item = &'a str>) -> String {
    let items: Vec<String> = items.into_iter().map(|item| format!("'{item}'")).collect();
    format!("[{}]", items.join(","))
}

/// Hangar's token variables and the profile's secret names, for an `exclude` list.
fn hangar_excludes<'a>(start: &'a Start<'_>) -> impl Iterator<Item = &'a str> {
    HANGAR_TOKENS.into_iter().chain(
        start
            .secret_env
            .iter()
            .map(String::as_str)
            .filter(|name| plain(name, false)),
    )
}

fn decide(policy: Option<&toml::Table>, start: &Start<'_>) -> Plan {
    let Some(table) = policy else {
        let exclude = format!(
            "{POLICY_KEY}.exclude={}",
            literal_list(hangar_excludes(start))
        );
        return Plan::Send(pairs(vec![exclude], start.batch).unwrap_or_default());
    };
    let Some(policy) = parse_policy(table) else {
        return Plan::NoSend;
    };
    if policy.inherit == Inherit::None
        || policy
            .set_names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(SEND_ENV))
        || matches_any(&policy.exclude, SEND_ENV)
    {
        return Plan::NoSend;
    }
    match policy.inherit {
        Inherit::All => {
            if !policy.include_only.is_empty() && !matches_any(&policy.include_only, SEND_ENV) {
                return Plan::NoSend;
            }
            // `HANGAR_SEND` arrives already; the tokens go too, where that keeps the person's
            // own list intact.
            if policy.filters || !policy.exclude.iter().all(|pattern| plain(pattern, true)) {
                return Plan::Send(Vec::new());
            }
            let list = policy
                .exclude
                .iter()
                .map(String::as_str)
                .chain(hangar_excludes(start));
            let exclude = format!("{POLICY_KEY}.exclude={}", literal_list(list));
            Plan::Send(pairs(vec![exclude], start.batch).unwrap_or_default())
        }
        Inherit::Core => {
            // An `include_only` override next to a keyed `filters` table would stop Codex.
            if policy.filters
                || !policy.include_only.is_empty()
                || !policy.set_names.iter().all(|name| plain(name, false))
            {
                return Plan::NoSend;
            }
            let list = core_list()
                .iter()
                .copied()
                .chain([SEND_ENV])
                .chain(policy.set_names.iter().map(String::as_str));
            let values = vec![
                format!("{POLICY_KEY}.inherit=all"),
                format!("{POLICY_KEY}.include_only={}", literal_list(list)),
            ];
            pairs(values, start.batch).map_or(Plan::NoSend, Plan::Send)
        }
        Inherit::None => Plan::NoSend,
    }
}

fn matches_any(patterns: &[String], name: &str) -> bool {
    patterns.iter().any(|pattern| glob_matches(pattern, name))
}

/// A case-insensitive glob with `*` (any run) and `?` (one character), as Codex matches
/// variable names.
fn glob_matches(pattern: &str, name: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().flat_map(char::to_lowercase).collect();
    let name: Vec<char> = name.chars().flat_map(char::to_lowercase).collect();
    let (mut p, mut n) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while n < name.len() {
        match pattern.get(p) {
            Some('*') => {
                star = Some((p, n));
                p += 1;
            }
            Some(&c) if c == '?' || c == name[n] => {
                p += 1;
                n += 1;
            }
            _ => match star {
                Some((sp, sn)) => {
                    p = sp + 1;
                    n = sn + 1;
                    star = Some((sp, sn + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// Whether the start arguments override the policy themselves (`-c shell_environment_policy...`,
/// `--config shell_environment_policy...`, or the `=` forms).
fn args_set_policy(args: &[String]) -> bool {
    let mut previous_is_flag = false;
    args.iter().any(|arg| {
        let value = if previous_is_flag {
            Some(arg.as_str())
        } else {
            arg.strip_prefix("--config=")
                .or_else(|| arg.strip_prefix("-c").filter(|rest| !rest.is_empty()))
        };
        previous_is_flag = arg == "-c" || arg == "--config";
        value.is_some_and(|value| value.trim_start().starts_with(POLICY_KEY))
    })
}

/// The value after one of `flags`, as `--flag value` or `--flag=value`, and for a short flag
/// also `-fvalue` (clap takes all three).
fn arg_value<'a>(args: &'a [String], flags: &[&str]) -> Option<&'a str> {
    args.iter().enumerate().find_map(|(index, arg)| {
        flags.iter().find_map(|flag| {
            if arg == flag {
                return args.get(index + 1).map(String::as_str);
            }
            let rest = arg.strip_prefix(flag)?;
            let short = !flag.starts_with("--");
            match rest.strip_prefix('=') {
                Some(value) => Some(value),
                None if short && !rest.is_empty() => Some(rest),
                None => None,
            }
        })
    })
}

/// Codex CLI's home: `CODEX_HOME` from the profile's environment first (it is set for the child
/// after Hangar's own), else `~/.codex`. A relative `CODEX_HOME` is taken from the working
/// folder, as the child would.
pub fn codex_home(
    profile_env: &BTreeMap<String, String>,
    var: &dyn Fn(&str) -> Option<String>,
    home: Option<&Path>,
    cwd: &Path,
) -> Option<PathBuf> {
    let codex_home = profile_env
        .get("CODEX_HOME")
        .cloned()
        .or_else(|| var("CODEX_HOME"))
        .filter(|value| !value.trim().is_empty())
        .map(PathBuf::from);
    match codex_home {
        Some(path) if path.is_absolute() => Some(path),
        Some(path) => Some(cwd.join(path)),
        None => Some(home?.join(".codex")),
    }
}

/// [`Disk::read`] on the real disk. Links are followed (dotfile setups link the config); the
/// size is bounded while reading, so a file that grows after a check cannot get through. A
/// leading byte order mark is dropped.
pub fn read_config(path: &Path) -> ConfigFile {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return ConfigFile::Missing,
        Err(_) => return ConfigFile::Unreadable,
    };
    if !file.metadata().is_ok_and(|meta| meta.is_file()) {
        return ConfigFile::Unreadable;
    }
    let mut bytes = Vec::new();
    if file
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || u64::try_from(bytes.len()).map_or(true, |len| len > MAX_CONFIG_BYTES)
    {
        return ConfigFile::Unreadable;
    }
    match String::from_utf8(bytes) {
        Ok(text) => ConfigFile::Text(
            text.strip_prefix('\u{feff}')
                .map(str::to_owned)
                .unwrap_or(text),
        ),
        Err(_) => ConfigFile::Unreadable,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    const HOME: &str = if cfg!(windows) { r"C:\codex" } else { "/codex" };

    /// `files`: name in Codex's home and text; a text of `None` is a file that cannot be read.
    fn plan_with(
        args: &[&str],
        secrets: &[&str],
        batch: bool,
        files: &[(&str, Option<&str>)],
    ) -> Plan {
        let files: BTreeMap<PathBuf, Option<String>> = files
            .iter()
            .map(|(name, text)| (Path::new(HOME).join(name), text.map(str::to_owned)))
            .collect();
        let read = move |path: &Path| match files.get(path) {
            None => ConfigFile::Missing,
            Some(Some(text)) => ConfigFile::Text(text.clone()),
            Some(None) => ConfigFile::Unreadable,
        };
        let args = strings(args);
        let secrets = strings(secrets);
        plan(
            &Start {
                program: Some(DefProgram::Codex),
                args: &args,
                secret_env: &secrets,
                batch,
            },
            &Disk {
                codex_home: Some(Path::new(HOME)),
                project_dirs: &[],
                read: &read,
            },
        )
    }

    const PROJECT: &str = if cfg!(windows) {
        r"C:\repo\app"
    } else {
        "/repo/app"
    };
    const REPO: &str = if cfg!(windows) { r"C:\repo" } else { "/repo" };

    /// A start in [`PROJECT`] inside the repository [`REPO`], without a user config; `files`
    /// holds project config files by path.
    fn plan_in_project(files: &[(&str, Option<&str>)]) -> Plan {
        let files: BTreeMap<PathBuf, Option<String>> = files
            .iter()
            .map(|(path, text)| (PathBuf::from(path), text.map(str::to_owned)))
            .collect();
        let read = move |path: &Path| match files.get(path) {
            None => ConfigFile::Missing,
            Some(Some(text)) => ConfigFile::Text(text.clone()),
            Some(None) => ConfigFile::Unreadable,
        };
        let dirs = project_dirs(Path::new(PROJECT), &|dir| dir == Path::new(REPO));
        plan(
            &Start {
                program: Some(DefProgram::Codex),
                args: &[],
                secret_env: &[],
                batch: false,
            },
            &Disk {
                codex_home: Some(Path::new(HOME)),
                project_dirs: &dirs,
                read: &read,
            },
        )
    }

    fn project_file(dir: &str) -> String {
        Path::new(dir)
            .join(".codex")
            .join("config.toml")
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn a_project_policy_offers_no_sending() {
        let without = plan_in_project(&[]);
        assert!(matches!(&without, Plan::Send(args) if !args.is_empty()));
        // A project file without the key changes nothing.
        let plain = project_file(PROJECT);
        assert_eq!(
            plan_in_project(&[(&plain, Some("model = \"gpt-5\"\n"))]),
            without
        );
        for dir in [PROJECT, REPO] {
            let file = project_file(dir);
            for text in [
                Some("[shell_environment_policy]\ninherit = \"core\"\n"),
                Some("[profiles.work.shell_environment_policy]\ninherit = \"core\"\n"),
                Some("not toml ="),
                None,
            ] {
                assert_eq!(
                    plan_in_project(&[(&file, text)]),
                    Plan::NoSend,
                    "{file}: {text:?}"
                );
            }
        }
        // Above the repository root nothing counts.
        let above = project_file(if cfg!(windows) { r"C:\" } else { "/" });
        assert_eq!(
            plan_in_project(&[(
                &above,
                Some("[shell_environment_policy]\ninherit = \"none\"\n")
            )]),
            without
        );
    }

    #[test]
    fn project_folders_end_at_the_repository_root() {
        let dirs = project_dirs(Path::new(PROJECT), &|dir| dir == Path::new(REPO));
        assert_eq!(dirs, [PathBuf::from(PROJECT), PathBuf::from(REPO)]);
        // Without a repository every parent counts.
        let dirs = project_dirs(Path::new(PROJECT), &|_| false);
        assert_eq!(dirs.len(), 3);
    }

    fn core_include(extra: &[&str]) -> String {
        let list = core_list().iter().copied().chain(extra.iter().copied());
        format!(
            "shell_environment_policy.include_only={}",
            literal_list(list)
        )
    }

    #[test]
    fn without_a_policy_only_hangars_tokens_and_secrets_leave_the_commands() {
        assert_eq!(
            plan_with(
                &["resume", "--last"],
                &["OPENAI_API_KEY", "MY_AUTH"],
                false,
                &[]
            ),
            Plan::Send(strings(&[
                "-c",
                "shell_environment_policy.exclude=['HANGAR_GATE','HANGAR_HOOK_TOKEN',\
                 'HANGAR_NOTIFY_TOKEN','OPENAI_API_KEY','MY_AUTH']",
            ]))
        );
        // A config without the table is the same.
        assert_eq!(
            plan_with(&[], &[], false, &[("config.toml", Some("model = \"gpt-5\"\n[tui]\nx = 1\n"))]),
            Plan::Send(strings(&[
                "-c",
                "shell_environment_policy.exclude=['HANGAR_GATE','HANGAR_HOOK_TOKEN','HANGAR_NOTIFY_TOKEN']",
            ]))
        );
    }

    #[test]
    fn a_core_policy_with_a_set_table_gets_the_variable_and_keeps_its_set() {
        // The owner's own config.
        let config = "[shell_environment_policy]\ninherit = \"core\"\n\n\
                      [shell_environment_policy.set]\nAPI_TIMEOUT_MS = \"1800000\"\n";
        assert_eq!(
            plan_with(&[], &["MY_AUTH"], false, &[("config.toml", Some(config))]),
            Plan::Send(vec![
                "-c".to_owned(),
                "shell_environment_policy.inherit=all".to_owned(),
                "-c".to_owned(),
                core_include(&["HANGAR_SEND", "API_TIMEOUT_MS"]),
            ])
        );
        // The person's exclude stays in the file: the merged key does not touch it.
        let config = "[shell_environment_policy]\ninherit = \"core\"\nexclude = ['TMP*']\n";
        assert_eq!(
            plan_with(&[], &[], false, &[("config.toml", Some(config))]),
            Plan::Send(vec![
                "-c".to_owned(),
                "shell_environment_policy.inherit=all".to_owned(),
                "-c".to_owned(),
                core_include(&["HANGAR_SEND"]),
            ])
        );
    }

    #[test]
    fn an_all_policy_keeps_its_own_excludes_and_adds_the_tokens() {
        let config =
            "[shell_environment_policy]\nexclude = ['AWS_*']\nignore_default_excludes = false\n";
        assert_eq!(
            plan_with(&[], &[], false, &[("config.toml", Some(config))]),
            Plan::Send(strings(&[
                "-c",
                "shell_environment_policy.exclude=['AWS_*','HANGAR_GATE','HANGAR_HOOK_TOKEN',\
                 'HANGAR_NOTIFY_TOKEN']",
            ]))
        );
        // A pattern Hangar cannot pass on unchanged: no hardening, sending still works.
        let config = "[shell_environment_policy]\nexclude = ['A B']\n";
        assert_eq!(
            plan_with(&[], &[], false, &[("config.toml", Some(config))]),
            Plan::Send(Vec::new())
        );
        // The keyed form cannot take a legacy list next to it.
        let config = "[shell_environment_policy.filters]\nAWS_KEY = \"exclude\"\n";
        assert_eq!(
            plan_with(&[], &[], false, &[("config.toml", Some(config))]),
            Plan::Send(Vec::new())
        );
        // An include_only that names it.
        let config = "[shell_environment_policy]\ninclude_only = ['PATH','hangar_*']\n";
        assert_eq!(
            plan_with(&[], &[], false, &[("config.toml", Some(config))]),
            Plan::Send(strings(&[
                "-c",
                "shell_environment_policy.exclude=['HANGAR_GATE','HANGAR_HOOK_TOKEN','HANGAR_NOTIFY_TOKEN']",
            ]))
        );
    }

    #[test]
    fn a_policy_that_cannot_carry_the_variable_offers_no_sending() {
        for config in [
            "[shell_environment_policy]\ninherit = \"none\"\n",
            "[shell_environment_policy]\nexclude = ['HANGAR_*']\n",
            "[shell_environment_policy]\nexclude = ['*send']\n",
            "[shell_environment_policy]\ninclude_only = ['PATH']\n",
            "[shell_environment_policy]\ninherit = \"core\"\ninclude_only = ['PATH']\n",
            "[shell_environment_policy.set]\nHANGAR_SEND = \"x\"\n",
            "[shell_environment_policy.filters]\nHANGAR_SEND = \"exclude\"\n",
            "[shell_environment_policy]\ninherit = \"everything\"\n",
            "[shell_environment_policy]\nunknown = 1\n",
            "[shell_environment_policy]\nexclude = ['A']\nfilters = { B = \"exclude\" }\n",
            "[shell_environment_policy]\ninherit = \"core\"\nfilters = { B = \"exclude\" }\n",
            "[profiles.work]\nmodel = \"gpt-5\"\n",
            "shell_environment_policy = \"all\"\n",
            "profile = \"work\"\n",
            "not toml =",
        ] {
            assert_eq!(
                plan_with(&[], &[], false, &[("config.toml", Some(config))]),
                Plan::NoSend,
                "{config}"
            );
        }
        // A config that is there but cannot be read.
        assert_eq!(
            plan_with(&[], &[], false, &[("config.toml", None)]),
            Plan::NoSend
        );
        // A policy in the start arguments is not evaluated.
        for own in [
            &["-c", "shell_environment_policy.inherit=core"][..],
            &["--config=shell_environment_policy.inherit=none"],
            &["-cshell_environment_policy={inherit='all'}"],
        ] {
            assert_eq!(plan_with(own, &[], false, &[]), Plan::NoSend, "{own:?}");
        }
        // Another override, or the key as a prompt, is not the policy.
        assert!(matches!(
            plan_with(&["-c", "model=gpt-5", "shell_environment_policy"], &[], false, &[]),
            Plan::Send(args) if !args.is_empty()
        ));
    }

    #[test]
    fn the_selected_profile_merges_over_the_user_config() {
        let user = "[shell_environment_policy]\ninherit = \"core\"\n";
        let profile = "[shell_environment_policy.set]\nPROFILE_SET = \"1\"\n";
        let files = [
            ("config.toml", Some(user)),
            ("work.config.toml", Some(profile)),
        ];
        let want = Plan::Send(vec![
            "-c".to_owned(),
            "shell_environment_policy.inherit=all".to_owned(),
            "-c".to_owned(),
            core_include(&["HANGAR_SEND", "PROFILE_SET"]),
        ]);
        assert_eq!(plan_with(&["--profile", "work"], &[], false, &files), want);
        assert_eq!(plan_with(&["-p", "work"], &[], false, &files), want);
        assert_eq!(plan_with(&["--profile=work"], &[], false, &files), want);
        assert_eq!(plan_with(&["-pwork"], &[], false, &files), want);
        assert_eq!(plan_with(&["-p=work"], &[], false, &files), want);
        // A profile that switches inheritance off.
        let off = [(
            "work.config.toml",
            Some("[shell_environment_policy]\ninherit = \"none\"\n"),
        )];
        assert_eq!(plan_with(&["-p", "work"], &[], false, &off), Plan::NoSend);
        // A missing profile file adds nothing; an unreadable one or a name with a path cannot be
        // evaluated.
        assert!(matches!(
            plan_with(&["-p", "work"], &[], false, &[]),
            Plan::Send(_)
        ));
        assert_eq!(
            plan_with(&["-p", "work"], &[], false, &[("work.config.toml", None)]),
            Plan::NoSend
        );
        assert_eq!(plan_with(&["-p", "../x"], &[], false, &[]), Plan::NoSend);
    }

    #[test]
    fn the_overrides_pass_a_cmd_launcher() {
        let config = "[shell_environment_policy]\ninherit = \"core\"\n[shell_environment_policy.set]\nA_1 = \"x\"\n";
        for files in [vec![], vec![("config.toml", Some(config))]] {
            let Plan::Send(args) = plan_with(&[], &["MY_AUTH"], true, &files) else {
                panic!("no sending");
            };
            assert!(!args.is_empty());
            assert!(
                args.iter().all(|arg| crate::batch::argument_ok(arg)),
                "{args:?}"
            );
            // Each value means what it says when Codex reads it as TOML (or as a raw string).
            for value in args.iter().filter(|arg| *arg != "-c") {
                let (_, text) = value.split_once('=').unwrap();
                let parsed = format!("v = {text}").parse::<toml::Table>();
                match text {
                    "all" => assert!(parsed.is_err()),
                    _ => assert!(parsed.unwrap()["v"].is_array(), "{text}"),
                }
            }
        }
    }

    #[test]
    fn other_programs_need_nothing() {
        let read = |_: &Path| -> ConfigFile { panic!("read for another program") };
        for program in [None, Some(DefProgram::Claude), Some(DefProgram::Gemini)] {
            let out = plan(
                &Start {
                    program,
                    args: &[],
                    secret_env: &[],
                    batch: false,
                },
                &Disk {
                    codex_home: Some(Path::new(HOME)),
                    project_dirs: &[],
                    read: &read,
                },
            );
            assert_eq!(out, Plan::Send(Vec::new()), "{program:?}");
        }
    }

    #[test]
    fn globs_match_like_codex() {
        assert!(glob_matches("HANGAR_*", "hangar_send"));
        assert!(glob_matches("*SEND", "HANGAR_SEND"));
        assert!(glob_matches("HANGAR_S?ND", "HANGAR_SEND"));
        assert!(glob_matches("*", "X"));
        assert!(glob_matches("PROGRAMFILES*", "ProgramFiles(x86)"));
        assert!(!glob_matches("HANGAR", "HANGAR_SEND"));
        assert!(!glob_matches("*KEY*", "HANGAR_SEND"));
        assert!(glob_matches("a*b*c", "a-b-c"));
        assert!(!glob_matches("a*b*c", "a-c"));
    }

    #[test]
    fn codex_home_comes_from_the_profile_then_hangar_then_home() {
        let home = Path::new(if cfg!(windows) {
            r"C:\Users\me"
        } else {
            "/home/me"
        });
        let cwd = Path::new(if cfg!(windows) { r"C:\work" } else { "/work" });
        let none = |_: &str| None;
        let empty = BTreeMap::new();
        assert_eq!(
            codex_home(&empty, &none, Some(home), cwd),
            Some(home.join(".codex"))
        );
        assert_eq!(codex_home(&empty, &none, None, cwd), None);
        let elsewhere = home.join("cfg");
        let text = elsewhere.to_string_lossy().into_owned();
        let own = move |name: &str| (name == "CODEX_HOME").then(|| text.clone());
        assert_eq!(codex_home(&empty, &own, Some(home), cwd), Some(elsewhere));
        let profile = BTreeMap::from([("CODEX_HOME".to_owned(), "conf".to_owned())]);
        assert_eq!(
            codex_home(&profile, &own, Some(home), cwd),
            Some(cwd.join("conf"))
        );
    }

    #[test]
    fn reading_tells_missing_from_unreadable() {
        let tmp = crate::test_support::TempDir::new("codex-shell-env");
        let dir = tmp.path();
        assert_eq!(read_config(&dir.join("config.toml")), ConfigFile::Missing);
        let file = dir.join("config.toml");
        std::fs::write(&file, "\u{feff}a = 1\n").unwrap();
        assert_eq!(read_config(&file), ConfigFile::Text("a = 1\n".to_owned()));
        // A folder where the file should be, a file too big, bytes that are no UTF-8.
        assert_eq!(read_config(dir), ConfigFile::Unreadable);
        let big = dir.join("big.toml");
        let max = usize::try_from(MAX_CONFIG_BYTES).unwrap();
        std::fs::write(&big, vec![b' '; max + 1]).unwrap();
        assert_eq!(read_config(&big), ConfigFile::Unreadable);
        std::fs::write(&big, vec![b' '; max]).unwrap();
        assert!(matches!(read_config(&big), ConfigFile::Text(_)));
        let bad = dir.join("bad.toml");
        std::fs::write(&bad, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(read_config(&bad), ConfigFile::Unreadable);
    }
}
