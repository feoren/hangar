//! Built-in catalog of known agent CLIs, merging with the user profiles and
//! detection of which of them are installed.
//!
//! The catalog is data, not code: a new agent is added as an `[[agent]]` in
//! `agent_catalog.toml`. User profiles take precedence; a profile with the same `id` replaces the
//! catalog entry completely.

use std::collections::{BTreeMap, HashSet};
use std::ffi::OsStr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::install_source::{is_foreign_launcher, Packages, SelfUpdate};
use crate::profile::Profile;
use crate::resolve::resolve_all;
use crate::resolve::resolve_program;
use crate::updates::{builtin_for_command, Latest, PlatformHint};

pub const BUILTIN: &str = include_str!("agent_catalog.toml");

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogAgent {
    pub id: String,
    pub name: String,
    /// Program names without a path, searched in this order.
    pub commands: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub install_windows: Option<String>,
    #[serde(default)]
    pub install_unix: Option<String>,
    /// Arguments for the version query; for the default see [`crate::version::version_args`].
    #[serde(default)]
    pub version_args: Option<Vec<String>>,
    /// Arguments for resuming the last conversation; they replace `args`
    /// (see [`Profile::resume_args`]).
    #[serde(default)]
    pub resume_args: Option<Vec<String>>,
    /// Fresh start with a conversation id chosen by Hangar (see [`Profile::session_id_args`]).
    #[serde(default)]
    pub session_id_args: Option<Vec<String>>,
    /// Resume of the conversation with the stored id (see [`Profile::resume_id_args`]).
    #[serde(default)]
    pub resume_id_args: Option<Vec<String>>,
    /// Source of sub-agent transcripts (see [`Profile::sub_agents`]).
    #[serde(default)]
    pub sub_agents: Option<crate::subagents::SubAgentKind>,
    /// Source of the newest version (see [`Latest`]).
    #[serde(default)]
    pub latest: Option<Latest>,
    /// Update command per platform; an override of the detected update (see [`crate::install_source::plan`]).
    #[serde(default)]
    pub update_hint: Option<PlatformHint>,
    /// Package name per installer (see [`Packages`]).
    #[serde(default)]
    pub packages: Option<Packages>,
    /// The program's own update command (see [`SelfUpdate`]).
    #[serde(default)]
    pub self_update: Option<SelfUpdate>,
    /// What the agent gate can do with the program (see [`Capabilities`]).
    #[serde(default)]
    pub capabilities: Capabilities,
}

/// What Hangar's agent gate (team service step B, `gate.rs`) can do with a program. All false
/// unless the catalog says otherwise; evidence per agent in
/// docs/wiki/pages/syntheses/team-service-v1.md, section 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    /// A hook can stop an edit before it happens and tell the agent why.
    #[serde(default)]
    pub pre_edit_block: bool,
    /// A hook can hand the agent facts as context.
    #[serde(default)]
    pub context_inject: bool,
    /// Hooks can be given to one session without touching the user's settings files.
    #[serde(default)]
    pub per_session_config: bool,
    /// How Hangar hands its hooks to one session of the program; `None` until a format for the
    /// program is built.
    #[serde(default)]
    pub hook_format: Option<HookFormat>,
    /// The recipe was run once against the live program and an edit was really held. Hangar
    /// offers only what works: a built but unmeasured recipe stays off.
    #[serde(default)]
    pub measured: bool,
    /// Oldest program version the recipe is known to work with (the measured or researched one);
    /// below it, or with the version unknown, the program is not offered for team sessions.
    #[serde(default)]
    pub min_version: Option<MinVersion>,
}

impl Capabilities {
    /// The gate can be installed for one session and hold its edits, so the program can run as a
    /// team session. Purely from the catalog's data: no program is named in code. What the host
    /// adds (version, administrator settings) is [`Capabilities::team_ready`].
    pub fn gate(self) -> bool {
        self.gate_format().is_some()
    }

    /// The hook format the gate uses for the program, when the gate applies at all.
    pub fn gate_format(self) -> Option<HookFormat> {
        if self.pre_edit_block && self.per_session_config && self.measured {
            self.hook_format
        } else {
            None
        }
    }

    /// [`Capabilities::gate`] on this machine: the installed `version` reaches
    /// [`Capabilities::min_version`] (an unknown version does not), and no administrator's system
    /// settings would be replaced by the session's file (`admin_settings`,
    /// [`crate::hook_recipes::admin_settings_present`]).
    pub fn team_ready(self, version: Option<&str>, admin_settings: bool) -> bool {
        self.team_block(version, admin_settings).is_none()
    }

    /// Why the program cannot be a team session on this machine, `None` when it can
    /// ([`Capabilities::team_ready`]). Checked in the order the catalog and then the host decide:
    /// no recipe, recipe not measured, administrator settings in the way, version unknown or
    /// below the floor.
    pub fn team_block(self, version: Option<&str>, admin_settings: bool) -> Option<TeamBlock> {
        if !(self.pre_edit_block && self.per_session_config) || self.hook_format.is_none() {
            return Some(TeamBlock::NoHooks);
        }
        if !self.measured {
            return Some(TeamBlock::NotMeasured);
        }
        if admin_settings {
            return Some(TeamBlock::AdminSettings);
        }
        let min = self.min_version?;
        let Some(installed) = version.and_then(MinVersion::parse) else {
            return Some(TeamBlock::UnknownVersion);
        };
        (installed < min).then(|| TeamBlock::BelowMinVersion {
            min: min.to_string(),
            installed: installed.to_string(),
        })
    }
}

/// Why a program cannot be a team session (`ProfileView::team_block`). `NoHooks` and
/// `NotMeasured` come from the catalog and cannot be fixed on this machine; the others can:
/// a version Hangar could not read, an older version, an administrator's settings file the
/// session's would replace, or a working folder fixed in the profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TeamBlock {
    NoHooks,
    NotMeasured,
    UnknownVersion,
    #[serde(rename_all = "camelCase")]
    BelowMinVersion {
        min: String,
        installed: String,
    },
    AdminSettings,
    FixedFolder,
}

/// A version as dotted numbers (up to four parts, missing ones count as 0), for the comparison
/// with a minimum. A pre-release or build suffix (`1.0.0-preview.1`, `2.1.3+abc`) is ignored, so a
/// preview counts as its release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub struct MinVersion([u64; 4]);

impl MinVersion {
    pub fn parse(text: &str) -> Option<Self> {
        let core = text
            .trim()
            .trim_start_matches(['v', 'V'])
            .split(['-', '+', ' '])
            .next()?;
        let mut parts = [0u64; 4];
        let mut count = 0;
        for part in core.split('.') {
            if count == parts.len() || part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            parts[count] = part.parse().ok()?;
            count += 1;
        }
        (count > 0).then_some(Self(parts))
    }
}

impl std::fmt::Display for MinVersion {
    /// Three parts always, the fourth only when set: `1.19.5`, `1.2.3.4`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [major, minor, patch, build] = self.0;
        write!(f, "{major}.{minor}.{patch}")?;
        if build != 0 {
            write!(f, ".{build}")?;
        }
        Ok(())
    }
}

impl TryFrom<String> for MinVersion {
    type Error = String;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text).ok_or_else(|| format!("not a version: {text}"))
    }
}

/// A way to give Hangar's hooks to exactly one session of a program. Each agent CLI has its own
/// hook configuration; a new one is another variant here plus its recipe in `hook_recipes.rs`,
/// installed by `gate_service.rs`. Evidence per format:
/// docs/wiki/pages/syntheses/agent-gate-adapters.md.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookFormat {
    /// Claude Code's `--settings <json or file>` with HTTP hooks (`gate::claude_settings`).
    ClaudeSettings,
    /// Qwen Code: a system settings file named by `QWEN_CODE_SYSTEM_SETTINGS_PATH`, HTTP hooks in
    /// Claude Code's shape.
    QwenSystemSettings,
    /// Gemini CLI: a system settings file named by `GEMINI_CLI_SYSTEM_SETTINGS_PATH` with a
    /// `BeforeTool` command hook running `hangar gate-hook`.
    GeminiSystemSettings,
    /// Copilot CLI: `--plugin-dir <folder>` with a plugin whose `preToolUse` hook runs
    /// `hangar gate-hook` as a program.
    CopilotPlugin,
    /// Codex CLI: `--dangerously-bypass-hook-trust -c hooks.PreToolUse=[…]` running
    /// `hangar gate-hook` through cmd.exe or the shell.
    CodexConfigFlags,
    /// Pi: `-e <file>` with an in-process extension whose `tool_call` handler asks the endpoint.
    PiExtension,
    /// Oh My Pi: the same extension with `-e <file>`.
    OmpExtension,
    /// OpenCode: `OPENCODE_CONFIG_CONTENT` naming an in-process plugin file with a
    /// `tool.execute.before` hook.
    OpencodePlugin,
    /// Droid: `--settings <file>` with a `PreToolUse` command hook running `hangar gate-hook`.
    DroidSettings,
}

impl HookFormat {
    /// The start arguments or the profile's environment already carry the program's own
    /// configuration where Hangar would put its hooks. Hangar's would replace it or be a second
    /// one, and how the program combines both is not measured, so such a start goes without the
    /// gate instead of risking the user's settings or a silently missing hook. Copilot plugin
    /// folders and Pi extensions add up; of Codex's config overrides only one of `hooks` or
    /// `hooks.PreToolUse` conflicts, as it sets the key Hangar's layer sets. `env` is the
    /// profile's plain environment, `secret_env` the names of its keychain variables; both reach
    /// the program.
    pub fn conflicts_with(
        self,
        args: &[String],
        env: &BTreeMap<String, String>,
        secret_env: &[String],
    ) -> bool {
        // Windows compares variable names without case.
        let has_env = |name: &str| {
            env.keys()
                .chain(secret_env)
                .any(|key| key == name || (cfg!(windows) && key.eq_ignore_ascii_case(name)))
        };
        match self {
            HookFormat::ClaudeSettings => args
                .iter()
                .any(|arg| arg == "--settings" || arg.starts_with("--settings=")),
            HookFormat::QwenSystemSettings => has_env(crate::hook_recipes::QWEN_SETTINGS_ENV),
            HookFormat::GeminiSystemSettings => has_env(crate::hook_recipes::GEMINI_SETTINGS_ENV),
            // A later `-c hooks=...` or `-c hooks.PreToolUse=...` sets the key of Hangar's layer
            // and would replace it while the session shows the gate as active.
            HookFormat::CodexConfigFlags => codex_overrides(args).any(|value| {
                config_key(value).is_some_and(|key| {
                    key == "hooks"
                        || key == "hooks.PreToolUse"
                        || key.starts_with("hooks.PreToolUse.")
                })
            }),
            // Extensions add up: Pi loads every `-e`, also with `--no-extensions`.
            HookFormat::CopilotPlugin | HookFormat::PiExtension => false,
            // Oh My Pi refuses `--trusted-extension` together with `-e`.
            HookFormat::OmpExtension => args.iter().any(|arg| {
                arg == crate::hook_recipes::OMP_TRUSTED_FLAG
                    || arg.starts_with("--trusted-extension=")
            }),
            // Hangar's plugin travels in OPENCODE_CONFIG_CONTENT, which would replace the
            // profile's own.
            HookFormat::OpencodePlugin => has_env(crate::hook_recipes::OPENCODE_CONFIG_CONTENT_ENV),
            HookFormat::DroidSettings => args
                .iter()
                .any(|arg| arg == "--settings" || arg.starts_with("--settings=")),
        }
    }
}

/// The values of Codex's `-c <key=value>`, `--config <key=value>` and `--config=<key=value>`.
fn codex_overrides(args: &[String]) -> impl Iterator<Item = &str> {
    args.iter().enumerate().filter_map(|(index, arg)| {
        if arg == "-c" || arg == "--config" {
            args.get(index + 1).map(String::as_str)
        } else {
            arg.strip_prefix("--config=")
        }
    })
}

/// The dotted key of a `key=value` override, without surrounding blanks.
fn config_key(value: &str) -> Option<&str> {
    value.split_once('=').map(|(key, _)| key.trim())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogFile {
    #[serde(default, rename = "agent")]
    agents: Vec<CatalogAgent>,
}

#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("Invalid agent catalog: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("Catalog ID '{0}' appears more than once")]
    DuplicateId(String),
    #[error("Catalog entry '{agent}': {problem}")]
    Invalid {
        agent: String,
        problem: &'static str,
    },
}

pub fn parse(source: &str) -> Result<Vec<CatalogAgent>, CatalogError> {
    let file: CatalogFile = toml::from_str(source)?;
    let mut seen = HashSet::new();
    for agent in &file.agents {
        let invalid = |problem| CatalogError::Invalid {
            agent: if agent.id.is_empty() {
                agent.name.clone()
            } else {
                agent.id.clone()
            },
            problem,
        };
        if agent.id.trim().is_empty() || agent.id.chars().any(char::is_whitespace) {
            return Err(invalid("id must not be empty or contain whitespace"));
        }
        if agent.name.trim().is_empty() {
            return Err(invalid("name must not be empty"));
        }
        if agent.commands.is_empty() {
            return Err(invalid("commands needs at least one command"));
        }
        // Plain PATH names: a path in the catalog would never be right on every platform.
        if agent
            .commands
            .iter()
            .any(|c| c.trim().is_empty() || c.contains('/') || c.contains('\\'))
        {
            return Err(invalid(
                "commands may only contain non-empty program names without a path",
            ));
        }
        if !seen.insert(agent.id.as_str()) {
            return Err(CatalogError::DuplicateId(agent.id.clone()));
        }
        let update_fields = [
            agent.latest.as_ref().map(Latest::validate),
            agent.packages.as_ref().map(Packages::validate),
            agent.self_update.as_ref().map(SelfUpdate::validate),
        ];
        if let Some(Err(problem)) = update_fields.into_iter().flatten().find(Result::is_err) {
            return Err(invalid(problem));
        }
        if let Err(problem) = crate::profile::check_id_args(
            agent.session_id_args.as_deref(),
            agent.resume_id_args.as_deref(),
        ) {
            return Err(invalid(problem));
        }
    }
    Ok(file.agents)
}

/// The embedded catalog. A test makes sure it is valid.
pub fn builtin() -> Result<Vec<CatalogAgent>, CatalogError> {
    parse(BUILTIN)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    User,
    Catalog,
}

/// A startable entry of the "New session" list, from a user profile or the catalog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    /// For catalog entries `command` is the first candidate; the first one found is started.
    pub profile: Profile,
    pub source: Source,
    /// Program names in search order; for user profiles only `profile.command`.
    pub candidates: Vec<String>,
    pub homepage: Option<String>,
    pub install_hint: Option<String>,
    /// Option that hands the working folder to the program as an argument, followed by the
    /// folder (`wsl.exe --cd <folder>`); `None` for programs that simply start in it.
    pub cwd_flag: Option<&'static str>,
    /// From the catalog; a user profile inherits them only when it starts the same program.
    pub capabilities: Capabilities,
}

impl Agent {
    pub fn from_user(mut profile: Profile) -> Self {
        // Existing profiles.toml files do not know `latest` yet; known shells get the source
        // anyway (see updates::builtin_for_command).
        if profile.latest.is_none() {
            if let Some((latest, packages)) = builtin_for_command(&profile.command) {
                profile.latest = Some(latest);
                profile.packages.get_or_insert(packages);
            }
        }
        Self {
            candidates: vec![profile.command.clone()],
            profile,
            source: Source::User,
            homepage: None,
            install_hint: None,
            cwd_flag: None,
            capabilities: Capabilities::default(),
        }
    }

    pub fn from_catalog(agent: CatalogAgent) -> Self {
        let install_hint = if cfg!(windows) {
            agent.install_windows
        } else {
            agent.install_unix
        };
        // `parse` guarantees at least one command; the fallback only keeps the types honest.
        let command = agent.commands.first().cloned().unwrap_or_default();
        Self {
            profile: Profile {
                id: agent.id,
                name: agent.name,
                command,
                args: agent.args,
                cwd: None,
                env: Default::default(),
                secret_env: Vec::new(),
                icon: agent.icon,
                version_args: agent.version_args,
                resume_args: agent.resume_args,
                session_id_args: agent.session_id_args,
                resume_id_args: agent.resume_id_args,
                sub_agents: agent.sub_agents,
                latest: agent.latest,
                update_hint: agent.update_hint,
                packages: agent.packages,
                self_update: agent.self_update,
            },
            source: Source::Catalog,
            candidates: agent.commands,
            homepage: agent.homepage,
            install_hint,
            cwd_flag: None,
            capabilities: agent.capabilities,
        }
    }

    pub fn id(&self) -> &str {
        &self.profile.id
    }

    /// First candidate that is found as a program, along with its resolved path. For catalog
    /// entries a launcher of another program (the VS Code `copilot` shim) does not count; the
    /// search goes on behind it.
    pub fn resolve(&self, path: Option<&OsStr>, pathext: Option<&OsStr>) -> Option<Resolved<'_>> {
        self.candidates.iter().find_map(|command| {
            let program = if self.source == Source::Catalog {
                resolve_all(command, path, pathext)
                    .into_iter()
                    .find(|program| !is_foreign_launcher(program))
            } else {
                resolve_program(command, path, pathext)
            };
            program.map(|program| Resolved { command, program })
        })
    }

    /// Text for the "not found" message.
    pub fn command_label(&self) -> String {
        self.candidates.join(" | ")
    }

    /// What the frontend sees. Of `env` only the names: values may contain secrets, the edit
    /// form fetches them separately (`profile_env`). Of `secret_env` only the names, as in the file.
    pub fn view(&self, path: Option<&OsStr>, pathext: Option<&OsStr>) -> ProfileView {
        let resolved = self.resolve(path, pathext);
        ProfileView {
            id: self.profile.id.clone(),
            name: self.profile.name.clone(),
            command: resolved
                .as_ref()
                .map_or(&self.profile.command, |r| r.command)
                .clone(),
            args: self.profile.args.clone(),
            resume_args: self.profile.resume_args().map(<[String]>::to_vec),
            cwd: self.profile.cwd.clone(),
            icon: self.profile.icon.clone(),
            source: self.source,
            installed: resolved.is_some(),
            homepage: self.homepage.clone(),
            install_hint: self.install_hint.clone(),
            editable: self.source == Source::User,
            env: self.profile.env.keys().cloned().collect(),
            secret_env: self.profile.secret_env.clone(),
            team_capable: self.capabilities.gate(),
            team_block: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved<'a> {
    /// The candidate that was found, such as `claude`.
    pub command: &'a String,
    /// Full path to the program, such as `C:\...\claude.exe`.
    pub program: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileView {
    pub id: String,
    pub name: String,
    /// The command used to start: the candidate found, otherwise the first one.
    pub command: String,
    pub args: Vec<String>,
    /// Arguments that `create_session(resume: true)` starts with; `null` = resuming is not
    /// possible, so a resume starts normally as well.
    pub resume_args: Option<Vec<String>>,
    pub cwd: Option<PathBuf>,
    pub icon: Option<String>,
    pub source: Source,
    pub installed: bool,
    pub homepage: Option<String>,
    pub install_hint: Option<String>,
    /// A profile from profiles.toml, which Settings can change and delete. Catalog entries and
    /// WSL distributions are not; saving one under its id creates a user profile that replaces it.
    pub editable: bool,
    /// Names of the plain environment variables, never their values.
    pub env: Vec<String>,
    /// Names of the variables whose values are kept in the OS keychain.
    pub secret_env: Vec<String>,
    /// Can run as a team session: the agent gate can be installed for it
    /// ([`Capabilities::gate`]).
    pub team_capable: bool,
    /// Why it cannot run as a team session on this machine (or, for `FixedFolder`, only in its
    /// own folder); `None` when it can. Set by the manager, which knows version and settings.
    pub team_block: Option<TeamBlock>,
}

/// User profiles in file order, followed by the catalog agents and then the detected WSL
/// distributions ([`crate::wsl`]), each without a user profile of the same id.
///
/// A user profile with a catalog ID determines name, command, arguments and position. Homepage and
/// install command still come from the catalog: existing users whose profiles.toml still holds the
/// old default agents would otherwise never see how to install a missing agent.
///
/// Such a profile also takes `resume_args` from the catalog, but only if it has none of its own
/// and starts the same program (`command` is one of the catalog commands). The options
/// belong to the program; a profile with its own wrapper would otherwise get foreign arguments.
pub fn merge(user: Vec<Profile>, catalog: Vec<CatalogAgent>) -> Vec<Agent> {
    merge_all(user, catalog, Vec::new())
}

/// [`merge`] plus terminals found on this machine (WSL), which behave like catalog entries.
pub fn merge_all(user: Vec<Profile>, catalog: Vec<CatalogAgent>, found: Vec<Agent>) -> Vec<Agent> {
    // Ids are compared without case, as the keychain compares them: a hand-written `Claude`
    // replaces the catalog's `claude` instead of sharing its secrets as a second entry.
    let fold = crate::profile::fold_id;
    let taken: HashSet<String> = user
        .iter()
        .map(|p| fold(&p.id))
        .chain(catalog.iter().map(|a| fold(&a.id)))
        .collect();
    let found: Vec<Agent> = found
        .into_iter()
        .filter(|agent| !taken.contains(&fold(agent.id())))
        .collect();
    let taken: HashSet<String> = user.iter().map(|p| fold(&p.id)).collect();
    let metadata: std::collections::HashMap<String, Agent> = catalog
        .iter()
        .filter(|a| taken.contains(&fold(&a.id)))
        .map(|a| (fold(&a.id), Agent::from_catalog(a.clone())))
        .collect();
    user.into_iter()
        .map(|profile| {
            let mut agent = Agent::from_user(profile);
            if let Some(from_catalog) = metadata.get(&fold(agent.id())) {
                agent.homepage.clone_from(&from_catalog.homepage);
                agent.install_hint.clone_from(&from_catalog.install_hint);
                let same_program = from_catalog.candidates.contains(&agent.profile.command);
                if same_program {
                    inherit_session_options(&mut agent.profile, &from_catalog.profile);
                    // The hooks belong to the program, like its session options.
                    agent.capabilities = from_catalog.capabilities;
                }
                // Same program, same packages and self-updater.
                if same_program {
                    if agent.profile.packages.is_none() {
                        agent
                            .profile
                            .packages
                            .clone_from(&from_catalog.profile.packages);
                    }
                    if agent.profile.self_update.is_none() {
                        agent
                            .profile
                            .self_update
                            .clone_from(&from_catalog.profile.self_update);
                    }
                }
                // Same program, same version source; the update command belongs with it.
                if agent.profile.latest.is_none() && same_program {
                    agent
                        .profile
                        .latest
                        .clone_from(&from_catalog.profile.latest);
                    if agent.profile.update_hint.is_none() {
                        agent
                            .profile
                            .update_hint
                            .clone_from(&from_catalog.profile.update_hint);
                    }
                }
            }
            agent
        })
        .chain(
            catalog
                .into_iter()
                .filter(|a| !taken.contains(&fold(&a.id)))
                .map(Agent::from_catalog),
        )
        .chain(found)
        .collect()
}

/// Options in a profile's own `args` that already pick a conversation; the catalog's id
/// options must not be added to them (claude refuses `--session-id` with `--resume` or
/// `--continue` unless `--fork-session` is given).
const CONVERSATION_OPTIONS: &[&str] = &["--session-id", "--resume", "--continue", "-c", "-r"];

fn picks_conversation(args: &[String]) -> bool {
    args.iter().any(|arg| {
        CONVERSATION_OPTIONS.iter().any(|option| {
            arg == option || arg.strip_prefix(option).is_some_and(|r| r.starts_with('='))
        })
    })
}

/// Resume, conversation id and sub-agent options belong to the program, so a user profile that
/// starts the catalog's program takes each one it does not set itself. `session_id_args = []`
/// in the profile turns the conversation ids off.
fn inherit_session_options(profile: &mut Profile, catalog: &Profile) {
    if profile.resume_args.is_none() {
        profile.resume_args.clone_from(&catalog.resume_args);
    }
    let own_choice = picks_conversation(&profile.args);
    if profile.session_id_args.is_none() && !own_choice {
        profile.session_id_args.clone_from(&catalog.session_id_args);
    }
    if profile.resume_id_args.is_none() && !own_choice {
        profile.resume_id_args.clone_from(&catalog.resume_id_args);
    }
    if profile.sub_agents.is_none() {
        profile.sub_agents = catalog.sub_agents;
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    fn user(id: &str, command: &str) -> Profile {
        Profile {
            id: id.into(),
            name: format!("User {id}"),
            command: command.into(),
            args: vec!["--own".into()],
            cwd: None,
            env: [("TOKEN".to_owned(), "secret".to_owned())].into(),
            secret_env: Vec::new(),
            icon: None,
            version_args: None,
            resume_args: None,
            session_id_args: None,
            resume_id_args: None,
            sub_agents: None,
            latest: None,
            update_hint: None,
            packages: None,
            self_update: None,
        }
    }

    fn catalog(id: &str, commands: &[&str]) -> CatalogAgent {
        CatalogAgent {
            id: id.into(),
            name: format!("Catalog {id}"),
            commands: commands.iter().map(|c| (*c).to_owned()).collect(),
            args: Vec::new(),
            icon: Some("K".into()),
            homepage: Some(format!("https://example.org/{id}")),
            install_windows: Some(format!("win-install {id}")),
            install_unix: Some(format!("unix-install {id}")),
            version_args: None,
            resume_args: Some(vec!["--continue".into()]),
            session_id_args: None,
            resume_id_args: None,
            sub_agents: None,
            latest: None,
            update_hint: None,
            packages: None,
            self_update: None,
            capabilities: Capabilities::default(),
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn resume_args_in_builtin_catalog() {
        let agents = builtin().unwrap();
        let resume = |id: &str| {
            agents
                .iter()
                .find(|a| a.id == id)
                .unwrap_or_else(|| panic!("{id} missing"))
                .resume_args
                .clone()
        };
        // Evidence: docs/wiki/pages/agents/resume.md
        let expected: &[(&str, &[&str])] = &[
            ("claude", &["--continue"]),
            ("codex", &["resume", "--last"]),
            ("gemini", &["--resume"]),
            ("antigravity", &["--continue"]),
            ("copilot", &["--continue"]),
            ("cursor", &["--continue"]),
            ("kimi", &["--continue"]),
            ("kiro", &["chat", "--resume"]),
            ("opencode", &["--continue"]),
            ("goose", &["session", "--resume"]),
            ("pi", &["--continue"]),
            ("vibe", &["--continue"]),
            ("droid", &["--resume"]),
            ("aider", &["--restore-chat-history"]),
        ];
        for (id, args) in expected {
            assert_eq!(resume(id), Some(strings(args)), "{id}");
        }
        for id in [
            "qwen", "omp", "forge", "kode", "codebuff", "amp", "crush", "cline",
        ] {
            assert_eq!(resume(id), None, "{id} has no evidence");
        }
        // A subcommand in args must come first again in resume_args, because they replace args.
        for agent in &agents {
            if let (Some(first), Some(resume)) = (agent.args.first(), &agent.resume_args) {
                assert_eq!(resume.first(), Some(first), "{}", agent.id);
            }
        }
    }

    #[test]
    fn latest_in_builtin_catalog() {
        let agents = builtin().unwrap();
        let key = |id: &str| {
            agents
                .iter()
                .find(|a| a.id == id)
                .unwrap_or_else(|| panic!("{id} missing"))
                .latest
                .as_ref()
                .map(|l| l.source().unwrap().cache_key())
        };
        // Evidence: docs/wiki/pages/agents/updates.md
        for (id, expected) in [
            ("claude", "npm:@anthropic-ai/claude-code"),
            ("codex", "npm:@openai/codex"),
            ("gemini", "npm:@google/gemini-cli"),
            ("copilot", "npm:@github/copilot"),
            ("kimi", "npm:@moonshot-ai/kimi-code"),
            ("qwen", "npm:@qwen-code/qwen-code"),
            ("opencode", "github:anomalyco/opencode:v"),
            ("aider", "pypi:aider-chat"),
            ("goose", "github:aaif-goose/goose:v"),
            ("crush", "npm:@charmland/crush"),
            ("pi", "npm:@earendil-works/pi-coding-agent"),
            ("omp", "npm:@oh-my-pi/pi-coding-agent"),
            ("kode", "npm:@shareai-lab/kode"),
            ("codebuff", "npm:codebuff"),
            ("cline", "npm:cline"),
            ("vibe", "pypi:mistral-vibe"),
            ("amp", "npm:@sourcegraph/amp"),
        ] {
            assert_eq!(key(id).as_deref(), Some(expected), "{id}");
        }
        for id in ["antigravity", "cursor", "kiro", "droid", "forge"] {
            assert_eq!(key(id), None, "{id} has no documented source");
        }
        let amp = agents.iter().find(|a| a.id == "amp").unwrap();
        assert_eq!(
            amp.latest.as_ref().unwrap().compare,
            crate::updates::Compare::Equal
        );

        // Default profile pwsh, and old profiles.toml files without the field get the same source.
        let windows = crate::profile::parse(include_str!("default_profiles_windows.toml")).unwrap();
        let pwsh = windows.iter().find(|p| p.id == "pwsh").unwrap();
        assert!(pwsh.latest.is_some());
        let old = Agent::from_user(user("pwsh", "pwsh"));
        assert_eq!(old.profile.latest, pwsh.latest);
        assert!(Agent::from_user(user("cmd", "cmd"))
            .profile
            .latest
            .is_none());
        // Old profiles.toml files get the winget package of pwsh as well.
        assert_eq!(
            old.profile.packages.and_then(|p| p.winget).as_deref(),
            Some("Microsoft.PowerShell")
        );
        // A user profile with a catalog ID and the same program inherits the source, the
        // packages and the self-updater.
        let merged = merge(vec![user("claude", "claude")], agents.clone());
        assert!(merged[0].profile.latest.is_some());
        assert!(merged[0].profile.packages.is_some());
        assert!(merged[0].profile.self_update.is_some());
        let merged = merge(vec![user("claude", "my-claude")], agents);
        assert!(merged[0].profile.latest.is_none());
    }

    #[test]
    fn packages_and_self_updaters_in_builtin_catalog() {
        // Evidence: docs/wiki/pages/sources/update-installers.md
        let agents = builtin().unwrap();
        let by_id = |id: &str| {
            agents
                .iter()
                .find(|a| a.id == id)
                .unwrap_or_else(|| panic!("{id} missing"))
        };
        let npm = |id: &str| by_id(id).packages.as_ref().and_then(|p| p.npm.clone());
        let winget = |id: &str| by_id(id).packages.as_ref().and_then(|p| p.winget.clone());
        for (id, package) in [
            ("claude", "@anthropic-ai/claude-code"),
            ("codex", "@openai/codex"),
            ("gemini", "@google/gemini-cli"),
            ("copilot", "@github/copilot"),
            ("kimi", "@moonshot-ai/kimi-code"),
            ("qwen", "@qwen-code/qwen-code"),
            ("opencode", "opencode-ai"),
            ("crush", "@charmland/crush"),
            ("pi", "@earendil-works/pi-coding-agent"),
            ("omp", "@oh-my-pi/pi-coding-agent"),
            ("kode", "@shareai-lab/kode"),
            ("codebuff", "codebuff"),
            ("cline", "cline"),
            ("amp", "@sourcegraph/amp"),
        ] {
            assert_eq!(npm(id).as_deref(), Some(package), "{id}");
        }
        for (id, package) in [
            ("claude", "Anthropic.ClaudeCode"),
            ("codex", "OpenAI.Codex"),
            ("copilot", "GitHub.Copilot"),
            ("kimi", "MoonshotAI.KimiCodeCLI"),
            ("crush", "charmbracelet.crush"),
            ("amp", "Sourcegraph.Amp"),
        ] {
            assert_eq!(winget(id).as_deref(), Some(package), "{id}");
        }
        for id in ["aider", "vibe"] {
            let pypi = by_id(id).packages.as_ref().and_then(|p| p.pypi.clone());
            assert!(pypi.is_some(), "{id}");
        }
        // Look-alikes stay out: Pressly.Goose is a database tool, Google.Antigravity and
        // Anysphere.Cursor are IDEs, npm cursor-agent is not Cursor's.
        for id in ["goose", "antigravity", "cursor", "kiro", "droid", "forge"] {
            assert!(by_id(id).packages.is_none(), "{id}");
        }
        for (id, args) in [
            ("claude", "update"),
            ("codex", "update"),
            ("opencode", "upgrade"),
            ("pi", "update"),
            ("goose", "update"),
        ] {
            let update = by_id(id).self_update.as_ref().unwrap();
            assert_eq!(update.args, [args], "{id}");
            assert!(update.aware, "{id}");
        }
        // Fixed update commands would run the wrong installer for a copy installed another way.
        assert!(agents.iter().all(|a| a.update_hint.is_none()));
        let windows = crate::profile::parse(include_str!("default_profiles_windows.toml")).unwrap();
        let pwsh = windows.iter().find(|p| p.id == "pwsh").unwrap();
        assert_eq!(
            pwsh.packages.as_ref().and_then(|p| p.winget.as_deref()),
            Some("Microsoft.PowerShell")
        );
        assert!(pwsh.update_hint.is_none());
        let bad = "[[agent]]
id='a'
name='A'
commands=['a']
packages={ winget='x y' }
";
        assert!(matches!(parse(bad), Err(CatalogError::Invalid { .. })));
    }

    #[test]
    fn copilot_shim_of_vs_code_does_not_count_as_installed() {
        let dir = TempDir::new("shim");
        let shim_dir = dir
            .0
            .join("Microsoft VS Code")
            .join("bin")
            .join("copilot-shim");
        std::fs::create_dir_all(&shim_dir).unwrap();
        let file = if cfg!(windows) {
            "copilot.exe"
        } else {
            "copilot"
        };
        std::fs::write(shim_dir.join(file), b"").unwrap();
        let real = dir.program("copilot");
        let catalog_entry = Agent::from_catalog(catalog("copilot", &["copilot"]));
        let only_shim = std::env::join_paths([&shim_dir]).unwrap();
        assert_eq!(catalog_entry.resolve(Some(&only_shim), None), None);
        assert!(!catalog_entry.view(Some(&only_shim), None).installed);
        // A real copy later on the PATH is found behind the shim.
        let both = std::env::join_paths([&shim_dir, &dir.0]).unwrap();
        assert_eq!(
            catalog_entry.resolve(Some(&both), None).unwrap().program,
            real
        );
        // A user profile starts what it names.
        let own = Agent::from_user(user("mine", "copilot"));
        assert!(own.resolve(Some(&only_shim), None).is_some());
    }

    #[test]
    fn claude_has_conversation_ids_and_sub_agents() {
        // Evidence: docs/wiki/pages/agents/resume.md, docs/wiki/pages/sources/subagent-transcripts.md
        let agents = builtin().unwrap();
        let claude = agents.iter().find(|a| a.id == "claude").unwrap();
        assert_eq!(
            claude.session_id_args,
            Some(strings(&["--session-id", "{id}"]))
        );
        assert_eq!(claude.resume_id_args, Some(strings(&["--resume", "{id}"])));
        assert_eq!(
            claude.sub_agents,
            Some(crate::subagents::SubAgentKind::Claude)
        );
        for agent in agents.iter().filter(|a| a.id != "claude") {
            assert_eq!(agent.session_id_args, None, "{}", agent.id);
            assert_eq!(agent.sub_agents, None, "{}", agent.id);
        }
        // Inherited by a user profile with the same program only.
        let merged = merge(
            vec![user("claude", "claude"), user("x", "y")],
            agents.clone(),
        );
        assert!(merged[0].profile.session_id_args().is_some());
        assert!(merged[0].profile.sub_agents.is_some());
        let merged = merge(vec![user("claude", "my-claude")], agents.clone());
        assert!(merged[0].profile.session_id_args().is_none());
        assert!(merged[0].profile.sub_agents.is_none());
        // Own arguments that already pick a conversation, and the explicit opt-out.
        for args in [
            &["--continue"][..],
            &["-c"],
            &["--resume", "x"],
            &["-r"],
            &["--session-id=x"],
        ] {
            let mut own = user("claude", "claude");
            own.args = strings(args);
            let merged = merge(vec![own], agents.clone());
            assert!(merged[0].profile.session_id_args.is_none(), "{args:?}");
            assert!(merged[0].profile.resume_id_args.is_none(), "{args:?}");
        }
        let mut opted_out = user("claude", "claude");
        opted_out.session_id_args = Some(Vec::new());
        let merged = merge(vec![opted_out], agents.clone());
        assert_eq!(merged[0].profile.session_id_args(), None);
        // Similar looking arguments do not count.
        let mut other = user("claude", "claude");
        other.args = strings(&["--resume-fast", "-cx"]);
        let merged = merge(vec![other], agents);
        assert!(merged[0].profile.session_id_args().is_some());
        let bad = "[[agent]]\nid='a'\nname='A'\ncommands=['a']\nresume_id_args=['--resume']\n";
        assert!(matches!(parse(bad), Err(CatalogError::Invalid { .. })));
    }

    #[test]
    fn user_profile_inherits_resume_args_only_for_same_program() {
        let mut own_profile = user("codex", "codex");
        own_profile.resume_args = Some(strings(&["--my-resume"]));
        let merged = merge(
            vec![
                user("claude", "claude"),
                user("codex-wrapper", "x"),
                user("gemini", "my-wrapper"),
                own_profile,
            ],
            vec![
                catalog("claude", &["claude"]),
                catalog("gemini", &["gemini"]),
                catalog("codex", &["codex"]),
            ],
        );
        let by_id = |id: &str| merged.iter().find(|a| a.id() == id).unwrap();
        assert_eq!(
            by_id("claude").profile.resume_args(),
            Some(&strings(&["--continue"])[..])
        );
        // A different program under a catalog ID: no foreign arguments.
        assert_eq!(by_id("gemini").profile.resume_args(), None);
        // Its own setting wins.
        assert_eq!(
            by_id("codex").profile.resume_args(),
            Some(&strings(&["--my-resume"])[..])
        );
        assert_eq!(by_id("codex-wrapper").profile.resume_args(), None);
        let view = serde_json::to_value(by_id("claude").view(None, None)).unwrap();
        assert_eq!(view["resumeArgs"], serde_json::json!(["--continue"]));
        let view = serde_json::to_value(by_id("gemini").view(None, None)).unwrap();
        assert!(view["resumeArgs"].is_null());
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("hangar-catalog-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        /// Creates a "program", on Windows with an extension from PATHEXT.
        fn program(&self, name: &str) -> PathBuf {
            let file = if cfg!(windows) {
                format!("{name}.exe")
            } else {
                name.to_owned()
            };
            let path = self.0.join(file);
            std::fs::write(&path, b"").unwrap();
            path
        }
        fn path(&self) -> OsString {
            std::env::join_paths([&self.0]).unwrap()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn builtin_catalog_is_valid() {
        let agents = builtin().unwrap();
        let ids: Vec<&str> = agents.iter().map(|a| a.id.as_str()).collect();
        for id in ["claude", "codex", "gemini", "aider"] {
            assert!(ids.contains(&id), "{id} missing from the catalog");
        }
        // Install commands are optional: they are only present if the vendor docs back them up
        // (docs/wiki/pages/agents/catalog.md). Every entry must have a source, though.
        for agent in &agents {
            assert!(agent.homepage.is_some(), "{}: homepage missing", agent.id);
            for hint in [&agent.install_windows, &agent.install_unix]
                .into_iter()
                .flatten()
            {
                assert!(
                    !hint.trim().is_empty(),
                    "{}: empty install command",
                    agent.id
                );
            }
            // update_hint gets executed: each one must split into argv without a shell.
            let hints = agent.update_hint.iter().flat_map(|h| [&h.windows, &h.unix]);
            for hint in hints.flatten() {
                assert!(
                    crate::updates::split_command(hint).is_ok(),
                    "{}: update_hint {hint:?} is not a plain command",
                    agent.id
                );
            }
        }
    }

    #[test]
    fn full_entry() {
        let agents = parse(
            r#"
            [[agent]]
            id = "x"
            name = "X"
            commands = ["x", "x-cli"]
            args = ["--a", "b c"]
            icon = "X"
            homepage = "https://x.example"
            install_windows = "winget install x"
            install_unix = "brew install x"
            "#,
        )
        .unwrap();
        assert_eq!(agents[0].commands, vec!["x", "x-cli"]);
        assert_eq!(agents[0].args, vec!["--a", "b c"]);
        assert_eq!(agents[0].install_unix.as_deref(), Some("brew install x"));
    }

    #[test]
    fn minimal_entry_and_empty_catalog() {
        let a = &parse("[[agent]]\nid='a'\nname='A'\ncommands=['a']\n").unwrap()[0];
        assert!(a.args.is_empty());
        assert_eq!((a.icon.as_ref(), a.homepage.as_ref()), (None, None));
        assert!(parse("").unwrap().is_empty());
    }

    #[test]
    fn invalid_catalogs() {
        let invalid = [
            "[[agent]]\nid='a'\nname='A'\ncommands=[]\n",
            "[[agent]]\nid='a'\nname='A'\ncommands=[' ']\n",
            "[[agent]]\nid='a'\nname='A'\ncommands=['bin/a']\n",
            "[[agent]]\nid='a'\nname='A'\ncommands=['C:\\\\a.exe']\n",
            "[[agent]]\nid='a b'\nname='A'\ncommands=['a']\n",
            "[[agent]]\nid=''\nname='A'\ncommands=['a']\n",
            "[[agent]]\nid='a'\nname=' '\ncommands=['a']\n",
        ];
        for source in invalid {
            assert!(
                matches!(parse(source), Err(CatalogError::Invalid { .. })),
                "{source:?} should have been rejected"
            );
        }
        let duplicate = "[[agent]]\nid='a'\nname='A'\ncommands=['a']\n[[agent]]\nid='a'\nname='B'\ncommands=['b']\n";
        assert!(matches!(parse(duplicate), Err(CatalogError::DuplicateId(id)) if id == "a"));
        // Required field missing, unknown field, typo in the table name.
        for source in [
            "[[agent]]\nid='a'\nname='A'\n",
            "[[agent]]\nid='a'\nname='A'\ncommands=['a']\ncommand='a'\n",
            "[[agents]]\nid='a'\nname='A'\ncommands=['a']\n",
        ] {
            assert!(matches!(parse(source), Err(CatalogError::Parse(_))));
        }
    }

    #[test]
    fn merge_puts_user_first_then_catalog() {
        let merged = merge(
            vec![user("pwsh", "pwsh"), user("cmd", "cmd")],
            vec![catalog("claude", &["claude"]), catalog("codex", &["codex"])],
        );
        let ids: Vec<&str> = merged.iter().map(Agent::id).collect();
        assert_eq!(ids, ["pwsh", "cmd", "claude", "codex"]);
        assert_eq!(merged[0].source, Source::User);
        assert_eq!(merged[2].source, Source::Catalog);
        assert_eq!(merged[2].profile.icon.as_deref(), Some("K"));
        assert!(merged[2].profile.env.is_empty());
    }

    #[test]
    fn found_terminals_come_last_and_yield_to_profiles() {
        let found = vec![
            crate::wsl::agent("Ubuntu"),
            crate::wsl::agent("Debian"),
            crate::wsl::agent("claude"),
        ];
        let merged = merge_all(
            vec![user("pwsh", "pwsh"), user("wsl-debian", "my-debian")],
            vec![
                catalog("claude", &["claude"]),
                catalog("wsl-claude", &["x"]),
            ],
            found,
        );
        let ids: Vec<&str> = merged.iter().map(Agent::id).collect();
        assert_eq!(
            ids,
            ["pwsh", "wsl-debian", "claude", "wsl-claude", "wsl-ubuntu"]
        );
        // The user's own profile under a WSL id wins and is editable.
        assert_eq!(merged[1].profile.command, "my-debian");
        assert!(merged[1].view(None, None).editable);
        assert_eq!(merged[4].cwd_flag, Some("--cd"));
    }

    #[test]
    fn ids_differing_only_in_case_are_one_entry() {
        // The keychain ignores case: a hand-written `Claude` must not stand beside `claude`.
        let merged = merge_all(
            vec![user("Claude", "my-claude"), user("WSL-Debian", "my-debian")],
            vec![catalog("claude", &["claude"])],
            vec![crate::wsl::agent("Debian")],
        );
        let ids: Vec<&str> = merged.iter().map(Agent::id).collect();
        assert_eq!(ids, ["Claude", "WSL-Debian"]);
        assert_eq!(merged[0].source, Source::User);
        assert!(merged[0].homepage.is_some(), "catalog hints still apply");
    }

    #[test]
    fn user_profile_overrides_catalog_keeps_hints() {
        let merged = merge(
            vec![user("codex", "my-codex")],
            vec![catalog("claude", &["claude"]), catalog("codex", &["codex"])],
        );
        let ids: Vec<&str> = merged.iter().map(Agent::id).collect();
        // The user profile determines position, command and arguments; the catalog entry does not appear twice.
        assert_eq!(ids, ["codex", "claude"]);
        let codex = &merged[0];
        assert_eq!(codex.source, Source::User);
        assert_eq!(codex.profile.command, "my-codex");
        assert_eq!(codex.profile.args, vec!["--own"]);
        assert_eq!(codex.candidates, vec!["my-codex"]);
        // Homepage and install command are kept from the catalog.
        let expected_hint = if cfg!(windows) {
            "win-install codex"
        } else {
            "unix-install codex"
        };
        assert_eq!(codex.homepage.as_deref(), Some("https://example.org/codex"));
        assert_eq!(codex.install_hint.as_deref(), Some(expected_hint));
        // User profiles without a catalog ID get neither homepage nor install command.
        let own = merge(vec![user("own", "own")], vec![catalog("codex", &["codex"])]);
        assert_eq!(
            (own[0].homepage.as_ref(), own[0].install_hint.as_ref()),
            (None, None)
        );
    }

    #[test]
    fn capabilities_in_builtin_catalog() {
        let agents = builtin().unwrap();
        let caps = |id: &str| {
            agents
                .iter()
                .find(|a| a.id == id)
                .unwrap_or_else(|| panic!("{id} missing"))
                .capabilities
        };
        // Evidence: docs/wiki/pages/sources/claude-http-hook-measurement.md.
        let claude = caps("claude");
        assert!(claude.pre_edit_block && claude.context_inject && claude.per_session_config);
        assert!(claude.gate());
        // Recipes: docs/wiki/pages/syntheses/agent-gate-adapters.md. Codex, OpenCode and Pi were
        // measured live; the others have their recipe but wait for that measurement.
        for (id, format, measured, floor) in [
            ("codex", HookFormat::CodexConfigFlags, true, "0.167.2"),
            ("gemini", HookFormat::GeminiSystemSettings, false, "0.68.0"),
            ("copilot", HookFormat::CopilotPlugin, false, "1.0.109"),
            ("qwen", HookFormat::QwenSystemSettings, false, "0.27.2"),
            ("opencode", HookFormat::OpencodePlugin, true, "1.19.5"),
            ("pi", HookFormat::PiExtension, true, "1.5.2"),
            ("omp", HookFormat::OmpExtension, false, "18.19.0"),
            ("droid", HookFormat::DroidSettings, false, "0.174.0"),
        ] {
            let caps = caps(id);
            assert_eq!(caps.hook_format, Some(format), "{id}");
            assert_eq!(caps.measured, measured, "{id}");
            assert_eq!(caps.gate(), measured, "{id}");
            assert_eq!(caps.min_version, MinVersion::parse(floor), "{id}");
        }
        assert!(claude.measured && claude.min_version.is_none());
        // Context goes in through Qwen's HTTP prompt hook only; the helper hands none yet.
        assert!(caps("qwen").context_inject);
        for id in [
            "codex", "gemini", "copilot", "opencode", "pi", "omp", "droid",
        ] {
            assert!(!caps(id).context_inject, "{id}");
        }
        assert_eq!(caps("aider"), Capabilities::default());
        assert!(!caps("aider").gate());
        let gated: Vec<&str> = agents
            .iter()
            .filter(|a| a.capabilities.gate())
            .map(|a| a.id.as_str())
            .collect();
        assert_eq!(gated, ["claude", "codex", "opencode", "pi"]);
    }

    #[test]
    fn versions_compare_by_number_not_by_text() {
        let v = |text: &str| MinVersion::parse(text).unwrap();
        assert!(v("0.167.10") > v("0.167.9"));
        assert!(v("1.0.109") > v("0.999.999"));
        assert_eq!(v("0.68"), v("0.68.0"));
        assert_eq!(v("v2.1.3"), v("2.1.3"));
        // A pre-release or build suffix counts as its release.
        assert_eq!(v("1.0.0-preview.1"), v("1.0.0"));
        assert_eq!(v("2.1.319 (Claude Code)"), v("2.1.319"));
        for bad in ["", "x", "1..2", "1.2.3.4.5", "a.b", "-1"] {
            assert_eq!(MinVersion::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn team_ready_needs_the_floor_a_known_version_and_no_admin_settings() {
        let caps = Capabilities {
            pre_edit_block: true,
            per_session_config: true,
            hook_format: Some(HookFormat::CodexConfigFlags),
            measured: true,
            min_version: MinVersion::parse("0.167.2"),
            ..Capabilities::default()
        };
        assert!(caps.team_ready(Some("0.167.2"), false));
        assert!(caps.team_ready(Some("0.168.0"), false));
        assert!(!caps.team_ready(Some("0.167.1"), false));
        assert!(!caps.team_ready(None, false));
        assert!(!caps.team_ready(Some("unknown"), false));
        assert!(!caps.team_ready(Some("0.168.0"), true));
        // Without a floor the version does not matter; unmeasured is never ready.
        let no_floor = Capabilities {
            min_version: None,
            ..caps
        };
        assert!(no_floor.team_ready(None, false));
        let unmeasured = Capabilities {
            measured: false,
            ..caps
        };
        assert!(!unmeasured.team_ready(Some("9.0.0"), false));
        // The catalog rejects a floor that is not a version.
        let bad = "[[agent]]\nid='x'\nname='X'\ncommands=['x']\ncapabilities = { min_version = 'soon' }\n";
        assert!(parse(bad).is_err());
    }

    #[test]
    fn team_block_names_the_first_reason_the_program_cannot_be_a_team_session() {
        let caps = Capabilities {
            pre_edit_block: true,
            per_session_config: true,
            hook_format: Some(HookFormat::OpencodePlugin),
            measured: true,
            min_version: MinVersion::parse("1.19.5"),
            ..Capabilities::default()
        };
        assert_eq!(caps.team_block(Some("1.19.5"), false), None);
        assert_eq!(
            caps.team_block(Some("1.18.0"), false),
            Some(TeamBlock::BelowMinVersion {
                min: "1.19.5".into(),
                installed: "1.18.0".into()
            })
        );
        assert_eq!(
            caps.team_block(None, false),
            Some(TeamBlock::UnknownVersion)
        );
        assert_eq!(
            caps.team_block(Some("garbage"), false),
            Some(TeamBlock::UnknownVersion)
        );
        // Administrator settings come before the version: updating would not help.
        assert_eq!(
            caps.team_block(Some("0.1.0"), true),
            Some(TeamBlock::AdminSettings)
        );
        let unmeasured = Capabilities {
            measured: false,
            ..caps
        };
        assert_eq!(
            unmeasured.team_block(Some("9.0.0"), true),
            Some(TeamBlock::NotMeasured)
        );
        let no_recipe = Capabilities {
            hook_format: None,
            ..caps
        };
        assert_eq!(
            no_recipe.team_block(Some("9.0.0"), false),
            Some(TeamBlock::NoHooks)
        );
        assert_eq!(
            Capabilities::default().team_block(None, false),
            Some(TeamBlock::NoHooks)
        );
        // The window reads the reason by its `kind`.
        let json = serde_json::to_value(caps.team_block(Some("1.0"), false)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"kind": "belowMinVersion", "min": "1.19.5", "installed": "1.0.0"})
        );
        assert_eq!(
            serde_json::to_value(TeamBlock::FixedFolder).unwrap(),
            serde_json::json!({"kind": "fixedFolder"})
        );
        assert_eq!(MinVersion::parse("1.2.3.4").unwrap().to_string(), "1.2.3.4");
    }

    #[test]
    fn capabilities_parse_and_reject_unknown_flags() {
        let base = "[[agent]]
id='x'
name='X'
commands=['x']
";
        assert_eq!(
            parse(base).unwrap()[0].capabilities,
            Capabilities::default()
        );
        let partial = format!(
            "{base}capabilities = {{ pre_edit_block = true }}
"
        );
        let caps = parse(&partial).unwrap()[0].capabilities;
        assert!(caps.pre_edit_block && !caps.per_session_config && !caps.gate());
        let unknown = format!(
            "{base}capabilities = {{ pre_edit_blocks = true }}
"
        );
        assert!(parse(&unknown).is_err());
        let wrong_type = format!(
            "{base}capabilities = {{ pre_edit_block = 'yes' }}
"
        );
        assert!(parse(&wrong_type).is_err());
    }

    #[test]
    fn capabilities_follow_the_program_into_user_profiles() {
        let mut with_gate = catalog("claude", &["claude"]);
        with_gate.capabilities = Capabilities {
            pre_edit_block: true,
            context_inject: true,
            per_session_config: true,
            hook_format: Some(HookFormat::ClaudeSettings),
            measured: true,
            min_version: None,
        };
        let same = merge(vec![user("claude", "claude")], vec![with_gate.clone()]);
        assert!(same[0].capabilities.gate());
        // A wrapper of its own may not understand the hooks.
        let other = merge(vec![user("claude", "my-wrapper")], vec![with_gate]);
        assert!(!other[0].capabilities.gate());
        assert!(!Agent::from_user(user("u", "claude")).capabilities.gate());
    }

    #[test]
    fn install_hint_matches_platform() {
        let agent = Agent::from_catalog(catalog("x", &["x"]));
        let expected = if cfg!(windows) {
            "win-install x"
        } else {
            "unix-install x"
        };
        assert_eq!(agent.install_hint.as_deref(), Some(expected));
    }

    #[test]
    fn detection_first_found_candidate_wins() {
        let dir = TempDir::new("detect");
        let found = dir.program("second-name");
        dir.program("third-name");
        let path = dir.path();
        let agent = Agent::from_catalog(catalog("x", &["first-name", "second-name", "third-name"]));

        let resolved = agent.resolve(Some(&path), None).unwrap();
        assert_eq!(resolved.command, "second-name");
        assert_eq!(resolved.program, found);

        let view = agent.view(Some(&path), None);
        assert!(view.installed);
        assert_eq!(view.command, "second-name");
    }

    #[test]
    fn detection_not_installed() {
        let dir = TempDir::new("missing");
        let path = dir.path();
        let agent = Agent::from_catalog(catalog("x", &["missing-a", "missing-b"]));
        assert_eq!(agent.resolve(Some(&path), None), None);
        let view = agent.view(Some(&path), None);
        assert!(!view.installed);
        assert_eq!(view.command, "missing-a");
        assert_eq!(agent.command_label(), "missing-a | missing-b");
        // Without PATH nothing is found, rather than guessing.
        assert!(!agent.view(None, None).installed);
    }

    #[test]
    fn detection_sees_freshly_installed_program() {
        let dir = TempDir::new("fresh");
        let path = dir.path();
        let agent = Agent::from_catalog(catalog("x", &["late"]));
        assert!(!agent.view(Some(&path), None).installed);
        dir.program("late");
        assert!(agent.view(Some(&path), None).installed);
    }

    #[test]
    fn user_profile_with_full_path_is_detected() {
        let dir = TempDir::new("userpath");
        let program = dir.program("own tool");
        let command = dir.0.join("own tool");
        let agent = Agent::from_user(user("own", command.to_str().unwrap()));
        assert_eq!(agent.resolve(None, None).unwrap().program, program);
    }

    #[test]
    fn view_without_env_in_camel_case() {
        let dir = TempDir::new("view");
        let json = serde_json::to_value(
            Agent::from_user(user("u", "missing")).view(Some(&dir.path()), None),
        )
        .unwrap();
        // Names only: the value "secret" never appears.
        assert_eq!(json["env"], serde_json::json!(["TOKEN"]));
        assert!(!json.to_string().contains("secret\""));
        assert_eq!(json["editable"], true);
        assert_eq!(json["secretEnv"], serde_json::json!([]));
        assert_eq!(json["source"], "user");
        assert_eq!(json["installed"], false);
        assert!(json["homepage"].is_null());
        assert!(json["installHint"].is_null());

        let json = serde_json::to_value(Agent::from_catalog(catalog("c", &["c"])).view(None, None))
            .unwrap();
        assert_eq!(json["source"], "catalog");
        assert_eq!(json["editable"], false);
        assert_eq!(json["homepage"], "https://example.org/c");
        assert!(json["installHint"].is_string());
        assert_eq!(json["teamCapable"], false);
    }

    #[test]
    fn team_capable_comes_from_the_capabilities_alone() {
        let mut agent = Agent::from_catalog(catalog("any-agent", &["any"]));
        let view = |agent: &Agent| serde_json::to_value(agent.view(None, None)).unwrap();
        assert_eq!(view(&agent)["teamCapable"], false);
        // Blocking and per-session hooks without a format Hangar can write: not yet.
        agent.capabilities = Capabilities {
            pre_edit_block: true,
            context_inject: false,
            per_session_config: true,
            hook_format: None,
            measured: true,
            min_version: None,
        };
        assert_eq!(view(&agent)["teamCapable"], false);
        // A format alone is not enough either.
        agent.capabilities = Capabilities {
            pre_edit_block: false,
            context_inject: false,
            per_session_config: true,
            hook_format: Some(HookFormat::ClaudeSettings),
            measured: true,
            min_version: None,
        };
        assert_eq!(view(&agent)["teamCapable"], false);
        agent.capabilities.pre_edit_block = true;
        assert_eq!(view(&agent)["teamCapable"], true);
        assert_eq!(
            agent.capabilities.gate_format(),
            Some(HookFormat::ClaudeSettings)
        );
    }

    #[test]
    fn hook_format_parses_and_knows_its_own_arguments() {
        let base = "[[agent]]
id='x'
name='X'
commands=['x']
";
        let with_format = format!(
            "{base}capabilities = {{ pre_edit_block = true, per_session_config = true, hook_format = 'claude_settings', measured = true }}
"
        );
        let caps = parse(&with_format).unwrap()[0].capabilities;
        assert_eq!(caps.gate_format(), Some(HookFormat::ClaudeSettings));
        let unknown = format!(
            "{base}capabilities = {{ hook_format = 'other_cli' }}
"
        );
        assert!(parse(&unknown).is_err());
        let args = |list: &[&str]| list.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
        let none = BTreeMap::new();
        let format = HookFormat::ClaudeSettings;
        assert!(format.conflicts_with(&args(&["--settings", "user.json"]), &none, &[]));
        assert!(format.conflicts_with(&args(&["--settings=user.json"]), &none, &[]));
        assert!(!format.conflicts_with(&args(&["--model", "x"]), &none, &[]));
        for (name, format) in [
            ("qwen_system_settings", HookFormat::QwenSystemSettings),
            ("gemini_system_settings", HookFormat::GeminiSystemSettings),
            ("copilot_plugin", HookFormat::CopilotPlugin),
            ("codex_config_flags", HookFormat::CodexConfigFlags),
            ("pi_extension", HookFormat::PiExtension),
            ("omp_extension", HookFormat::OmpExtension),
            ("opencode_plugin", HookFormat::OpencodePlugin),
            ("droid_settings", HookFormat::DroidSettings),
        ] {
            let text = format!(
                "{base}capabilities = {{ pre_edit_block = true, per_session_config = true, hook_format = '{name}', measured = true }}
"
            );
            assert_eq!(
                parse(&text).unwrap()[0].capabilities.gate_format(),
                Some(format)
            );
        }
    }

    #[test]
    fn a_profile_variable_for_the_settings_file_conflicts() {
        let env = |name: &str| BTreeMap::from([(name.to_owned(), "x".to_owned())]);
        let none: Vec<String> = Vec::new();
        assert!(HookFormat::QwenSystemSettings.conflicts_with(
            &none,
            &env("QWEN_CODE_SYSTEM_SETTINGS_PATH"),
            &[]
        ));
        assert!(HookFormat::GeminiSystemSettings.conflicts_with(
            &none,
            &env("GEMINI_CLI_SYSTEM_SETTINGS_PATH"),
            &[]
        ));
        assert_eq!(
            HookFormat::GeminiSystemSettings.conflicts_with(
                &none,
                &env("gemini_cli_system_settings_path"),
                &[]
            ),
            cfg!(windows)
        );
        assert!(!HookFormat::QwenSystemSettings.conflicts_with(
            &none,
            &env("GEMINI_CLI_SYSTEM_SETTINGS_PATH"),
            &[]
        ));
        // Plugin folders add up; the user's own never conflict.
        let own = vec!["--plugin-dir".to_owned(), "mine".to_owned()];
        assert!(!HookFormat::CopilotPlugin.conflicts_with(&own, &BTreeMap::new(), &[]));
        // A user's own Codex hooks override sets the key Hangar's layer sets; other overrides
        // do not touch it.
        let codex = |list: &[&str]| {
            let args: Vec<String> = list.iter().map(|a| (*a).to_owned()).collect();
            HookFormat::CodexConfigFlags.conflicts_with(&args, &BTreeMap::new(), &[])
        };
        assert!(codex(&["-c", "hooks.PreToolUse=[]"]));
        assert!(codex(&["--config", " hooks = {}"]));
        assert!(codex(&["--config=hooks.PreToolUse.0.matcher='x'"]));
        assert!(!codex(&["-c", "hooks.PostToolUse=[]"]));
        assert!(!codex(&["-c", "model=\"o3\"", "resume", "--last"]));
        assert!(!codex(&["-c", "hooks_extra=1"]));
        assert!(!codex(&["-c"]));
        assert!(!codex(&["hooks.PreToolUse=[]"]));
    }

    #[test]
    fn the_plugin_formats_know_what_takes_their_place() {
        let args = |list: &[&str]| list.iter().map(|a| (*a).to_owned()).collect::<Vec<_>>();
        let none = BTreeMap::new();
        // Pi loads every `-e`, the user's own and `--no-extensions` included.
        assert!(!HookFormat::PiExtension.conflicts_with(
            &args(&["-e", "mine.ts", "-ne"]),
            &none,
            &[]
        ));
        // Oh My Pi refuses its allowlist next to `-e`.
        let omp = HookFormat::OmpExtension;
        assert!(omp.conflicts_with(&args(&["--trusted-extension", "/x.ts"]), &none, &[]));
        assert!(omp.conflicts_with(&args(&["--trusted-extension=/x.ts"]), &none, &[]));
        assert!(!omp.conflicts_with(&args(&["-e", "mine.ts"]), &none, &[]));
        // OpenCode: the profile's own inline configuration would be replaced, whether a plain or
        // a keychain variable; its own configuration folder adds up.
        let opencode = HookFormat::OpencodePlugin;
        let inline = BTreeMap::from([("OPENCODE_CONFIG_CONTENT".to_owned(), "{}".to_owned())]);
        assert!(opencode.conflicts_with(&[], &inline, &[]));
        assert!(opencode.conflicts_with(&[], &none, &["OPENCODE_CONFIG_CONTENT".to_owned()]));
        let folder = BTreeMap::from([("OPENCODE_CONFIG_DIR".to_owned(), "x".to_owned())]);
        assert!(!opencode.conflicts_with(&[], &folder, &[]));
        assert!(!opencode.conflicts_with(&args(&["--continue"]), &none, &[]));
        // A keychain variable counts like a plain one for the settings formats too.
        let secret = ["QWEN_CODE_SYSTEM_SETTINGS_PATH".to_owned()];
        assert!(HookFormat::QwenSystemSettings.conflicts_with(&[], &none, &secret));
        // Droid: a second settings file; how two combine is not known.
        let droid = HookFormat::DroidSettings;
        assert!(droid.conflicts_with(&args(&["--settings", "a.json"]), &none, &[]));
        assert!(droid.conflicts_with(&args(&["--settings=a.json"]), &none, &[]));
        assert!(!droid.conflicts_with(&args(&["--resume"]), &none, &[]));
    }
}
