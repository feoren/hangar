//! Newer versions of the installed programs (sources: docs/wiki/pages/agents/updates.md).
//!
//! Per catalog entry or profile, `latest` says where the newest version is published: npm registry,
//! PyPI or a GitHub release. Only the `/latest` endpoints are queried (never lists of all
//! versions, never prereleases), only over HTTPS and only on the three fixed hosts. Besides the
//! address, only the user agent `Hangar/<version>` is sent, nothing that identifies the user.
//! Results stay in the disk cache for 24 h; errors silently count as "unknown", for one hour.

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
pub const CACHE_TTL_MS: u64 = 24 * 60 * 60 * 1000;
pub const FAILURE_TTL_MS: u64 = 60 * 60 * 1000;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheEntry {
    pub latest: Option<String>,
    /// Unix time in milliseconds.
    pub checked_at: u64,
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

    /// Entry still valid: hits for 24 h, failures for 1 h.
    pub fn fresh(&self, key: &str, now: u64) -> Option<&CacheEntry> {
        let entry = self.entries.get(key)?;
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

/// Sources that need a fresh query (without duplicates), in a stable order.
pub fn stale_sources(items: &[Item], cache: &UpdateCache, now: u64, force: bool) -> Vec<Source> {
    let mut seen = std::collections::HashSet::new();
    items
        .iter()
        .map(|item| &item.source)
        .filter(|source| force || cache.fresh(&source.cache_key(), now).is_none())
        .filter(|source| seen.insert(source.cache_key()))
        .cloned()
        .collect()
}

/// Rustls needs a process-wide crypto provider; the same one as tauri-plugin-updater.
fn ensure_crypto_provider() {
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

/// Newest version according to the source; any error yields `None`.
pub async fn fetch(client: &reqwest::Client, source: &Source) -> Option<String> {
    let mut response = client
        .get(source.url())
        .header(reqwest::header::ACCEPT, source.accept())
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len() + chunk.len() > RESPONSE_LIMIT {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    source.parse(&body)
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
                    tauri::async_runtime::spawn(async move {
                        let latest = match &client {
                            Some(client) => fetch(client, &source).await,
                            None => None,
                        };
                        (source, latest)
                    })
                })
                .collect();
            for job in jobs {
                if let Ok((source, latest)) = job.await {
                    cache.put(
                        source.cache_key(),
                        CacheEntry {
                            latest,
                            checked_at: now,
                        },
                    );
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
            },
        );
        cache.put(
            b.cache_key(),
            CacheEntry {
                latest: None,
                checked_at: now - FAILURE_TTL_MS,
            },
        );
        cache.save(&dir).unwrap();
        let cache = UpdateCache::load(&dir);
        assert!(cache.fresh(&a.cache_key(), now).is_some());
        // Failures count for one hour only, hits for 24 h.
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
            },
        );
        let items = vec![item("x", a, Some("1.0"))];
        let result = tauri::async_runtime::block_on(check(&items, &mut cache, now, false));
        assert!(result["x"].update_available);
        assert_eq!(result["x"].checked_at, now);
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
            let latest = tauri::async_runtime::block_on(fetch(&client, &source));
            println!("{source:?}: {latest:?}");
            assert!(latest.is_some(), "{source:?}");
        }
    }
}
