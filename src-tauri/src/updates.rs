//! Newer versions of the installed programs (sources: docs/wiki/pages/agents/updates.md).
//!
//! Per catalog entry or profile, `latest` says where the newest version is published: npm registry,
//! PyPI or a GitHub release. Only the `/latest` endpoints are queried (never lists of all
//! versions, never prereleases), only over HTTPS and only on the three fixed hosts. Besides the
//! address, Hangar sends the user agent `Hangar/<version>` and, once a source has answered, the
//! entity tag that source chose for its answer (`If-None-Match`). No account, cookie or ID of
//! Hangar's own goes out. The tag is the server's value, though: a server that gave each client
//! a tag of its own could recognize that client again by it, as it can by the IP address anyway.
//! Results stay in the disk cache for 25 min, a little less than the frontend's 30 min check
//! interval, so every periodic check really asks; errors silently count as "unknown", for 15 min.
//! A stored `ETag` goes out as `If-None-Match`, and a `304` keeps the known version. A rate limit
//! (`403`/`429` with `Retry-After` or `X-RateLimit-Reset`) pauses that source until the given time.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::agent_catalog::Agent;
use crate::install_source::{
    self, Action, Installer, Packages, PlanKind, Probe, Reason, UpdatePlan, Warning,
};
use crate::persist::{read_limited, write_atomic};
use crate::version::{normalize_windows_suffix, parse_version};

pub const CACHE_FILE: &str = "update-cache.json";
const CACHE_VERSION: u32 = 1;
const CACHE_MAX_BYTES: usize = 1024 * 1024;
/// Below the frontend's `UPDATE_CHECK_INTERVAL_MS` (30 min), so a periodic check never finds
/// a cached answer from the previous round.
pub const CACHE_TTL_MS: u64 = 25 * 60 * 1000;
pub const FAILURE_TTL_MS: u64 = 15 * 60 * 1000;
/// GitHub: "wait for at least one minute" when a rate limit names no time.
pub const RATE_LIMIT_MIN_BACKOFF_MS: u64 = 60 * 1000;
/// Upper bound for a backoff a server asks for; the hourly GitHub window never needs more.
pub const RATE_LIMIT_MAX_BACKOFF_MS: u64 = 6 * 60 * 60 * 1000;
/// Longer entity tags are not stored; real ones are short quoted hashes.
const ETAG_MAX_LENGTH: usize = 200;
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// PyPI returns all releases along with the package; nothing beyond this is ever read.
const RESPONSE_LIMIT: usize = 8 * 1024 * 1024;
/// At most this many concurrent requests.
pub const PARALLEL: usize = 4;
const ALLOWED_HOSTS: &[&str] = &["registry.npmjs.org", "pypi.org", "api.github.com"];

/// Field `latest` in catalog and profile, such as `{ npm = "@openai/codex" }`,
/// `{ pypi = "aider-chat" }` or `{ github = "PowerShell/PowerShell", tag_prefix = "v" }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Latest {
    #[serde(default)]
    pub npm: Option<String>,
    #[serde(default)]
    pub pypi: Option<String>,
    #[serde(default)]
    pub github: Option<String>,
    /// Stripped from the GitHub tag, such as `v` in `v7.5.4`.
    #[serde(default)]
    pub tag_prefix: Option<String>,
    #[serde(default)]
    pub compare: Compare,
}

/// `newer`: update if the newest version is numerically greater. `equal`: update if it
/// differs; for versions that cannot be ordered meaningfully (amp: `0.0.<time>-g<hash>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Compare {
    #[default]
    Newer,
    Equal,
}

/// Field `update_hint`: the update command. It is shown and, on request, run as a session of its
/// own ([`update_command`]), hence a simple command without shell syntax. If it is missing
/// for the platform, Hangar only shows the install command (never runs it).
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlatformHint {
    #[serde(default)]
    pub windows: Option<String>,
    #[serde(default)]
    pub unix: Option<String>,
}

impl PlatformHint {
    pub fn for_platform(&self) -> Option<&String> {
        if cfg!(windows) {
            self.windows.as_ref()
        } else {
            self.unix.as_ref()
        }
    }
}

/// Characters that only a shell would interpret (chaining, redirection, variables, quoting,
/// wildcards, comments). Nothing is started through a shell, so a command with such characters
/// would not do what it promises. `%`, `^` and `!` are included because `.cmd` programs such as
/// `npm.cmd` run through cmd.exe on Windows, where they mean variables and escaping.
///
/// The block prevents wrong splitting and interpretation by cmd.exe; it is not a
/// sandbox. `update_hint` is exactly as trustworthy as `command`: whoever writes the catalog or
/// profiles.toml decides what Hangar starts anyway.
const SHELL_SYNTAX: &[char] = &[
    '|', '&', ';', '<', '>', '$', '`', '(', ')', '"', '\'', '%', '^', '!', '*', '?', '[', ']', '{',
    '}', '#', '~',
];

/// The first character a shell would interpret, or a control character other than tab.
pub(crate) fn has_shell_syntax(text: &str) -> Option<char> {
    text.chars()
        .find(|c| SHELL_SYNTAX.contains(c) || (c.is_control() && !matches!(c, ' ' | '\t')))
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    #[error("No update command is known for this program on this platform")]
    Missing,
    #[error("Hangar cannot run the update itself ({0:?})")]
    NoPlan(Reason),
    #[error("The update command is empty")]
    Empty,
    #[error(
        "The update command contains '{0}'; only a plain command without shell syntax is allowed"
    )]
    ShellSyntax(char),
    #[error("The program '{0}' in the update command is a relative path; only a bare name or an absolute path is allowed")]
    RelativeProgram(String),
}

/// Splits an update command at whitespace into an argv array. An empty command is rejected, and
/// so is any with shell syntax or control characters, rather than running it half right. The program
/// (`argv[0]`) is a bare name (PATH search) or an absolute path; a relative path would depend
/// on the working folder.
pub fn split_command(command: &str) -> Result<Vec<String>, CommandError> {
    if let Some(bad) = has_shell_syntax(command) {
        return Err(CommandError::ShellSyntax(bad));
    }
    let argv: Vec<String> = command.split_whitespace().map(str::to_owned).collect();
    let Some(program) = argv.first() else {
        return Err(CommandError::Empty);
    };
    let has_separator = program.contains('/') || program.contains('\\');
    if has_separator && !Path::new(program).is_absolute() {
        return Err(CommandError::RelativeProgram(program.clone()));
    }
    Ok(argv)
}

/// Why a plan without a run action cannot start: the exact splitting error of an invalid
/// `update_hint`, `Missing` for a program that is not installed, otherwise the plan's reason.
fn not_runnable(agent: &Agent, plan: &UpdatePlan) -> CommandError {
    match plan.reason {
        Some(Reason::InvalidHint) => agent
            .profile
            .update_hint
            .as_ref()
            .and_then(PlatformHint::for_platform)
            .map_or(CommandError::Missing, |hint| {
                split_command(hint).err().unwrap_or(CommandError::Missing)
            }),
        None | Some(Reason::NotInstalled) => CommandError::Missing,
        Some(reason) => CommandError::NoPlan(reason),
    }
}

/// Entry that starts the program's update instead of the program, from the plan
/// ([`crate::install_source::plan`]): the install command never runs, it is often a shell
/// pipeline (`irm … | iex`) and meant for reading only. A bare program name (from an
/// `update_hint`) is looked up on the PATH; planned tools are already absolute. The profile's
/// environment and fixed working folder stay, so that a custom npm prefix from `env`, for
/// example, also applies to the update.
pub fn update_agent(agent: &Agent, plan: &UpdatePlan) -> Result<Agent, CommandError> {
    let Action::Run { program, args } = &plan.action else {
        return Err(not_runnable(agent, plan));
    };
    if program.trim().is_empty() {
        return Err(CommandError::Empty);
    }
    let command = program.clone();
    let mut updater = agent.clone();
    updater.profile.command.clone_from(&command);
    updater.profile.args.clone_from(args);
    updater.profile.resume_args = None;
    updater.profile.session_id_args = None;
    updater.profile.resume_id_args = None;
    updater.profile.sub_agents = None;
    // The update command needs no API keys of the agent.
    updater.profile.secret_env.clear();
    updater.cwd_flag = None;
    updater.candidates = vec![command];
    Ok(updater)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Source {
    Npm(String),
    Pypi(String),
    Github { repo: String, tag_prefix: String },
}

pub(crate) fn npm_name_ok(name: &str) -> bool {
    let part = |p: &str| {
        !p.is_empty()
            && p.bytes().all(|b| {
                b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || matches!(b, b'-' | b'.' | b'_' | b'~')
            })
    };
    match name.strip_prefix('@') {
        Some(scoped) => scoped
            .split_once('/')
            .is_some_and(|(scope, pkg)| part(scope) && part(pkg)),
        None => part(name) && !name.starts_with('.'),
    }
}

pub(crate) fn plain_name_ok(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_'))
}

impl Latest {
    /// Exactly one source with a valid name; so nothing foreign ever gets into the address.
    pub fn source(&self) -> Result<Source, &'static str> {
        match (&self.npm, &self.pypi, &self.github) {
            (Some(name), None, None) if npm_name_ok(name) => Ok(Source::Npm(name.clone())),
            (None, Some(name), None) if plain_name_ok(name) => Ok(Source::Pypi(name.clone())),
            (None, None, Some(repo))
                if repo
                    .split_once('/')
                    .is_some_and(|(owner, name)| plain_name_ok(owner) && plain_name_ok(name)) =>
            {
                Ok(Source::Github {
                    repo: repo.clone(),
                    tag_prefix: self.tag_prefix.clone().unwrap_or_default(),
                })
            }
            (Some(_), None, None) | (None, Some(_), None) | (None, None, Some(_)) => {
                Err("latest: invalid package or repository name")
            }
            _ => Err("latest needs exactly one source: npm, pypi or github"),
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        let source = self.source()?;
        if self.tag_prefix.is_some() && !matches!(source, Source::Github { .. }) {
            return Err("latest: tag_prefix only applies to github");
        }
        Ok(())
    }
}

impl Source {
    pub fn url(&self) -> String {
        match self {
            // The slash of a scope is encoded, as the registry API expects.
            Self::Npm(name) => format!(
                "https://registry.npmjs.org/{}/latest",
                name.replace('/', "%2f")
            ),
            Self::Pypi(name) => format!("https://pypi.org/pypi/{name}/json"),
            Self::Github { repo, .. } => {
                format!("https://api.github.com/repos/{repo}/releases/latest")
            }
        }
    }

    pub fn cache_key(&self) -> String {
        match self {
            Self::Npm(name) => format!("npm:{name}"),
            Self::Pypi(name) => format!("pypi:{name}"),
            Self::Github { repo, tag_prefix } => format!("github:{repo}:{tag_prefix}"),
        }
    }

    fn accept(&self) -> &'static str {
        match self {
            Self::Github { .. } => "application/vnd.github+json",
            _ => "application/json",
        }
    }

    /// Version from the response; `None` if the field is missing or does not look like a version.
    pub fn parse(&self, body: &[u8]) -> Option<String> {
        let json: serde_json::Value = serde_json::from_slice(body).ok()?;
        let raw = match self {
            Self::Npm(_) => json.get("version")?.as_str()?.trim().to_owned(),
            Self::Pypi(_) => json
                .get("info")?
                .get("version")?
                .as_str()?
                .trim()
                .to_owned(),
            Self::Github { tag_prefix, .. } => {
                // /releases/latest never returns drafts or prereleases; checking twice does no harm.
                let flag = |name: &str| json.get(name).and_then(|v| v.as_bool()) == Some(true);
                if flag("draft") || flag("prerelease") {
                    return None;
                }
                let tag = json.get("tag_name")?.as_str()?.trim();
                tag.strip_prefix(tag_prefix.as_str())
                    .unwrap_or(tag)
                    .to_owned()
            }
        };
        // Git for Windows tags `2.56.0.windows.1`; shown and compared as `2.56.0.1`.
        let raw = normalize_windows_suffix(&raw);
        parse_version(&raw).map(|_| raw)
    }
}

fn numeric(version: &str) -> Option<[u64; 4]> {
    let version = parse_version(version)?;
    let mut parts = [0u64; 4];
    for (slot, part) in parts.iter_mut().zip(version.split('.')) {
        *slot = part.parse().ok()?;
    }
    Some(parts)
}

/// Whether `latest` is an update over `installed`. Not comparable means no.
pub fn update_available(installed: &str, latest: &str, compare: Compare) -> bool {
    match compare {
        Compare::Newer => {
            matches!((numeric(installed), numeric(latest)), (Some(i), Some(l)) if l > i)
        }
        Compare::Equal => matches!(
            (parse_version(installed), parse_version(latest)),
            (Some(i), Some(l)) if i != l
        ),
    }
}

fn github_latest(repo: &str) -> Latest {
    Latest {
        npm: None,
        pypi: None,
        github: Some(repo.into()),
        tag_prefix: Some("v".into()),
        compare: Compare::Newer,
    }
}

fn winget_package(id: &str) -> Packages {
    Packages {
        winget: Some(id.into()),
        ..Packages::default()
    }
}

/// Built-in source for known shells from user profiles that were created before this field
/// and therefore have no `latest` (the default profiles are only written on first start).
pub fn builtin_for_command(command: &str) -> Option<(Latest, Packages)> {
    let stem = Path::new(command)
        .file_stem()?
        .to_str()?
        .to_ascii_lowercase();
    match stem.as_str() {
        "pwsh" => Some((
            github_latest("PowerShell/PowerShell"),
            winget_package("Microsoft.PowerShell"),
        )),
        _ => None,
    }
}

/// Built-in source that needs the resolved program: Git Bash belongs to Git for Windows, whose
/// version and update are those of Git (`git --version`, winget `Git.Git`).
pub fn builtin_for_program(program: &Path, probe: &dyn Probe) -> Option<(Latest, Packages)> {
    if !cfg!(windows) {
        return None;
    }
    install_source::git_for_windows(program, probe)?;
    Some((
        github_latest("git-for-windows/git"),
        winget_package("Git.Git"),
    ))
}

/// Adds the built-in source of [`builtin_for_program`] to an entry that has none of its own.
pub fn enrich(agent: &mut Agent, program: &Path, probe: &dyn Probe) {
    if agent.profile.latest.is_some() {
        return;
    }
    if let Some((latest, packages)) = builtin_for_program(program, probe) {
        agent.profile.latest = Some(latest);
        agent.profile.packages.get_or_insert(packages);
    }
}

/// Why the version did not change after an update session, for the frontend to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OutcomeReason {
    /// The program still runs in a session; on Windows its files stay locked.
    Running,
    /// The update ended with a nonzero exit code.
    Failed,
    /// Another copy exists; the update went to one that is not the started copy.
    Shadowed,
    /// The update reported success but the started copy has the same version.
    Unchanged,
}

/// Event `program-update-result` after an update session ended and the version was read again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateOutcome {
    pub profile_id: String,
    pub session_id: Option<u64>,
    pub code: Option<u32>,
    pub before: Option<String>,
    pub after: Option<String>,
    /// `null` if one of the two versions is unknown.
    pub changed: Option<bool>,
    pub reason: Option<OutcomeReason>,
    pub copies: Vec<PathBuf>,
}

/// `None` when the version changed, and when a successful update has no version to compare
/// (nothing is known to be wrong). A program still running explains an unchanged version or a
/// failure best, then a failed update, then a second copy.
pub fn outcome_reason(
    before: Option<&str>,
    after: Option<&str>,
    code: Option<u32>,
    running: bool,
    second_copy: bool,
) -> Option<OutcomeReason> {
    let changed = match (before, after) {
        (Some(b), Some(a)) => Some(b != a),
        _ => None,
    };
    if changed == Some(true) || (changed.is_none() && code == Some(0)) {
        return None;
    }
    if running {
        Some(OutcomeReason::Running)
    } else if code != Some(0) {
        Some(OutcomeReason::Failed)
    } else if second_copy {
        Some(OutcomeReason::Shadowed)
    } else if changed == Some(false) {
        Some(OutcomeReason::Unchanged)
    } else {
        None
    }
}

// --- Disk cache ---

/// Both optional fields came later; older cache files without them stay valid.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    pub latest: Option<String>,
    /// Unix time in milliseconds of the answer `latest` comes from (a `304` counts as one).
    pub checked_at: u64,
    /// Entity tag of the response `latest` was read from, sent back as `If-None-Match`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    /// Unix time in milliseconds before which the source must not be asked (rate limit).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_at: Option<u64>,
}

impl CacheEntry {
    /// The source asked to wait and the time has not come yet.
    /// A time further away than [`RATE_LIMIT_MAX_BACKOFF_MS`] (clock turned back) does not count.
    pub fn backing_off(&self, now: u64) -> bool {
        self.retry_at
            .is_some_and(|at| now < at && at - now <= RATE_LIMIT_MAX_BACKOFF_MS)
    }

    /// `If-None-Match` for the next request: only with a known version that a `304` can keep.
    pub fn if_none_match(&self) -> Option<&str> {
        self.latest.as_ref().and(self.etag.as_deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCache {
    version: u32,
    entries: BTreeMap<String, CacheEntry>,
}

impl UpdateCache {
    pub fn path(dir: &Path) -> PathBuf {
        dir.join(CACHE_FILE)
    }

    /// If the file is missing or unusable, the cache starts empty.
    pub fn load(dir: &Path) -> Self {
        let path = Self::path(dir);
        let cache = match read_limited(&path, CACHE_MAX_BYTES) {
            Ok(Some(bytes)) => serde_json::from_slice::<Self>(&bytes).ok(),
            Ok(None) => None,
            Err(reason) => {
                eprintln!("hangar: ignoring {}: {reason}", path.display());
                None
            }
        };
        cache
            .filter(|c| c.version == CACHE_VERSION)
            .unwrap_or_else(|| Self {
                version: CACHE_VERSION,
                entries: BTreeMap::new(),
            })
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        write_atomic(&Self::path(dir), &json)
    }

    /// Entry still valid: hits for [`CACHE_TTL_MS`], failures for [`FAILURE_TTL_MS`], and any
    /// entry whose source asked to wait, until then.
    pub fn fresh(&self, key: &str, now: u64) -> Option<&CacheEntry> {
        let entry = self.entries.get(key)?;
        if entry.backing_off(now) {
            return Some(entry);
        }
        let ttl = if entry.latest.is_some() {
            CACHE_TTL_MS
        } else {
            FAILURE_TTL_MS
        };
        (now.saturating_sub(entry.checked_at) < ttl && entry.checked_at <= now).then_some(entry)
    }

    pub fn put(&mut self, key: String, entry: CacheEntry) {
        self.version = CACHE_VERSION;
        self.entries.insert(key, entry);
    }
}

// --- Query ---

/// How the update of an entry would run, as the frontend sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanView {
    pub kind: PlanKind,
    /// [`UpdatePlan::fingerprint`].
    pub id: String,
    pub reason: Option<Reason>,
    pub needs_admin: bool,
    pub command: Option<String>,
    pub installer: Option<Installer>,
    pub warnings: Vec<Warning>,
    pub copies: Vec<PathBuf>,
}

impl PlanView {
    /// `running`: the program runs in a session right now.
    pub fn of(plan: &UpdatePlan, running: bool) -> Self {
        let mut warnings = plan.warnings.clone();
        if running {
            warnings.push(Warning::Running);
        }
        Self {
            kind: plan.kind(),
            id: plan.fingerprint(),
            reason: plan.reason,
            needs_admin: plan.needs_admin,
            command: plan.command(),
            installer: plan.installer,
            warnings,
            copies: plan.copies.clone(),
        }
    }
}

/// An installed entry with `latest`.
#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    pub source: Source,
    pub compare: Compare,
    pub installed: Option<String>,
    pub plan: PlanView,
}

impl Item {
    /// `None` without a valid `latest`.
    pub fn of(agent: &Agent, installed: Option<String>, plan: PlanView) -> Option<Self> {
        let latest = agent.profile.latest.as_ref()?;
        Some(Self {
            id: agent.id().to_owned(),
            source: latest.source().ok()?,
            compare: latest.compare,
            installed,
            plan,
        })
    }
}

/// One item per installed entry with `latest`, each with its update plan. `running`: profiles
/// that run in a session right now.
pub fn items(
    installed: &[(Agent, PathBuf)],
    versions: &HashMap<String, Option<String>>,
    running: &HashSet<String>,
    probe: &dyn Probe,
) -> Vec<Item> {
    installed
        .iter()
        .filter(|(agent, _)| agent.profile.latest.is_some())
        .filter_map(|(agent, program)| {
            let plan = install_source::plan(
                &install_source::PlanInput {
                    profile: &agent.profile,
                    install_hint: agent.install_hint.as_deref(),
                    program: Some(program),
                    candidates: &agent.candidates,
                },
                probe,
            );
            let view = PlanView::of(&plan, running.contains(agent.id()));
            Item::of(agent, versions.get(agent.id()).cloned().flatten(), view)
        })
        .collect()
}

/// The event after an update session: versions before and after, and why nothing changed.
#[allow(clippy::too_many_arguments)] // All values describe exactly this one update.
pub fn outcome(
    profile_id: &str,
    session_id: Option<u64>,
    code: Option<u32>,
    before: Option<String>,
    after: Option<String>,
    running: bool,
    copies: Vec<PathBuf>,
) -> UpdateOutcome {
    let reason = outcome_reason(
        before.as_deref(),
        after.as_deref(),
        code,
        running,
        copies.len() > 1,
    );
    let changed = match (&before, &after) {
        (Some(b), Some(a)) => Some(b != a),
        _ => None,
    };
    UpdateOutcome {
        profile_id: profile_id.to_owned(),
        session_id,
        code,
        before,
        after,
        changed,
        reason,
        copies,
    }
}

pub fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Result per profileId (contract: docs/wiki/pages/syntheses/program-updates.md).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub installed: Option<String>,
    pub latest: Option<String>,
    pub update_available: bool,
    /// Same as `command`; kept for older frontends.
    pub update_hint: Option<String>,
    /// `plan == "run"`; kept for older frontends.
    pub can_run: bool,
    /// Unix time in milliseconds of the query that `latest` comes from.
    pub checked_at: u64,
    /// `run`: `create_update_session` starts `command`. `copy`: `command` is for copying only.
    /// `none`: nothing to offer.
    pub plan: PlanKind,
    /// Why there is no `run` plan (stable code), otherwise `null`.
    pub reason: Option<Reason>,
    /// The install is machine-wide; the update asks for administrator rights.
    pub needs_admin: bool,
    /// Display text of the command (never run from this text).
    pub command: Option<String>,
    /// How the started copy was installed, if known.
    pub installer: Option<Installer>,
    pub warnings: Vec<Warning>,
    /// Every copy of the program found, the started one first.
    pub copies: Vec<PathBuf>,
    /// Fingerprint of the plan; `create_update_session` refuses if it no longer matches.
    pub plan_id: String,
}

pub fn result(item: &Item, entry: &CacheEntry) -> UpdateInfo {
    let update_available = match (&item.installed, &entry.latest) {
        (Some(installed), Some(latest)) => update_available(installed, latest, item.compare),
        _ => false,
    };
    let plan = &item.plan;
    UpdateInfo {
        installed: item.installed.clone(),
        latest: entry.latest.clone(),
        update_available,
        update_hint: plan.command.clone(),
        can_run: plan.kind == PlanKind::Run,
        checked_at: entry.checked_at,
        plan: plan.kind,
        reason: plan.reason,
        needs_admin: plan.needs_admin,
        command: plan.command.clone(),
        installer: plan.installer,
        warnings: plan.warnings.clone(),
        copies: plan.copies.clone(),
        plan_id: plan.id.clone(),
    }
}

/// Sources that need a fresh query (without duplicates), in a stable order. `force` bypasses
/// the cache but not a rate limit: asking again before the given time only extends it.
pub fn stale_sources(items: &[Item], cache: &UpdateCache, now: u64, force: bool) -> Vec<Source> {
    let mut seen = std::collections::HashSet::new();
    items
        .iter()
        .map(|item| &item.source)
        .filter(|source| {
            let key = source.cache_key();
            let backing_off = cache
                .entries
                .get(&key)
                .is_some_and(|entry| entry.backing_off(now));
            !backing_off && (force || cache.fresh(&key, now).is_none())
        })
        .filter(|source| seen.insert(source.cache_key()))
        .cloned()
        .collect()
}

/// Rustls needs a process-wide crypto provider; the same one as tauri-plugin-updater.
pub(crate) fn ensure_crypto_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        // Fails only if another thread has just set one; that is fine then.
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
}

pub fn client() -> Result<reqwest::Client, reqwest::Error> {
    ensure_crypto_provider();
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        let url = attempt.url();
        let allowed = url.scheme() == "https"
            && url.host_str().is_some_and(|h| ALLOWED_HOSTS.contains(&h))
            && attempt.previous().len() < 3;
        if allowed {
            attempt.follow()
        } else {
            attempt.stop()
        }
    });
    reqwest::Client::builder()
        .user_agent(format!("Hangar/{}", env!("CARGO_PKG_VERSION")))
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(REQUEST_TIMEOUT)
        .https_only(true)
        .redirect(policy)
        .build()
}

/// What a source answered, reduced to what decides the cache entry ([`settle`]).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reply {
    pub status: u16,
    pub etag: Option<String>,
    pub retry_after: Option<String>,
    pub rate_limit_remaining: Option<String>,
    pub rate_limit_reset: Option<String>,
    /// Body of a successful response; `None` otherwise, on a read error or over the limit.
    pub body: Option<Vec<u8>>,
}

/// Unix time in ms until which a rate-limited source must wait, `None` if this is no rate limit.
/// GitHub answers both limits with `403` or `429`: `Retry-After` (seconds) comes first, then
/// `X-RateLimit-Reset` (epoch seconds) when `X-RateLimit-Remaining` is `0`, otherwise at least
/// one minute. A `403` without either signal is an ordinary failure (such as a blocked
/// repository). Always between one minute and [`RATE_LIMIT_MAX_BACKOFF_MS`] from `now`.
pub fn rate_limited_until(reply: &Reply, now: u64) -> Option<u64> {
    if !matches!(reply.status, 403 | 429) {
        return None;
    }
    let seconds = |value: &Option<String>| {
        value
            .as_deref()
            .and_then(|text| text.trim().parse::<u64>().ok())
    };
    let exhausted = reply.rate_limit_remaining.as_deref().map(str::trim) == Some("0");
    let until = if let Some(wait) = seconds(&reply.retry_after) {
        now.saturating_add(wait.saturating_mul(1000))
    } else if exhausted {
        seconds(&reply.rate_limit_reset).map_or(0, |reset| reset.saturating_mul(1000))
    } else if reply.status == 429 {
        0
    } else {
        return None;
    };
    Some(until.clamp(
        now.saturating_add(RATE_LIMIT_MIN_BACKOFF_MS),
        now.saturating_add(RATE_LIMIT_MAX_BACKOFF_MS),
    ))
}

/// The cache entry after an answer (`None`: no answer at all). A rate limit keeps what was
/// known and only adds the pause; a `304` confirms the known version as of `now`; a success
/// stores the version together with its entity tag; anything else is a failure.
pub fn settle(
    source: &Source,
    prior: Option<&CacheEntry>,
    reply: Option<&Reply>,
    now: u64,
) -> CacheEntry {
    let failure = CacheEntry {
        latest: None,
        checked_at: now,
        ..CacheEntry::default()
    };
    let Some(reply) = reply else {
        return failure;
    };
    if let Some(retry_at) = rate_limited_until(reply, now) {
        let mut kept = prior.cloned().unwrap_or(failure);
        kept.retry_at = Some(retry_at);
        return kept;
    }
    match reply.status {
        304 => match prior.filter(|entry| entry.latest.is_some()) {
            Some(entry) => CacheEntry {
                latest: entry.latest.clone(),
                checked_at: now,
                etag: reply.etag.clone().or_else(|| entry.etag.clone()),
                retry_at: None,
            },
            // Nothing known to keep: only possible if the server answers a plain request so.
            None => failure,
        },
        200..=299 => {
            let latest = reply.body.as_deref().and_then(|body| source.parse(body));
            let etag = latest.as_ref().and(reply.etag.clone());
            CacheEntry {
                latest,
                checked_at: now,
                etag,
                retry_at: None,
            }
        }
        _ => failure,
    }
}

/// The request for a source; `if_none_match` makes it conditional. An entity tag that is no
/// valid header value is left out rather than sent half.
pub fn request(
    client: &reqwest::Client,
    source: &Source,
    if_none_match: Option<&str>,
) -> reqwest::RequestBuilder {
    let request = client
        .get(source.url())
        .header(reqwest::header::ACCEPT, source.accept());
    match if_none_match.and_then(|tag| reqwest::header::HeaderValue::from_str(tag).ok()) {
        Some(tag) => request.header(reqwest::header::IF_NONE_MATCH, tag),
        None => request,
    }
}

/// A short header value as text; anything long or not visible ASCII is dropped.
fn header_text(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    let text = headers.get(name)?.to_str().ok()?.trim();
    (!text.is_empty() && text.len() <= ETAG_MAX_LENGTH).then(|| text.to_owned())
}

async fn read_body(response: &mut reqwest::Response) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len() + chunk.len() > RESPONSE_LIMIT {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    Some(body)
}

/// The source's answer; `None` if none arrived (network, TLS, timeout, redirect refused).
pub async fn fetch(
    client: &reqwest::Client,
    source: &Source,
    if_none_match: Option<&str>,
) -> Option<Reply> {
    let mut response = request(client, source, if_none_match).send().await.ok()?;
    let headers = response.headers();
    let mut reply = Reply {
        status: response.status().as_u16(),
        etag: header_text(headers, reqwest::header::ETAG.as_str()),
        retry_after: header_text(headers, reqwest::header::RETRY_AFTER.as_str()),
        rate_limit_remaining: header_text(headers, "x-ratelimit-remaining"),
        rate_limit_reset: header_text(headers, "x-ratelimit-reset"),
        body: None,
    };
    if response.status().is_success() {
        reply.body = read_body(&mut response).await;
    }
    Some(reply)
}

/// Queries stale sources (at most [`PARALLEL`] at a time), stores them in the cache
/// and returns the result per profileId.
pub async fn check(
    items: &[Item],
    cache: &mut UpdateCache,
    now: u64,
    force: bool,
) -> HashMap<String, UpdateInfo> {
    let stale = stale_sources(items, cache, now, force);
    if !stale.is_empty() {
        let client = client().ok();
        for batch in stale.chunks(PARALLEL) {
            let jobs: Vec<_> = batch
                .iter()
                .cloned()
                .map(|source| {
                    let client = client.clone();
                    let prior = cache.entries.get(&source.cache_key()).cloned();
                    tauri::async_runtime::spawn(async move {
                        let tag = prior.as_ref().and_then(CacheEntry::if_none_match);
                        let reply = match &client {
                            Some(client) => fetch(client, &source, tag).await,
                            None => None,
                        };
                        let entry = settle(&source, prior.as_ref(), reply.as_ref(), now);
                        (source, entry)
                    })
                })
                .collect();
            for job in jobs {
                if let Ok((source, entry)) = job.await {
                    cache.put(source.cache_key(), entry);
                }
            }
        }
    }
    items
        .iter()
        .map(|item| {
            let entry = cache
                .entries
                .get(&item.source.cache_key())
                .cloned()
                .unwrap_or(CacheEntry {
                    latest: None,
                    checked_at: now,
                    ..CacheEntry::default()
                });
            (item.id.clone(), result(item, &entry))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn latest(toml_src: &str) -> Latest {
        #[derive(Deserialize)]
        struct Wrap {
            latest: Latest,
        }
        toml::from_str::<Wrap>(toml_src).unwrap().latest
    }

    #[test]
    fn fields_are_read_and_validated() {
        let npm = latest(r#"latest = { npm = "@openai/codex" }"#);
        assert_eq!(npm.source(), Ok(Source::Npm("@openai/codex".into())));
        assert_eq!(
            npm.source().unwrap().url(),
            "https://registry.npmjs.org/@openai%2fcodex/latest"
        );
        let pypi = latest(r#"latest = { pypi = "aider-chat" }"#);
        assert_eq!(
            pypi.source().unwrap().url(),
            "https://pypi.org/pypi/aider-chat/json"
        );
        let gh = latest(r#"latest = { github = "PowerShell/PowerShell", tag_prefix = "v" }"#);
        assert_eq!(
            gh.source().unwrap().url(),
            "https://api.github.com/repos/PowerShell/PowerShell/releases/latest"
        );
        let amp = latest(r#"latest = { npm = "@sourcegraph/amp", compare = "equal" }"#);
        assert_eq!(amp.compare, Compare::Equal);
        assert_eq!(npm.compare, Compare::Newer);
        assert!(gh.validate().is_ok());

        for bad in [
            r#"latest = { }"#,
            r#"latest = { npm = "a", pypi = "b" }"#,
            r#"latest = { npm = "../../x" }"#,
            r#"latest = { npm = "Upper" }"#,
            r#"latest = { npm = "@scope" }"#,
            r#"latest = { npm = "a?b=c" }"#,
            r#"latest = { pypi = "a/b" }"#,
            r#"latest = { github = "owner-only" }"#,
            r#"latest = { github = "../x" }"#,
            r#"latest = { github = "a/b/c" }"#,
            r#"latest = { npm = "a", tag_prefix = "v" }"#,
        ] {
            assert!(latest(bad).validate().is_err(), "{bad}");
        }
        #[derive(Debug, Deserialize)]
        #[allow(dead_code)]
        struct Wrap {
            latest: Latest,
        }
        assert!(toml::from_str::<Wrap>(r#"latest = { npm = "a", tag = "x" }"#).is_err());
        assert!(toml::from_str::<Wrap>(r#"latest = { npm = "a", compare = "greater" }"#).is_err());
    }

    #[test]
    fn parse_responses() {
        let npm = Source::Npm("x".into());
        assert_eq!(
            npm.parse(br#"{"name":"@anthropic-ai/claude-code","version":"2.1.319","bin":{}}"#)
                .as_deref(),
            Some("2.1.319")
        );
        assert_eq!(npm.parse(br#"{"name":"x"}"#), None);
        assert_eq!(npm.parse(br#"{"version":"latest"}"#), None);
        assert_eq!(npm.parse(b"<html>"), None);

        let pypi = Source::Pypi("aider-chat".into());
        assert_eq!(
            pypi.parse(
                br#"{"info":{"name":"aider-chat","version":"0.86.2"},"releases":{"0.1.0":[]}}"#
            )
            .as_deref(),
            Some("0.86.2")
        );
        assert_eq!(pypi.parse(br#"{"version":"1.0.0"}"#), None);

        let gh = Source::Github {
            repo: "PowerShell/PowerShell".into(),
            tag_prefix: "v".into(),
        };
        assert_eq!(
            gh.parse(br#"{"tag_name":"v7.5.4","draft":false,"prerelease":false}"#)
                .as_deref(),
            Some("7.5.4")
        );
        assert_eq!(
            gh.parse(br#"{"tag_name":"v7.6.0-rc.1","prerelease":true}"#),
            None
        );
        // Without a prefix in the tag, the tag stays unchanged.
        assert_eq!(
            gh.parse(br#"{"tag_name":"7.5.4"}"#).as_deref(),
            Some("7.5.4")
        );
        let codex = Source::Github {
            repo: "openai/codex".into(),
            tag_prefix: "rust-v".into(),
        };
        assert_eq!(
            codex.parse(br#"{"tag_name":"rust-v0.167.0"}"#).as_deref(),
            Some("0.167.0")
        );
        assert_eq!(gh.parse(br#"{"message":"Not Found"}"#), None);
    }

    #[test]
    fn compare_versions() {
        let newer = |i, l| update_available(i, l, Compare::Newer);
        assert!(newer("0.156.1", "0.167.0"));
        assert!(newer("1.15.7", "1.19.5"));
        assert!(newer("2.1.9", "2.1.10"));
        assert!(newer("2.1", "2.1.1"));
        assert!(newer("0.79.9", "1.5.0"));
        assert!(!newer("2.1.319", "2.1.319"));
        assert!(!newer("2.1.320", "2.1.319"));
        assert!(!newer("7.5.4", "7.5.4"));
        assert!(!newer("unknown", "1.0"));
        assert!(!newer("1.0", ""));
        let equal = |i, l| update_available(i, l, Compare::Equal);
        assert!(equal("0.0.1730000000", "0.0.1740000000-gabc123"));
        assert!(!equal("0.0.1730000000", "0.0.1730000000-gabc123"));
        assert!(!equal("x", "y"));
    }

    #[test]
    fn builtin_source_for_pwsh() {
        let (latest, packages) = builtin_for_command("pwsh").unwrap();
        assert_eq!(
            latest.source().unwrap().cache_key(),
            "github:PowerShell/PowerShell:v"
        );
        assert_eq!(packages.winget.as_deref(), Some("Microsoft.PowerShell"));
        assert!(packages.validate().is_ok());
        assert!(builtin_for_command("PWSH.EXE").is_some());
        assert!(builtin_for_command("/opt/microsoft/powershell/7/pwsh").is_some());
        // Backslash paths only split into folder and file name on Windows.
        #[cfg(windows)]
        assert!(builtin_for_command(r"C:\Program Files\PowerShell\7\PWSH.EXE").is_some());
        assert!(builtin_for_command("powershell").is_none());
        assert!(builtin_for_command("cmd").is_none());
    }

    fn item(id: &str, source: Source, installed: Option<&str>) -> Item {
        Item {
            id: id.into(),
            source,
            compare: Compare::Newer,
            installed: installed.map(str::to_owned),
            plan: PlanView {
                kind: PlanKind::Run,
                id: "0011223344556677".into(),
                reason: None,
                needs_admin: false,
                command: Some("hint".into()),
                installer: Some(Installer::Npm),
                warnings: Vec::new(),
                copies: Vec::new(),
            },
        }
    }

    #[test]
    fn split_update_command() {
        assert_eq!(
            split_command("  npm install -g @google/gemini-cli@latest "),
            Ok(vec![
                "npm".to_owned(),
                "install".into(),
                "-g".into(),
                "@google/gemini-cli@latest".into()
            ])
        );
        assert_eq!(
            split_command("winget\tupgrade GitHub.Copilot"),
            Ok(vec![
                "winget".to_owned(),
                "upgrade".into(),
                "GitHub.Copilot".into()
            ])
        );
        let absolute = if cfg!(windows) {
            r"C:\Tools\uv.exe tool upgrade mistral-vibe"
        } else {
            "/usr/local/bin/uv tool upgrade mistral-vibe"
        };
        assert_eq!(split_command(absolute).map(|a| a.len()), Ok(4));
        // A relative program path would depend on the working folder; slashes in arguments
        // (npm scopes), on the other hand, are no program path.
        for relative in [
            r"tools\npm.cmd install",
            "./npm install",
            "../bin/x up",
            r".\x",
        ] {
            assert_eq!(
                split_command(relative),
                Err(CommandError::RelativeProgram(
                    relative.split(' ').next().unwrap_or_default().to_owned()
                )),
                "{relative:?}"
            );
        }
        assert_eq!(split_command(""), Err(CommandError::Empty));
        assert_eq!(split_command("   \t "), Err(CommandError::Empty));
        for (bad, char) in [
            ("irm https://claude.ai/install.ps1 | iex", '|'),
            ("a && b", '&'),
            ("a; b", ';'),
            ("a > file", '>'),
            ("a < file", '<'),
            ("echo $HOME", '$'),
            ("echo `id`", '`'),
            ("powershell -c (irm x)", '('),
            ("powershell -c \"irm x\"", '"'),
            ("sh -c 'x'", '\''),
            ("echo %PATH%", '%'),
            ("echo ^x", '^'),
            ("echo !x!", '!'),
            ("rm *", '*'),
            ("a # comment", '#'),
            ("ls ~", '~'),
            ("a\nb", '\n'),
            ("a\rb", '\r'),
            ("a\u{1b}b", '\u{1b}'),
        ] {
            assert_eq!(
                split_command(bad),
                Err(CommandError::ShellSyntax(char)),
                "{bad:?}"
            );
        }
    }

    fn agent_with(update_hint: Option<PlatformHint>, install: &str) -> Agent {
        let mut agent = Agent::from_catalog(crate::agent_catalog::CatalogAgent {
            id: "x".into(),
            name: "X".into(),
            commands: vec!["x-cli".into(), "x".into()],
            args: vec!["--fancy".into()],
            icon: None,
            homepage: None,
            install_windows: Some(install.into()),
            install_unix: Some(install.into()),
            version_args: None,
            resume_args: Some(vec!["--resume".into()]),
            session_id_args: Some(vec!["--id".into(), "{id}".into()]),
            resume_id_args: None,
            sub_agents: Some(crate::subagents::SubAgentKind::Claude),
            latest: None,
            update_hint,
            packages: None,
            self_update: None,
            capabilities: Default::default(),
            instruction_args: None,
            prompt_args: None,
        });
        agent
            .profile
            .env
            .insert("NPM_CONFIG_PREFIX".into(), "C:/npm".into());
        agent
    }

    fn both(command: &str) -> PlatformHint {
        PlatformHint {
            windows: Some(command.into()),
            unix: Some(command.into()),
        }
    }

    fn plan_of(agent: &Agent) -> UpdatePlan {
        // A program that does not exist: only the update_hint can make a run plan.
        install_source::plan(
            &install_source::PlanInput {
                profile: &agent.profile,
                install_hint: agent.install_hint.as_deref(),
                program: None,
                candidates: &agent.candidates,
            },
            &install_source::SystemProbe,
        )
    }

    #[test]
    fn update_agent_runs_only_the_plan() {
        let agent = agent_with(Some(both("npm install -g x@latest")), "npm install -g x");
        let updater = update_agent(&agent, &plan_of(&agent)).unwrap();
        assert_eq!(updater.profile.command, "npm");
        assert_eq!(updater.profile.args, ["install", "-g", "x@latest"]);
        assert_eq!(updater.candidates, ["npm"]);
        assert_eq!(updater.profile.resume_args, None);
        assert_eq!(updater.profile.session_id_args, None);
        assert_eq!(updater.profile.sub_agents, None);
        assert_eq!(updater.id(), "x");
        // The profile's environment applies to the update as well.
        assert_eq!(updater.profile.env, agent.profile.env);

        // Without update_hint and without an install the install command is never run, not
        // even a harmless one.
        let without = agent_with(None, "npm install -g x");
        let plan = plan_of(&without);
        assert_eq!(plan.kind(), PlanKind::Copy);
        assert_eq!(plan.command().as_deref(), Some("npm install -g x"));
        assert_eq!(
            update_agent(&without, &plan).err(),
            Some(CommandError::Missing)
        );
        // Only the other platform has a command: none here.
        let other = if cfg!(windows) {
            PlatformHint {
                windows: None,
                unix: Some("x upgrade".into()),
            }
        } else {
            PlatformHint {
                windows: Some("x upgrade".into()),
                unix: None,
            }
        };
        let other = agent_with(Some(other), "y");
        assert_eq!(
            update_agent(&other, &plan_of(&other)).err(),
            Some(CommandError::Missing)
        );
        let pipeline = agent_with(Some(both("irm a | iex")), "y");
        assert_eq!(
            update_agent(&pipeline, &plan_of(&pipeline)).err(),
            Some(CommandError::ShellSyntax('|'))
        );
        // Any other reason reaches the frontend as such.
        let mut ambiguous = plan_of(&without);
        ambiguous.reason = Some(Reason::Ambiguous);
        assert_eq!(
            update_agent(&without, &ambiguous).err(),
            Some(CommandError::NoPlan(Reason::Ambiguous))
        );
    }

    #[test]
    fn info_follows_the_plan() {
        let latest = Some(Latest {
            npm: Some("x".into()),
            pypi: None,
            github: None,
            tag_prefix: None,
            compare: Compare::Newer,
        });
        let mut runnable = agent_with(Some(both("x upgrade")), "irm x | iex");
        runnable.profile.latest.clone_from(&latest);
        let view = PlanView::of(&plan_of(&runnable), true);
        let item = Item::of(&runnable, Some("1.0".into()), view).unwrap();
        assert_eq!(item.plan.kind, PlanKind::Run);
        assert_eq!(item.plan.command.as_deref(), Some("x upgrade"));
        assert_eq!(item.plan.warnings, [Warning::Running]);

        let mut copy_only = agent_with(None, "irm x | iex");
        copy_only.profile.latest = latest;
        let view = PlanView::of(&plan_of(&copy_only), false);
        let item = Item::of(&copy_only, Some("1.0".into()), view).unwrap();
        let info = result(
            &item,
            &CacheEntry {
                latest: Some("2.0".into()),
                checked_at: 1,
                ..CacheEntry::default()
            },
        );
        let json = serde_json::to_value(info).unwrap();
        assert_eq!(json["canRun"], false);
        assert_eq!(json["plan"], "copy");
        // The install command is still shown, for copying.
        assert_eq!(json["command"], "irm x | iex");
        assert_eq!(json["updateHint"], "irm x | iex");
        assert_eq!(json["reason"], "not-installed");
        assert_eq!(json["needsAdmin"], false);
        assert!(json["installer"].is_null());
        assert_eq!(json["warnings"], serde_json::json!([]));
    }

    #[test]
    fn outcome_reasons() {
        let r = outcome_reason;
        assert_eq!(r(Some("1.0"), Some("1.1"), Some(0), true, true), None);
        assert_eq!(
            r(Some("1.0"), Some("1.0"), Some(0), true, true),
            Some(OutcomeReason::Running)
        );
        assert_eq!(
            r(Some("1.0"), Some("1.0"), Some(1), false, true),
            Some(OutcomeReason::Failed)
        );
        assert_eq!(
            r(Some("1.0"), Some("1.0"), Some(0), false, true),
            Some(OutcomeReason::Shadowed)
        );
        assert_eq!(
            r(Some("1.0"), Some("1.0"), Some(0), false, false),
            Some(OutcomeReason::Unchanged)
        );
        assert_eq!(r(None, Some("1.0"), Some(0), false, false), None);
        // A successful update without a version to compare claims no cause.
        assert_eq!(r(None, Some("1.0"), Some(0), true, true), None);
        assert_eq!(
            r(None, Some("1.0"), Some(1), false, true),
            Some(OutcomeReason::Failed)
        );
        assert_eq!(
            r(None, None, None, false, false),
            Some(OutcomeReason::Failed)
        );
        let json = serde_json::to_value(UpdateOutcome {
            profile_id: "x".into(),
            session_id: Some(3),
            code: Some(0),
            before: Some("1.0".into()),
            after: Some("1.0".into()),
            changed: Some(false),
            reason: Some(OutcomeReason::Shadowed),
            copies: vec![PathBuf::from("a")],
        })
        .unwrap();
        assert_eq!(json["reason"], "shadowed");
        assert_eq!(json["profileId"], "x");
        assert_eq!(json["sessionId"], 3);
    }

    #[test]
    fn git_for_windows_tags_compare_with_build_number() {
        let gh = Source::Github {
            repo: "git-for-windows/git".into(),
            tag_prefix: "v".into(),
        };
        assert_eq!(
            gh.parse(br#"{"tag_name":"v2.56.0.windows.1"}"#).as_deref(),
            Some("2.56.0.1")
        );
        let newer = |i: &str, l: &str| update_available(i, l, Compare::Newer);
        // `git --version` prints `git version 2.56.0.windows.4`; winget lists 2.56.0.4.
        let installed = parse_version("git version 2.56.0.windows.4").unwrap();
        assert_eq!(installed, "2.56.0.4");
        assert!(newer(&installed, "2.56.0.5"));
        assert!(!newer(&installed, "2.56.0.4"));
        assert!(newer(&installed, "2.57.0.1"));
        assert!(!newer("2.56.0.4", "2.56.0.windows.4"));
    }

    #[test]
    fn cache_validity_and_force() {
        let dir = std::env::temp_dir().join(format!("hangar-updates-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let now = 1_000_000_000_000;
        let a = Source::Npm("a".into());
        let b = Source::Pypi("b".into());
        let c = Source::Npm("c".into());
        let mut cache = UpdateCache::load(&dir);
        cache.put(
            a.cache_key(),
            CacheEntry {
                latest: Some("2.0.0".into()),
                checked_at: now - CACHE_TTL_MS + 1,
                etag: Some("\"a1\"".into()),
                retry_at: None,
            },
        );
        cache.put(
            b.cache_key(),
            CacheEntry {
                latest: None,
                checked_at: now - FAILURE_TTL_MS,
                ..CacheEntry::default()
            },
        );
        cache.save(&dir).unwrap();
        let cache = UpdateCache::load(&dir);
        assert!(cache.fresh(&a.cache_key(), now).is_some());
        // The entity tag survives the disk round trip.
        assert_eq!(
            cache.entries[&a.cache_key()].if_none_match(),
            Some("\"a1\"")
        );
        // Failures count for 15 min only, hits for 25 min.
        assert!(cache.fresh(&b.cache_key(), now).is_none());
        assert!(cache.fresh(&a.cache_key(), now + 1).is_none());
        // An entry from the future (clock set wrong) is not valid.
        assert!(cache.fresh(&a.cache_key(), now - CACHE_TTL_MS).is_none());

        let items = vec![
            item("x", a.clone(), Some("1.0.0")),
            item("y", b.clone(), Some("1.0")),
            item("z", c.clone(), None),
            item("x2", a.clone(), Some("3.0.0")),
        ];
        assert_eq!(
            stale_sources(&items, &cache, now, false),
            vec![b.clone(), c.clone()]
        );
        assert_eq!(
            stale_sources(&items, &cache, now, true),
            vec![a.clone(), b, c]
        );

        let info = result(&items[0], cache.fresh(&a.cache_key(), now).unwrap());
        assert!(info.update_available);
        assert_eq!(info.latest.as_deref(), Some("2.0.0"));
        assert_eq!(info.update_hint.as_deref(), Some("hint"));
        assert!(!result(&items[3], cache.fresh(&a.cache_key(), now).unwrap()).update_available);
        let json = serde_json::to_value(&info).unwrap();
        for key in [
            "installed",
            "latest",
            "updateAvailable",
            "updateHint",
            "canRun",
            "checkedAt",
            "plan",
            "reason",
            "needsAdmin",
            "command",
            "installer",
            "warnings",
            "copies",
        ] {
            assert!(json.get(key).is_some(), "{key}");
        }

        std::fs::write(UpdateCache::path(&dir), "broken").unwrap();
        assert_eq!(UpdateCache::load(&dir).entries.len(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn check_without_network_uses_only_cache() {
        let now = 5_000_000;
        let a = Source::Npm("a".into());
        let mut cache = UpdateCache::default();
        cache.put(
            a.cache_key(),
            CacheEntry {
                latest: Some("1.1".into()),
                checked_at: now,
                ..CacheEntry::default()
            },
        );
        let items = vec![item("x", a, Some("1.0"))];
        let result = tauri::async_runtime::block_on(check(&items, &mut cache, now, false));
        assert!(result["x"].update_available);
        assert_eq!(result["x"].checked_at, now);
    }

    #[test]
    fn ttls_stay_below_the_check_interval() {
        // The frontend checks every 30 min (UPDATE_CHECK_INTERVAL_MS in programUpdates.ts).
        let interval = 30 * 60 * 1000;
        assert_eq!(CACHE_TTL_MS, 25 * 60 * 1000);
        assert_eq!(FAILURE_TTL_MS, 15 * 60 * 1000);
        assert!(CACHE_TTL_MS < interval && FAILURE_TTL_MS < CACHE_TTL_MS);
    }

    fn github() -> Source {
        Source::Github {
            repo: "PowerShell/PowerShell".into(),
            tag_prefix: "v".into(),
        }
    }

    fn known(latest: &str, etag: Option<&str>, checked_at: u64) -> CacheEntry {
        CacheEntry {
            latest: Some(latest.into()),
            checked_at,
            etag: etag.map(str::to_owned),
            retry_at: None,
        }
    }

    #[test]
    fn success_stores_the_entity_tag_and_it_is_sent_back() {
        let now = 9_000_000;
        let ok = Reply {
            status: 200,
            etag: Some("W/\"abc\"".into()),
            body: Some(br#"{"tag_name":"v7.5.4"}"#.to_vec()),
            ..Reply::default()
        };
        let entry = settle(&github(), None, Some(&ok), now);
        assert_eq!(entry, known("7.5.4", Some("W/\"abc\""), now));
        assert_eq!(entry.if_none_match(), Some("W/\"abc\""));

        // A body without a version stores no tag: a 304 on it would confirm nothing.
        let broken = Reply {
            body: Some(br#"{"message":"x"}"#.to_vec()),
            ..ok.clone()
        };
        let entry = settle(&github(), None, Some(&broken), now);
        assert_eq!((entry.latest, entry.etag), (None, None));
        // No version known, so nothing to make conditional.
        let tag_only = CacheEntry {
            etag: Some("\"x\"".into()),
            ..CacheEntry::default()
        };
        assert_eq!(tag_only.if_none_match(), None);

        let client = client().unwrap();
        let conditional = request(&client, &github(), Some("W/\"abc\""))
            .build()
            .unwrap();
        assert_eq!(
            conditional.headers()[reqwest::header::IF_NONE_MATCH],
            "W/\"abc\""
        );
        assert_eq!(
            conditional.headers()[reqwest::header::ACCEPT],
            "application/vnd.github+json"
        );
        let plain = request(&client, &github(), None).build().unwrap();
        assert!(plain
            .headers()
            .get(reqwest::header::IF_NONE_MATCH)
            .is_none());
        // A tag that is no valid header value is left out.
        let invalid = request(&client, &github(), Some("a\nb")).build().unwrap();
        assert!(invalid
            .headers()
            .get(reqwest::header::IF_NONE_MATCH)
            .is_none());
    }

    #[test]
    fn not_modified_keeps_the_version_and_refreshes_the_time() {
        let prior = known("7.5.4", Some("\"abc\""), 1_000);
        let now = 2_000_000;
        let not_modified = Reply {
            status: 304,
            ..Reply::default()
        };
        let entry = settle(&github(), Some(&prior), Some(&not_modified), now);
        assert_eq!(entry, known("7.5.4", Some("\"abc\""), now));
        // A new tag on the 304 replaces the old one.
        let retagged = Reply {
            etag: Some("\"def\"".into()),
            ..not_modified.clone()
        };
        let entry = settle(&github(), Some(&prior), Some(&retagged), now);
        assert_eq!(entry.etag.as_deref(), Some("\"def\""));
        // Without a known version a 304 is a failure.
        let entry = settle(&github(), None, Some(&not_modified), now);
        assert_eq!(entry.latest, None);
        assert_eq!(entry.checked_at, now);
    }

    #[test]
    fn rate_limits_pause_the_source() {
        let now = 1_700_000_000_000;
        let limited =
            |status, retry: Option<&str>, remaining: Option<&str>, reset: Option<&str>| Reply {
                status,
                retry_after: retry.map(str::to_owned),
                rate_limit_remaining: remaining.map(str::to_owned),
                rate_limit_reset: reset.map(str::to_owned),
                ..Reply::default()
            };
        // Retry-After in seconds comes first.
        assert_eq!(
            rate_limited_until(&limited(429, Some("120"), Some("0"), Some("1")), now),
            Some(now + 120_000)
        );
        // Primary limit: until X-RateLimit-Reset (epoch seconds).
        let reset = (now / 1000 + 1800).to_string();
        assert_eq!(
            rate_limited_until(&limited(403, None, Some("0"), Some(&reset)), now),
            Some(now + 1_800_000)
        );
        // Without a time: at least one minute.
        assert_eq!(
            rate_limited_until(&limited(429, None, None, None), now),
            Some(now + RATE_LIMIT_MIN_BACKOFF_MS)
        );
        // A reset in the past or absurdly far away is bounded.
        assert_eq!(
            rate_limited_until(&limited(403, None, Some("0"), Some("5")), now),
            Some(now + RATE_LIMIT_MIN_BACKOFF_MS)
        );
        assert_eq!(
            rate_limited_until(&limited(403, Some("999999999"), None, None), now),
            Some(now + RATE_LIMIT_MAX_BACKOFF_MS)
        );
        // A plain 403 and other statuses are no rate limit.
        assert_eq!(
            rate_limited_until(&limited(403, None, Some("59"), None), now),
            None
        );
        assert_eq!(
            rate_limited_until(&limited(500, Some("60"), None, None), now),
            None
        );

        // The known version stays; the source waits, even for "Check now".
        let prior = known("7.5.4", Some("\"abc\""), now - CACHE_TTL_MS);
        let entry = settle(
            &github(),
            Some(&prior),
            Some(&limited(429, Some("600"), None, None)),
            now,
        );
        assert_eq!(entry.latest.as_deref(), Some("7.5.4"));
        assert_eq!(entry.checked_at, prior.checked_at);
        assert_eq!(entry.retry_at, Some(now + 600_000));
        let mut cache = UpdateCache::default();
        cache.put(github().cache_key(), entry);
        let items = vec![item("pwsh", github(), Some("7.5.0"))];
        assert!(stale_sources(&items, &cache, now + 1, true).is_empty());
        assert!(stale_sources(&items, &cache, now + 599_999, false).is_empty());
        assert_eq!(
            stale_sources(&items, &cache, now + 600_000, false),
            vec![github()]
        );
        // The offer stays visible during the pause.
        let result = tauri::async_runtime::block_on(check(&items, &mut cache, now + 1, true));
        assert!(result["pwsh"].update_available);
        // A pause set before the clock was turned back does not block for longer.
        let far = CacheEntry {
            retry_at: Some(now + RATE_LIMIT_MAX_BACKOFF_MS + 1),
            ..CacheEntry::default()
        };
        assert!(!far.backing_off(now));

        // A rate limit without anything known yet is remembered as a failure with a pause.
        let entry = settle(&github(), None, Some(&limited(429, None, None, None)), now);
        assert_eq!(entry.latest, None);
        assert_eq!(entry.retry_at, Some(now + RATE_LIMIT_MIN_BACKOFF_MS));
        // No answer at all is a plain failure.
        assert_eq!(
            settle(&github(), Some(&prior), None, now),
            CacheEntry {
                latest: None,
                checked_at: now,
                ..CacheEntry::default()
            }
        );
    }

    #[test]
    fn older_cache_entries_without_the_new_fields_load() {
        let entry: CacheEntry = serde_json::from_str(r#"{"latest":"1.0","checkedAt":5}"#).unwrap();
        assert_eq!(entry, known("1.0", None, 5));
        let json = serde_json::to_value(&entry).unwrap();
        assert!(json.get("etag").is_none() && json.get("retryAt").is_none());
    }

    /// Real query, only on request: `cargo test -- --ignored live`.
    #[test]
    #[ignore]
    fn live_query_of_the_three_sources() {
        let sources = [
            Source::Npm("@anthropic-ai/claude-code".into()),
            Source::Pypi("aider-chat".into()),
            Source::Github {
                repo: "PowerShell/PowerShell".into(),
                tag_prefix: "v".into(),
            },
        ];
        let client = client().unwrap();
        for source in sources {
            let reply = tauri::async_runtime::block_on(fetch(&client, &source, None));
            let entry = settle(&source, None, reply.as_ref(), now_millis());
            println!("{source:?}: {:?} etag {:?}", entry.latest, entry.etag);
            assert!(entry.latest.is_some(), "{source:?}");
            // Asked again with the tag: a 304 must keep the version.
            let tag = entry.if_none_match();
            let again = tauri::async_runtime::block_on(fetch(&client, &source, tag));
            println!(
                "  conditional: status {:?}",
                again.as_ref().map(|r| r.status)
            );
            let next = settle(&source, Some(&entry), again.as_ref(), now_millis());
            assert_eq!(next.latest, entry.latest, "{source:?}");
        }
    }
}
