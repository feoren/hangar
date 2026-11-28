//! State only the main window may read and write: the security switches (local claims, the agent
//! gate, the command guard), the queued messages as Hangar 0.10 to 0.13 kept them (since 0.14 they live
//! in `workspace.json`; the file is only read for their migration and stays as its marker,
//! emptied), the settings that decide what
//! reaches an agent or the machine ([`ProtectedSettings`]: the paste warning, automatic updates,
//! HANGAR.md for every agent, the snippets), the workspace templates (program and folder of
//! each session they start) and the start and notice preferences ([`MainPrefs`]: private or team
//! start, the last program, the favorite and recent folders, session restore, whether and when
//! a waiting session shows itself, the daily budget with its early warning and the day and level
//! its notice last showed, whether the morning summary is offered and for which day it last was,
//! and whether the dictation key is on), the partner projects ([`crate::partners`]: which
//! projects may send each other messages), the recently opened sessions
//! ([`ClosedSession`]: program and folder to start again with one click) and the remembered
//! link groups ([`crate::link_memory`]: which sessions formed a group, offered again after a
//! restart and linked only once the person confirms).
//!
//! The panel and file viewer windows share the main window's origin and with it its
//! `localStorage`. A compromised viewer (a crafted Markdown file) could otherwise turn the gate
//! off for the next start, queue a message or plant a snippet that Hangar later pastes into an
//! agent session, switch the paste warning or updates off, plant a template that starts a
//! program in a folder of its choice, point the next start at such a folder, start it private
//! (without the gate), silence the notices of a session that waits for the person, hide the
//! budget notice (a budget out of reach, today marked as warned), turn the dictation key on, so
//! that holding it opens the microphone, or plant a recently opened session that starts a program
//! in a folder of its choice on the next click. Here the files sit in the local app data folder, written
//! atomically ([`crate::persist::write_atomic`]) and validated on reading and on writing; the
//! commands answer the main window only ([`check_main`]).
//!
//! `None` from a load means nothing was stored yet: the main window then migrates the values it
//! kept in `localStorage` before (once) and removes them there.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, Serializer};

use crate::agent_models::{DefProgram, ModelChoice};
use crate::partner_inbox::Inbox;
use crate::partners::PartnerBook;
use crate::persist::{normalize_key, read_limited, write_atomic};

pub const SWITCHES_FILE: &str = "security-switches.json";
/// Legacy: read once to move the messages into `workspace.json`, then kept empty, so a value an
/// older version left in `localStorage` is never migrated again (`mainStore.ts`).
pub const QUEUE_FILE: &str = "queued-messages.json";
pub const SETTINGS_FILE: &str = "protected-settings.json";
pub const TEMPLATES_FILE: &str = "workspace-templates.json";
pub const PREFS_FILE: &str = "main-prefs.json";
pub const SUB_AGENTS_FILE: &str = "sub-agent-settings.json";
pub const PARTNERS_FILE: &str = "partner-projects.json";
/// Messages agents sent to partner projects that wait for the person ([`crate::partner_inbox`]).
pub const INBOX_FILE: &str = "partner-messages.json";
/// The recently opened sessions, to start again with one click (`recentlyOpened.ts`). The file
/// keeps its first name, so a list from before carries over.
pub const RECENTLY_CLOSED_FILE: &str = "recently-closed.json";
/// The composition of the link groups, offered again after a restart ([`crate::link_memory`]).
pub const LINK_GROUPS_FILE: &str = "link-groups.json";
/// Recently opened sessions kept, as the window bounds them (`recentlyOpened.ts` `KEEP_LIMIT`).
pub const MAX_RECENTLY_CLOSED: usize = 10;
/// Every entry at its longest in escaped JSON: folder, program, title, marker and id, with room.
const MAX_RECENTLY_CLOSED_BYTES: usize = MAX_RECENTLY_CLOSED
    * ((MAX_FOLDER_UNITS + MAX_PROFILE_ID_UNITS + crate::manager::MAX_TITLE_CHARS * 2) * 6 + 512)
    + 1024;
/// The latest `closedAt` the window can write: `Number.MAX_SAFE_INTEGER` milliseconds.
const MAX_CLOSED_AT: u64 = (1 << 53) - 1;
/// Every project with its longest folder in escaped JSON, the pairs by name, with room.
const MAX_PARTNERS_BYTES: usize = crate::partners::MAX_PROJECTS * (MAX_FOLDER_UNITS * 6 + 128)
    + crate::partners::MAX_PAIRS * 96
    + 1024;
/// A few programs with a short model each.
const MAX_SUB_AGENTS_BYTES: usize = 16 * 1024;
/// Queued messages kept at most, one per session ([`crate::persist::MAX_SESSIONS`]).
pub const MAX_QUEUED: usize = crate::persist::MAX_SESSIONS;
/// Length of one queued message in UTF-16 units, the same as in `workspace.json`.
pub const MAX_QUEUED_UNITS: usize = crate::persist::MAX_QUEUED_MESSAGE_UNITS;
/// Snippets kept at most, and the length of one in UTF-16 units, as the window bounds them
/// (`snippets.ts`: `SNIPPET_LIMIT`, `SNIPPET_MAX_LENGTH`).
pub const MAX_SNIPPETS: usize = 50;
pub const MAX_SNIPPET_UNITS: usize = 8000;
/// Workspace templates as the window bounds them (`templates.ts`).
pub const MAX_TEMPLATES: usize = 30;
pub const MAX_TEMPLATE_SESSIONS: usize = 16;
pub const MAX_TEMPLATE_NAME_CHARS: usize = 60;
pub const MAX_TEMPLATE_TITLE_CHARS: usize = 80;
pub const MAX_FOLDER_UNITS: usize = 4096;
pub const MAX_PROFILE_ID_UNITS: usize = 200;
/// Folder lists of the "Starts in" menu as the window bounds them (`folders.ts`).
pub const MAX_RECENT_FOLDERS: usize = 8;
pub const MAX_FAVORITE_FOLDERS: usize = 50;
/// Highest daily budget in dollars, as the window bounds it (`costs.ts` `MAX_BUDGET_USD`).
pub const MAX_BUDGET_USD: f64 = 100_000.0;
/// Lowest daily budget: one cent. The window rounds to cents, so anything below would read
/// there as zero and make it drop all preferences of the file.
pub const MIN_BUDGET_USD: f64 = 0.01;
/// The early budget warning's default share of the budget, in percent (`costs.ts`).
pub const DEFAULT_EARLY_PERCENT: u8 = 80;
/// Minutes in a day: quiet hours start and end before this (`quietHours.ts`).
const MINUTES_PER_DAY: u16 = 24 * 60;
/// IDs of snippets and templates: `^[a-z0-9-]{1,40}$`, as the window checks them.
const MAX_ID_LEN: usize = 40;
const MAX_SWITCHES_BYTES: usize = 4 * 1024;
/// Every message at its longest in UTF-8, with room for the keys and the JSON around them.
const MAX_QUEUE_BYTES: usize = MAX_QUEUED * (MAX_QUEUED_UNITS * 3 + 128);
/// Every snippet at its longest in UTF-8 (JSON may escape a character to six bytes), with room.
const MAX_SETTINGS_BYTES: usize = MAX_SNIPPETS * (MAX_SNIPPET_UNITS * 6 + 128) + 1024;
/// Every template at its longest: sessions with long folders dominate.
const MAX_TEMPLATES_BYTES: usize = MAX_TEMPLATES * MAX_TEMPLATE_SESSIONS * 32 * 1024;
/// Every folder at its longest in escaped JSON, with room for the program and the flags.
const MAX_PREFS_BYTES: usize =
    (MAX_RECENT_FOLDERS + MAX_FAVORITE_FOLDERS) * (MAX_FOLDER_UNITS * 6 + 8) + 4 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum MainStoreError {
    #[error("Only the main window may do this")]
    WrongWindow,
    #[error("Invalid queued message: {0}")]
    Invalid(&'static str),
    #[error("Invalid setting: {0}")]
    InvalidSetting(&'static str),
    #[error("Invalid workspace template: {0}")]
    InvalidTemplate(&'static str),
    #[error("Invalid preference: {0}")]
    InvalidPrefs(&'static str),
    #[error("Invalid recently opened session: {0}")]
    InvalidClosed(&'static str),
    #[error(transparent)]
    Partners(#[from] crate::partners::PartnerError),
    #[error(transparent)]
    LinkMemory(#[from] crate::link_memory::MemoryError),
    #[error("File {path} cannot be read or written: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Background task aborted: {0}")]
    Task(String),
}

/// No error text carries a queued message, a snippet or a folder.
impl Serialize for MainStoreError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// Only the window labelled `main` may call the commands of this store. The capabilities grant
/// them to it alone already; this check holds even if a capability file grew by mistake.
pub fn check_main(label: &str) -> Result<(), MainStoreError> {
    if label == crate::panel_window::MAIN_LABEL {
        Ok(())
    } else {
        Err(MainStoreError::WrongWindow)
    }
}

/// The switches that keep agents in check. All on by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SecuritySwitches {
    /// Local claims; the gate needs them.
    pub claims_enabled: bool,
    pub gate_enabled: bool,
    pub command_guard_enabled: bool,
}

impl Default for SecuritySwitches {
    fn default() -> Self {
        Self {
            claims_enabled: true,
            gate_enabled: true,
            command_guard_enabled: true,
        }
    }
}

/// Queued message text per session key (a UUID, as the workspace stores it).
pub type QueuedMessages = BTreeMap<String, String>;

/// Keys as UUIDs in lower case, texts not empty and not too long, not too many.
pub fn validate_queue(entries: QueuedMessages) -> Result<QueuedMessages, MainStoreError> {
    if entries.len() > MAX_QUEUED {
        return Err(MainStoreError::Invalid("too many messages"));
    }
    let mut valid = BTreeMap::new();
    for (key, text) in entries {
        let key = normalize_key(&key).map_err(|_| MainStoreError::Invalid("session key"))?;
        if text.trim().is_empty() {
            return Err(MainStoreError::Invalid("empty text"));
        }
        if text.encode_utf16().count() > MAX_QUEUED_UNITS {
            return Err(MainStoreError::Invalid("text too long"));
        }
        valid.insert(key, text);
    }
    Ok(valid)
}

/// One stored text for agents (`snippets.ts`), pasted into a session like the clipboard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snippet {
    pub id: String,
    pub text: String,
}

/// Settings that decide what reaches an agent or the machine. A damaged file gives the safe
/// defaults: the warning, updates and HANGAR.md on, no snippets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProtectedSettings {
    /// Ctrl+V and Shift+Insert ask before text that would run line by line.
    pub warn_multiline_paste: bool,
    pub auto_update: bool,
    /// Every agent start hands the agent HANGAR.md.
    pub hangar_md: bool,
    pub snippets: Vec<Snippet>,
    /// Claude Code, Qwen Code and Codex CLI started from Hangar show Hangar's status line in
    /// place of their own (ADR 0023). Missing in files from before: on.
    #[serde(default = "on")]
    pub status_line: bool,
}

fn on() -> bool {
    true
}

impl Default for ProtectedSettings {
    fn default() -> Self {
        Self {
            warn_multiline_paste: true,
            auto_update: true,
            hangar_md: true,
            snippets: Vec::new(),
            status_line: true,
        }
    }
}

/// The default model of every sub-agent per program, applied to each session Hangar starts
/// ([`crate::agent_models::launch`]), and the programs that do not get copies of the user's
/// definitions ([`crate::agent_provision`]). Protected: a model reaches every agent.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubAgentSettings {
    #[serde(default)]
    pub models: BTreeMap<DefProgram, ModelChoice>,
    #[serde(default)]
    pub provide_off: BTreeSet<DefProgram>,
    /// Hangar's own sub-agents are not placed in `~/.claude/agents`
    /// ([`crate::bundled_agents`]); on by default.
    #[serde(default)]
    pub bundled_off: bool,
}

/// Every choice checked for its program; empty ones are dropped.
pub fn validate_sub_agents(
    mut settings: SubAgentSettings,
) -> Result<SubAgentSettings, MainStoreError> {
    for (program, choice) in &settings.models {
        crate::agent_models::check_choice(*program, choice)
            .map_err(|_| MainStoreError::InvalidSetting("sub-agent model"))?;
    }
    settings.models.retain(|_, choice| !choice.is_empty());
    Ok(settings)
}

/// A Codex CLI effort Hangar offered earlier and no longer does
/// ([`crate::agent_models::RETIRED_CODEX_EFFORTS`]) is removed, so the stored model and switches
/// still load and the program's own effort applies. Any other value the checks refuse still
/// gives the defaults: only what Hangar itself once wrote is forgiven.
fn drop_retired_efforts(mut settings: SubAgentSettings) -> SubAgentSettings {
    if let Some(choice) = settings.models.get_mut(&DefProgram::Codex) {
        if choice
            .effort
            .as_deref()
            .is_some_and(|effort| crate::agent_models::RETIRED_CODEX_EFFORTS.contains(&effort))
        {
            choice.effort = None;
        }
    }
    settings
}

/// `^[a-z0-9-]{1,40}$`
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// C0 controls and DEL, as the window's `CONTROL` pattern.
fn has_control(text: &str) -> bool {
    text.chars().any(|c| c <= '\u{1f}' || c == '\u{7f}')
}

/// Not too many snippets, IDs valid and unique, texts not blank and not too long.
pub fn validate_settings(settings: &ProtectedSettings) -> Result<(), MainStoreError> {
    if settings.snippets.len() > MAX_SNIPPETS {
        return Err(MainStoreError::InvalidSetting("too many snippets"));
    }
    let mut ids = BTreeSet::new();
    for snippet in &settings.snippets {
        if !valid_id(&snippet.id) || !ids.insert(snippet.id.as_str()) {
            return Err(MainStoreError::InvalidSetting("snippet id"));
        }
        if snippet.text.trim().is_empty() {
            return Err(MainStoreError::InvalidSetting("empty snippet"));
        }
        if snippet.text.encode_utf16().count() > MAX_SNIPPET_UNITS {
            return Err(MainStoreError::InvalidSetting("snippet too long"));
        }
    }
    Ok(())
}

/// Whether a template session starts privately or as a team session in a fresh worktree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TemplateMode {
    Private,
    Team,
}

/// A template session's part in the link group the template forms (ADR 0014).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TemplateRole {
    Lead,
    Member,
}

/// One session of a template (`templates.ts`, `TemplateSession`). `member_name` and `role` say
/// how it joins the link group of a template that links its sessions; older templates have
/// neither.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateSession {
    pub profile_id: String,
    pub folder: String,
    pub mode: TemplateMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub member_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<TemplateRole>,
}

/// A named set of sessions that starts again with one click (`templates.ts`). `link`: its agent
/// sessions form a link group once they run, with "Ask before pasting" as `ask_first` says;
/// older templates have neither and link nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceTemplate {
    pub id: String,
    pub name: String,
    pub sessions: Vec<TemplateSession>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub link: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ask_first: bool,
}

/// A name or title as the window cleans it: not blank, no control characters, not too long.
fn valid_label(text: &str, max_chars: usize) -> bool {
    !text.trim().is_empty() && !has_control(text) && text.chars().count() <= max_chars
}

/// The bounds the window keeps (`parseTemplates`): not too many templates and sessions, IDs
/// valid and unique, every session with a program and a folder, member names valid and unique
/// within a template (`links.ts` `validMemberName`), at most one lead. Template names are not
/// compared: the window ignores case by the user's locale, which no check here could match
/// exactly.
pub fn validate_templates(templates: &[WorkspaceTemplate]) -> Result<(), MainStoreError> {
    if templates.len() > MAX_TEMPLATES {
        return Err(MainStoreError::InvalidTemplate("too many templates"));
    }
    let mut ids = BTreeSet::new();
    for template in templates {
        if !valid_id(&template.id) || !ids.insert(template.id.as_str()) {
            return Err(MainStoreError::InvalidTemplate("template id"));
        }
        if !valid_label(&template.name, MAX_TEMPLATE_NAME_CHARS) {
            return Err(MainStoreError::InvalidTemplate("name"));
        }
        if template.sessions.is_empty() || template.sessions.len() > MAX_TEMPLATE_SESSIONS {
            return Err(MainStoreError::InvalidTemplate("number of sessions"));
        }
        for session in &template.sessions {
            let profile = session.profile_id.encode_utf16().count();
            if session.profile_id.trim().is_empty() || profile > MAX_PROFILE_ID_UNITS {
                return Err(MainStoreError::InvalidTemplate("program"));
            }
            if session.folder.trim().is_empty()
                || has_control(&session.folder)
                || session.folder.encode_utf16().count() > MAX_FOLDER_UNITS
            {
                return Err(MainStoreError::InvalidTemplate("folder"));
            }
            if session
                .title
                .as_deref()
                .is_some_and(|title| !valid_label(title, MAX_TEMPLATE_TITLE_CHARS))
            {
                return Err(MainStoreError::InvalidTemplate("title"));
            }
        }
        let names: Vec<&str> = template
            .sessions
            .iter()
            .filter_map(|session| session.member_name.as_deref())
            .collect();
        for (index, name) in names.iter().enumerate() {
            if !crate::partners::valid_name(name)
                || *name == crate::links::ALL
                || *name == crate::links::LEAD
                || names[..index].contains(name)
            {
                return Err(MainStoreError::InvalidTemplate("member name"));
            }
        }
        let leads = template
            .sessions
            .iter()
            .filter(|session| session.role == Some(TemplateRole::Lead))
            .count();
        if leads > 1 {
            return Err(MainStoreError::InvalidTemplate("more than one lead"));
        }
    }
    Ok(())
}

/// How the next session starts (`startMode.ts`): in the folder itself, or as a team session in a
/// worktree of its own with the agent gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StartMode {
    #[default]
    Private,
    Team,
}

/// A daily window without taskbar flash, chime and desktop notifications (`quietHours.ts`), in
/// minutes after local midnight; `to` before `from` crosses midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuietHours {
    pub enabled: bool,
    pub from: u16,
    pub to: u16,
}

impl Default for QuietHours {
    fn default() -> Self {
        Self {
            enabled: false,
            from: 22 * 60,
            to: 7 * 60,
        }
    }
}

/// How far today's cost got when the budget notice showed (`costs.ts` `BudgetLevel`): the early
/// warning at a share of the budget, or the budget itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BudgetLevel {
    Early,
    Reached,
}

/// The local day (`YYYY-MM-DD`) and level the budget notice last showed: each level shows once a
/// day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetWarned {
    pub day: String,
    pub level: BudgetLevel,
}

/// `budgetWarned` as files store it: before the early warning existed, the day alone
/// (`budgetWarnedDay`), which was always the day the budget was reached.
#[derive(Deserialize)]
#[serde(untagged)]
enum StoredWarned {
    Day(String),
    Warned(BudgetWarned),
}

fn stored_warned<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<BudgetWarned>, D::Error> {
    Ok(
        Option::<StoredWarned>::deserialize(deserializer)?.map(|stored| match stored {
            StoredWarned::Day(day) => BudgetWarned {
                day,
                level: BudgetLevel::Reached,
            },
            StoredWarned::Warned(warned) => warned,
        }),
    )
}

fn default_early_percent() -> Option<u8> {
    Some(DEFAULT_EARLY_PERCENT)
}

fn enabled() -> bool {
    true
}

/// Where and how the next session starts, and whether a waiting session shows itself. A damaged
/// file gives the defaults: private start in the home folder, the first program, sessions
/// restored, every notice on, no budget.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MainPrefs {
    pub start_mode: StartMode,
    /// The profile ID Ctrl+Shift+T starts; `None`: the first program in the list.
    pub last_program: Option<String>,
    /// In the order the person arranged them.
    pub favorites: Vec<String>,
    /// Most recent first; the first is where the next session starts without another choice.
    pub recent: Vec<String>,
    pub restore_sessions: bool,
    pub attention_enabled: bool,
    pub taskbar_flash: bool,
    pub notify_desktop: bool,
    pub chime: bool,
    pub quiet_hours: QuietHours,
    /// Daily budget in dollars; `None`: no budget notice. Missing in files written before it
    /// existed.
    #[serde(default)]
    pub daily_budget: Option<f64>,
    /// The early budget warning at this share of the budget in percent (1 to 99); `None`: off.
    /// Missing in files written before it existed: the default.
    #[serde(default = "default_early_percent")]
    pub budget_early_percent: Option<u8>,
    /// The day and level the budget notice last showed. Older files kept only the day, under
    /// `budgetWarnedDay`; it becomes that day at the level "reached".
    #[serde(default, alias = "budgetWarnedDay", deserialize_with = "stored_warned")]
    pub budget_warned: Option<BudgetWarned>,
    /// Offer yesterday's summary on the first start or focus of a day. On unless turned off.
    #[serde(default = "enabled")]
    pub morning_review: bool,
    /// The local day (`YYYY-MM-DD`) whose summary was last offered, so it is offered once.
    #[serde(default)]
    pub morning_offered_day: Option<String>,
    /// Holding the dictation key opens the microphone. `None`: a file from before dictation; the
    /// window then takes over its old localStorage value once and stores it here.
    #[serde(default)]
    pub dictation_enabled: Option<bool>,
}

impl Default for MainPrefs {
    fn default() -> Self {
        Self {
            start_mode: StartMode::Private,
            last_program: None,
            favorites: Vec::new(),
            recent: Vec::new(),
            restore_sessions: true,
            attention_enabled: true,
            taskbar_flash: true,
            notify_desktop: true,
            chime: true,
            quiet_hours: QuietHours::default(),
            daily_budget: None,
            budget_early_percent: default_early_percent(),
            budget_warned: None,
            morning_review: true,
            morning_offered_day: None,
            dictation_enabled: Some(false),
        }
    }
}

/// A calendar day as `YYYY-MM-DD`: four digits, a month from 01 to 12 and a day from 01 to 31.
fn valid_day(day: &str) -> bool {
    let bytes = day.as_bytes();
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    if !(digits(0..4) && digits(5..7) && digits(8..10)) {
        return false;
    }
    let number = |range: std::ops::Range<usize>| {
        bytes[range]
            .iter()
            .fold(0_u32, |sum, digit| sum * 10 + u32::from(digit - b'0'))
    };
    (1..=12).contains(&number(5..7)) && (1..=31).contains(&number(8..10))
}

/// A folder as the window keeps it (`folders.ts` `validPath`): not blank, no control
/// characters, not too long.
fn valid_folder(folder: &str) -> bool {
    !folder.trim().is_empty()
        && !folder.chars().any(|c| c <= '\u{1f}')
        && folder.encode_utf16().count() <= MAX_FOLDER_UNITS
}

/// The bounds the window keeps: a program ID like a template's, folders like the menu's, quiet
/// hours within a day, a budget from [`MIN_BUDGET_USD`] up to [`MAX_BUDGET_USD`], an early
/// warning below the whole budget, the warned and the offered day as dates.
pub fn validate_prefs(prefs: &MainPrefs) -> Result<(), MainStoreError> {
    if let Some(program) = &prefs.last_program {
        if program.trim().is_empty()
            || has_control(program)
            || program.encode_utf16().count() > MAX_PROFILE_ID_UNITS
        {
            return Err(MainStoreError::InvalidPrefs("program"));
        }
    }
    if prefs.recent.len() > MAX_RECENT_FOLDERS || prefs.favorites.len() > MAX_FAVORITE_FOLDERS {
        return Err(MainStoreError::InvalidPrefs("too many folders"));
    }
    if !prefs
        .recent
        .iter()
        .chain(&prefs.favorites)
        .all(|folder| valid_folder(folder))
    {
        return Err(MainStoreError::InvalidPrefs("folder"));
    }
    let hours = prefs.quiet_hours;
    if hours.from >= MINUTES_PER_DAY || hours.to >= MINUTES_PER_DAY {
        return Err(MainStoreError::InvalidPrefs("quiet hours"));
    }
    if let Some(budget) = prefs.daily_budget {
        if !budget.is_finite() || budget < MIN_BUDGET_USD || budget > MAX_BUDGET_USD {
            return Err(MainStoreError::InvalidPrefs("budget"));
        }
    }
    if let Some(percent) = prefs.budget_early_percent {
        if !(1..=99).contains(&percent) {
            return Err(MainStoreError::InvalidPrefs("early warning"));
        }
    }
    if let Some(warned) = &prefs.budget_warned {
        if !valid_day(&warned.day) {
            return Err(MainStoreError::InvalidPrefs("warned day"));
        }
    }
    if let Some(day) = &prefs.morning_offered_day {
        if !valid_day(day) {
            return Err(MainStoreError::InvalidPrefs("offered day"));
        }
    }
    Ok(())
}

/// A recently opened session (`recentlyOpened.ts`, `RecentSession`): its program, folder, title
/// and marker, and the conversation id the agent CLI resumes (`None` for programs without one).
/// `closed_at` is when it was last used, in milliseconds since the Unix epoch; 0 means unknown
/// (an entry taken over from the recent folders, `UNKNOWN_TIME`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClosedSession {
    pub profile_id: String,
    pub cwd: String,
    pub title: String,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub emoji: Option<String>,
    #[serde(default)]
    pub cli_session_id: Option<String>,
    pub closed_at: u64,
    /// The model the session started with (ADR 0022); left out when there is none, so the file
    /// still reads in an older Hangar.
    /// An invalid value is dropped on reading, the entry stays ([`crate::start_model::lenient_stored`]).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::start_model::lenient_stored"
    )]
    pub model: Option<crate::start_model::StartModel>,
}

/// The bounds the window keeps (`parseRecentSessions`): not too many entries, a program ID like
/// the last program's, a folder like the menu's, a title as the backend stores session titles, a
/// marker as the workspace keeps it, and a conversation id that is a lowercase UUID used once:
/// two entries with one id would resume one conversation twice.
pub fn validate_recently_closed(entries: &[ClosedSession]) -> Result<(), MainStoreError> {
    if entries.len() > MAX_RECENTLY_CLOSED {
        return Err(MainStoreError::InvalidClosed("too many sessions"));
    }
    let mut ids = BTreeSet::new();
    for entry in entries {
        if entry.profile_id.trim().is_empty()
            || has_control(&entry.profile_id)
            || entry.profile_id.encode_utf16().count() > MAX_PROFILE_ID_UNITS
        {
            return Err(MainStoreError::InvalidClosed("program"));
        }
        if !valid_folder(&entry.cwd) {
            return Err(MainStoreError::InvalidClosed("folder"));
        }
        if !valid_label(&entry.title, crate::manager::MAX_TITLE_CHARS) {
            return Err(MainStoreError::InvalidClosed("title"));
        }
        if entry
            .color
            .as_deref()
            .is_some_and(|color| !crate::persist::is_hex_color(color))
        {
            return Err(MainStoreError::InvalidClosed("color"));
        }
        if entry.emoji.as_deref().is_some_and(|emoji| {
            emoji.is_empty()
                || emoji.len() > crate::persist::MAX_EMOJI_BYTES
                || emoji.chars().any(char::is_control)
        }) {
            return Err(MainStoreError::InvalidClosed("emoji"));
        }
        if let Some(id) = &entry.cli_session_id {
            if normalize_key(id).ok().as_deref() != Some(id.as_str()) || !ids.insert(id.as_str()) {
                return Err(MainStoreError::InvalidClosed("conversation id"));
            }
        }
        if entry.closed_at > MAX_CLOSED_AT {
            return Err(MainStoreError::InvalidClosed("closed at"));
        }
    }
    Ok(())
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> MainStoreError + '_ {
    move |source| MainStoreError::Io {
        path: path.to_owned(),
        source,
    }
}

/// `None` when the file does not exist. A file that is too large, no regular file, cannot be
/// parsed or fails the checks counts as stored, with the safe defaults: switches on, no
/// messages (`Some(None)`).
fn load<T: serde::de::DeserializeOwned>(path: &Path, max: usize) -> Option<Option<T>> {
    match read_limited(path, max) {
        Ok(Some(bytes)) => Some(serde_json::from_slice(&bytes).ok()),
        Ok(None) => None,
        Err(_) => Some(None),
    }
}

fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), MainStoreError> {
    let json = serde_json::to_vec(value)
        .map_err(|error| io_error(path)(io::Error::new(io::ErrorKind::InvalidData, error)))?;
    write_atomic(path, &json).map_err(io_error(path))
}

/// The files in the local app data folder. Cheap to clone for blocking threads.
#[derive(Debug, Clone)]
pub struct MainStore {
    dir: PathBuf,
}

impl MainStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Where Hangar notes the bundled sub-agents it placed ([`crate::bundled_agents`]).
    pub fn bundled_manifest(&self) -> PathBuf {
        self.dir.join(crate::bundled_agents::MANIFEST_FILE)
    }

    /// `None`: never stored. A damaged file gives the defaults (everything on).
    pub fn load_switches(&self) -> Option<SecuritySwitches> {
        let path = self.dir.join(SWITCHES_FILE);
        load(&path, MAX_SWITCHES_BYTES).map(Option::unwrap_or_default)
    }

    pub fn save_switches(&self, switches: SecuritySwitches) -> Result<(), MainStoreError> {
        save(&self.dir.join(SWITCHES_FILE), &switches)
    }

    /// `None`: never stored. A damaged file, or one that fails the checks, gives no messages.
    pub fn load_queue(&self) -> Option<QueuedMessages> {
        let path = self.dir.join(QUEUE_FILE);
        load::<QueuedMessages>(&path, MAX_QUEUE_BYTES).map(|entries| {
            entries
                .and_then(|entries| validate_queue(entries).ok())
                .unwrap_or_default()
        })
    }

    pub fn save_queue(&self, entries: QueuedMessages) -> Result<(), MainStoreError> {
        let entries = validate_queue(entries)?;
        save(&self.dir.join(QUEUE_FILE), &entries)
    }

    /// `None`: never stored. A damaged file, or one that fails the checks, gives the defaults
    /// (warning, updates and HANGAR.md on, no snippets).
    pub fn load_settings(&self) -> Option<ProtectedSettings> {
        let path = self.dir.join(SETTINGS_FILE);
        load::<ProtectedSettings>(&path, MAX_SETTINGS_BYTES).map(|settings| {
            settings
                .filter(|settings| validate_settings(settings).is_ok())
                .unwrap_or_default()
        })
    }

    /// The sub-agent settings; a missing, damaged or invalid file gives the defaults (no model
    /// set, every program gets the user's definitions).
    pub fn load_sub_agents(&self) -> SubAgentSettings {
        let path = self.dir.join(SUB_AGENTS_FILE);
        load::<SubAgentSettings>(&path, MAX_SUB_AGENTS_BYTES)
            .flatten()
            .map(drop_retired_efforts)
            .and_then(|settings| validate_sub_agents(settings).ok())
            .unwrap_or_default()
    }

    pub fn save_sub_agents(&self, settings: SubAgentSettings) -> Result<(), MainStoreError> {
        let settings = validate_sub_agents(settings)?;
        save(&self.dir.join(SUB_AGENTS_FILE), &settings)
    }

    pub fn save_settings(&self, settings: ProtectedSettings) -> Result<(), MainStoreError> {
        validate_settings(&settings)?;
        save(&self.dir.join(SETTINGS_FILE), &settings)
    }

    /// `None`: never stored. A damaged file, or one that fails the checks, gives no templates.
    pub fn load_templates(&self) -> Option<Vec<WorkspaceTemplate>> {
        let path = self.dir.join(TEMPLATES_FILE);
        load::<Vec<WorkspaceTemplate>>(&path, MAX_TEMPLATES_BYTES).map(|templates| {
            templates
                .filter(|templates| validate_templates(templates).is_ok())
                .unwrap_or_default()
        })
    }

    pub fn save_templates(&self, templates: Vec<WorkspaceTemplate>) -> Result<(), MainStoreError> {
        validate_templates(&templates)?;
        save(&self.dir.join(TEMPLATES_FILE), &templates)
    }

    /// `None`: never stored. A damaged file, or one that fails the checks, gives the defaults
    /// (private start, no remembered program or folders, restore and every notice on).
    pub fn load_prefs(&self) -> Option<MainPrefs> {
        let path = self.dir.join(PREFS_FILE);
        load::<MainPrefs>(&path, MAX_PREFS_BYTES).map(|prefs| {
            prefs
                .filter(|prefs| validate_prefs(prefs).is_ok())
                .unwrap_or_default()
        })
    }

    pub fn save_prefs(&self, prefs: MainPrefs) -> Result<(), MainStoreError> {
        validate_prefs(&prefs)?;
        save(&self.dir.join(PREFS_FILE), &prefs)
    }

    /// The recently opened sessions, most recent first. A missing, damaged or invalid file gives
    /// none, so a planted file starts nothing.
    pub fn load_recently_closed(&self) -> Vec<ClosedSession> {
        let path = self.dir.join(RECENTLY_CLOSED_FILE);
        load::<Vec<ClosedSession>>(&path, MAX_RECENTLY_CLOSED_BYTES)
            .flatten()
            .filter(|entries| validate_recently_closed(entries).is_ok())
            .unwrap_or_default()
    }

    pub fn save_recently_closed(&self, entries: Vec<ClosedSession>) -> Result<(), MainStoreError> {
        validate_recently_closed(&entries)?;
        save(&self.dir.join(RECENTLY_CLOSED_FILE), &entries)
    }

    /// The partner projects; a missing, damaged or invalid file gives none, so a planted file
    /// can link no project to another.
    pub fn load_partners(&self) -> PartnerBook {
        let path = self.dir.join(PARTNERS_FILE);
        load::<PartnerBook>(&path, MAX_PARTNERS_BYTES)
            .flatten()
            .filter(|book| book.validate().is_ok())
            .unwrap_or_default()
    }

    pub fn save_partners(&self, book: &PartnerBook) -> Result<(), MainStoreError> {
        book.validate()?;
        save(&self.dir.join(PARTNERS_FILE), book)
    }

    /// The messages agents sent that wait for the person (ADR 0012). A missing file is an empty
    /// inbox; a damaged one, too large or failing the checks, too, said on stderr: a planted
    /// file must not put a text before the person that no agent sent through the checks.
    pub fn load_inbox(&self) -> Inbox {
        let path = self.dir.join(INBOX_FILE);
        match read_limited(&path, crate::partner_inbox::MAX_FILE_BYTES) {
            Ok(None) => Inbox::default(),
            Ok(Some(bytes)) => match serde_json::from_slice::<Inbox>(&bytes) {
                Ok(inbox) => match inbox.validate() {
                    Ok(()) => inbox,
                    Err(reason) => {
                        eprintln!("hangar: ignoring {}: {reason}", path.display());
                        Inbox::default()
                    }
                },
                Err(error) => {
                    eprintln!("hangar: ignoring {}: {error}", path.display());
                    Inbox::default()
                }
            },
            Err(reason) => {
                eprintln!("hangar: ignoring {}: {reason}", path.display());
                Inbox::default()
            }
        }
    }

    pub fn save_inbox(&self, inbox: &Inbox) -> Result<(), MainStoreError> {
        save(&self.dir.join(INBOX_FILE), inbox)
    }

    /// The remembered link groups (ADR 0014). A missing, damaged or invalid file gives none:
    /// they are only ever an offer the person confirms, but a planted file should not even
    /// offer anything.
    pub fn load_link_groups(&self) -> Vec<crate::link_memory::RememberedGroup> {
        let path = self.dir.join(LINK_GROUPS_FILE);
        load::<crate::link_memory::MemoryFile>(&path, crate::link_memory::MAX_FILE_BYTES)
            .flatten()
            .filter(|file| crate::link_memory::validate(file).is_ok())
            .map(|file| file.groups)
            .unwrap_or_default()
    }

    pub fn save_link_groups(
        &self,
        groups: Vec<crate::link_memory::RememberedGroup>,
    ) -> Result<(), MainStoreError> {
        let file = crate::link_memory::MemoryFile {
            version: crate::link_memory::VERSION,
            groups,
        };
        crate::link_memory::validate(&file)?;
        save(&self.dir.join(LINK_GROUPS_FILE), &file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    const KEY: &str = "0F8FAD5B-D9CB-469F-A165-70867728950E";

    #[test]
    fn only_the_main_window_may_call() {
        assert!(check_main("main").is_ok());
        for other in ["panel", "file-viewer-1", "Main", ""] {
            assert!(
                matches!(check_main(other), Err(MainStoreError::WrongWindow)),
                "{other}"
            );
        }
    }

    #[test]
    fn switches_round_trip_and_start_unset() {
        let tmp = TempDir::new("main-store-switches");
        let store = MainStore::new(tmp.path().to_owned());
        assert_eq!(store.load_switches(), None);
        let off = SecuritySwitches {
            claims_enabled: true,
            gate_enabled: false,
            command_guard_enabled: false,
        };
        store.save_switches(off).unwrap();
        assert_eq!(store.load_switches(), Some(off));
        // Damaged or foreign content: stored, but everything on.
        std::fs::write(tmp.path().join(SWITCHES_FILE), r#"{"gateEnabled":"no"}"#).unwrap();
        assert_eq!(store.load_switches(), Some(SecuritySwitches::default()));
        let json = serde_json::to_value(off).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "claimsEnabled": true, "gateEnabled": false,
                "commandGuardEnabled": false })
        );
    }

    #[test]
    fn queued_messages_are_checked_both_ways() {
        let tmp = TempDir::new("main-store-queue");
        let store = MainStore::new(tmp.path().to_owned());
        assert_eq!(store.load_queue(), None);
        let entries = QueuedMessages::from([(KEY.to_owned(), "run the tests".to_owned())]);
        store.save_queue(entries).unwrap();
        assert_eq!(
            store.load_queue(),
            Some(QueuedMessages::from([(
                KEY.to_ascii_lowercase(),
                "run the tests".to_owned()
            )]))
        );
        for (key, text) in [
            ("../escape", "x"),
            (KEY, "   "),
            (KEY, &"y".repeat(MAX_QUEUED_UNITS + 1)),
        ] {
            let bad = QueuedMessages::from([(key.to_owned(), text.to_owned())]);
            assert!(store.save_queue(bad).is_err(), "{key}");
        }
        // Exactly at the limit, counted in UTF-16 units like the window (one emoji is two).
        let emoji = "\u{1F600}".repeat(MAX_QUEUED_UNITS / 2);
        assert!(validate_queue(QueuedMessages::from([(KEY.to_owned(), emoji)])).is_ok());
        let many: QueuedMessages = (0..=MAX_QUEUED)
            .map(|i| (format!("00000000-0000-0000-0000-{i:012}"), "x".to_owned()))
            .collect();
        assert!(validate_queue(many).is_err());
        // A file someone else wrote with a bad key gives no messages at all.
        std::fs::write(tmp.path().join(QUEUE_FILE), r#"{"../x":"HANGAR-MARKER"}"#).unwrap();
        assert_eq!(store.load_queue(), Some(QueuedMessages::new()));
    }

    fn snippet(id: &str, text: &str) -> Snippet {
        Snippet {
            id: id.to_owned(),
            text: text.to_owned(),
        }
    }

    #[test]
    fn sub_agent_settings_are_checked_and_default_to_nothing() {
        let tmp = TempDir::new("main-store-sub-agents");
        let store = MainStore::new(tmp.path().to_owned());
        assert_eq!(store.load_sub_agents(), SubAgentSettings::default());
        let settings = SubAgentSettings {
            models: BTreeMap::from([
                (
                    DefProgram::Claude,
                    ModelChoice {
                        model: Some("sonnet".into()),
                        effort: None,
                    },
                ),
                (DefProgram::Codex, ModelChoice::default()),
            ]),
            provide_off: BTreeSet::from([DefProgram::Codex]),
            bundled_off: true,
        };
        store.save_sub_agents(settings).unwrap();
        let loaded = store.load_sub_agents();
        // The empty choice is dropped.
        assert_eq!(loaded.models.len(), 1);
        assert!(loaded.provide_off.contains(&DefProgram::Codex));
        assert_eq!(
            serde_json::to_value(&loaded).unwrap(),
            serde_json::json!({ "models": { "claude": { "model": "sonnet", "effort": null } },
                "provideOff": ["codex"], "bundledOff": true })
        );
        let bad = SubAgentSettings {
            models: BTreeMap::from([(
                DefProgram::Claude,
                ModelChoice {
                    model: Some("x\" y".into()),
                    effort: None,
                },
            )]),
            provide_off: BTreeSet::new(),
            bundled_off: false,
        };
        assert!(store.save_sub_agents(bad).is_err());
        // A file with a value no check lets through gives the defaults.
        std::fs::write(
            tmp.path().join(SUB_AGENTS_FILE),
            r#"{"models":{"codex":{"model":"a b"}},"provideOff":[]}"#,
        )
        .unwrap();
        assert_eq!(store.load_sub_agents(), SubAgentSettings::default());
    }

    #[test]
    fn an_effort_no_longer_offered_is_dropped_on_load() {
        let tmp = TempDir::new("main-store-sub-agents-effort");
        let store = MainStore::new(tmp.path().to_owned());
        // Hangar offered `minimal` before 0.171.0 was measured; no model takes it.
        std::fs::write(
            tmp.path().join(SUB_AGENTS_FILE),
            r#"{"models":{"codex":{"model":"gpt-6.1-sol","effort":"minimal"}},"provideOff":["codex"],"bundledOff":true}"#,
        )
        .unwrap();
        let loaded = store.load_sub_agents();
        assert_eq!(
            loaded.models.get(&DefProgram::Codex),
            Some(&ModelChoice {
                model: Some("gpt-6.1-sol".into()),
                effort: None,
            })
        );
        assert!(loaded.provide_off.contains(&DefProgram::Codex));
        assert!(loaded.bundled_off);
        // A choice that held nothing but that effort is gone; the rest still loads.
        std::fs::write(
            tmp.path().join(SUB_AGENTS_FILE),
            r#"{"models":{"codex":{"effort":"minimal"}},"provideOff":["codex"]}"#,
        )
        .unwrap();
        let loaded = store.load_sub_agents();
        assert!(loaded.models.is_empty());
        assert!(loaded.provide_off.contains(&DefProgram::Codex));
        // Only what Hangar once offered is forgiven: any other refused value, or an effort for
        // Claude Code, still gives the defaults, as before.
        for models in [
            r#"{"codex":{"model":"gpt-6.1-sol","effort":"turbo"}}"#,
            r#"{"codex":{"model":"gpt-6.1-sol","effort":"ultra"}}"#,
            r#"{"claude":{"model":"sonnet","effort":"minimal"}}"#,
            r#"{"codex":{"model":"a b","effort":"minimal"}}"#,
        ] {
            std::fs::write(
                tmp.path().join(SUB_AGENTS_FILE),
                format!(r#"{{"models":{models},"provideOff":["codex"],"bundledOff":true}}"#),
            )
            .unwrap();
            assert_eq!(
                store.load_sub_agents(),
                SubAgentSettings::default(),
                "{models}"
            );
        }
    }

    #[test]
    fn partner_projects_round_trip_and_a_bad_file_links_nothing() {
        let tmp = TempDir::new("main-store-partners");
        let store = MainStore::new(tmp.path().to_owned());
        assert_eq!(store.load_partners(), PartnerBook::default());
        let book = PartnerBook::default()
            .link(
                crate::partners::ProjectEnd {
                    folder: "/work/app",
                    name: "app",
                },
                crate::partners::ProjectEnd {
                    folder: "/work/web",
                    name: "web",
                },
            )
            .unwrap();
        store.save_partners(&book).unwrap();
        assert_eq!(store.load_partners(), book);
        let mut bad = book.clone();
        bad.pairs.push(["app".into(), "app".into()]);
        assert!(store.save_partners(&bad).is_err());
        std::fs::write(
            tmp.path().join(PARTNERS_FILE),
            r#"{"projects":[{"name":"x","folder":"/a"}],"pairs":[["x","y"]]}"#,
        )
        .unwrap();
        assert_eq!(store.load_partners(), PartnerBook::default());
    }

    #[test]
    fn protected_settings_round_trip_and_start_unset() {
        let tmp = TempDir::new("main-store-settings");
        let store = MainStore::new(tmp.path().to_owned());
        assert_eq!(store.load_settings(), None);
        let settings = ProtectedSettings {
            warn_multiline_paste: false,
            auto_update: false,
            hangar_md: false,
            snippets: vec![snippet("default-1", "Review the diff")],
            status_line: false,
        };
        store.save_settings(settings.clone()).unwrap();
        assert_eq!(store.load_settings(), Some(settings.clone()));
        assert_eq!(
            serde_json::to_value(&settings).unwrap(),
            serde_json::json!({ "warnMultilinePaste": false, "autoUpdate": false,
                "hangarMd": false, "snippets": [{ "id": "default-1", "text": "Review the diff" }],
                "statusLine": false })
        );
        // A file from before the status line: on.
        std::fs::write(
            tmp.path().join(SETTINGS_FILE),
            r#"{"warnMultilinePaste":false,"autoUpdate":true,"hangarMd":true,"snippets":[]}"#,
        )
        .unwrap();
        let older = store.load_settings().unwrap();
        assert!(older.status_line);
        assert!(!older.warn_multiline_paste);
        // Damaged or foreign content: stored, but the safe defaults.
        for foreign in [
            r#"{"warnMultilinePaste":"no"}"#,
            r#"{"warnMultilinePaste":false,"autoUpdate":false,"hangarMd":false,"snippets":[],"x":1}"#,
            r#"{"warnMultilinePaste":false,"autoUpdate":false,"hangarMd":false,"snippets":[{"id":"../x","text":"HANGAR-MARKER"}]}"#,
        ] {
            std::fs::write(tmp.path().join(SETTINGS_FILE), foreign).unwrap();
            assert_eq!(
                store.load_settings(),
                Some(ProtectedSettings::default()),
                "{foreign}"
            );
        }
        assert!(ProtectedSettings::default().warn_multiline_paste);
        assert!(ProtectedSettings::default().auto_update);
        assert!(ProtectedSettings::default().status_line);
    }

    #[test]
    fn snippets_are_checked_like_the_window_checks_them() {
        let with = |snippets: Vec<Snippet>| ProtectedSettings {
            snippets,
            ..ProtectedSettings::default()
        };
        assert!(validate_settings(&with(vec![snippet("snippet-1", "x")])).is_ok());
        // Exactly at the limit, counted in UTF-16 units like the window.
        let emoji = "\u{1F600}".repeat(MAX_SNIPPET_UNITS / 2);
        assert!(validate_settings(&with(vec![snippet("a", &emoji)])).is_ok());
        for bad in [
            vec![snippet("", "x")],
            vec![snippet("Upper", "x")],
            vec![snippet(&"a".repeat(MAX_ID_LEN + 1), "x")],
            vec![snippet("a", "x"), snippet("a", "y")],
            vec![snippet("a", " \n ")],
            vec![snippet("a", &"y".repeat(MAX_SNIPPET_UNITS + 1))],
            (0..=MAX_SNIPPETS)
                .map(|i| snippet(&format!("s-{i}"), "x"))
                .collect(),
        ] {
            assert!(validate_settings(&with(bad.clone())).is_err(), "{bad:?}");
        }
    }

    fn session(folder: &str) -> TemplateSession {
        TemplateSession {
            profile_id: "claude".to_owned(),
            folder: folder.to_owned(),
            mode: TemplateMode::Private,
            title: None,
            member_name: None,
            role: None,
        }
    }

    fn template(id: &str, name: &str, sessions: Vec<TemplateSession>) -> WorkspaceTemplate {
        WorkspaceTemplate {
            id: id.to_owned(),
            name: name.to_owned(),
            sessions,
            link: false,
            ask_first: false,
        }
    }

    fn member(folder: &str, name: &str, role: TemplateRole) -> TemplateSession {
        TemplateSession {
            member_name: Some(name.to_owned()),
            role: Some(role),
            ..session(folder)
        }
    }

    #[test]
    fn templates_without_a_link_group_still_load_and_new_ones_keep_theirs() {
        // An older template: no link fields at all.
        let old: Vec<WorkspaceTemplate> = serde_json::from_str(
            r#"[{"id":"t-1","name":"Old","sessions":[{"profileId":"claude","folder":"/a","mode":"private"}]}]"#,
        )
        .unwrap();
        assert!(validate_templates(&old).is_ok());
        assert!(!old[0].link && !old[0].ask_first);
        assert_eq!(old[0].sessions[0].member_name, None);
        // Saved again, it stays as it was.
        assert_eq!(
            serde_json::to_value(&old).unwrap(),
            serde_json::json!([{ "id": "t-1", "name": "Old", "sessions": [
                { "profileId": "claude", "folder": "/a", "mode": "private" }] }])
        );
        let mut linked = template(
            "t-2",
            "Crew",
            vec![
                member("/a", "boss", TemplateRole::Lead),
                member("/b", "web", TemplateRole::Member),
            ],
        );
        linked.link = true;
        linked.ask_first = true;
        let json = serde_json::to_value(&linked).unwrap();
        assert_eq!(json["link"], true);
        assert_eq!(json["askFirst"], true);
        assert_eq!(json["sessions"][0]["memberName"], "boss");
        assert_eq!(json["sessions"][0]["role"], "lead");
        let back: WorkspaceTemplate = serde_json::from_value(json).unwrap();
        assert_eq!(back, linked);
        assert!(validate_templates(std::slice::from_ref(&linked)).is_ok());
        let tmp = TempDir::new("main-store-linked-templates");
        let store = MainStore::new(tmp.path().to_owned());
        store.save_templates(vec![linked.clone()]).unwrap();
        assert_eq!(store.load_templates(), Some(vec![linked.clone()]));
        // Two leads, a reserved, invalid or repeated name: refused.
        for sessions in [
            vec![
                member("/a", "a", TemplateRole::Lead),
                member("/b", "b", TemplateRole::Lead),
            ],
            vec![member("/a", "lead", TemplateRole::Member)],
            vec![member("/a", "all", TemplateRole::Member)],
            vec![member("/a", "Web Site", TemplateRole::Member)],
            vec![
                member("/a", "web", TemplateRole::Member),
                member("/b", "web", TemplateRole::Member),
            ],
        ] {
            let bad = template("t-3", "Bad", sessions);
            assert!(
                validate_templates(std::slice::from_ref(&bad)).is_err(),
                "{bad:?}"
            );
        }
        let mut wrong_role = serde_json::to_value(&linked).unwrap();
        wrong_role["sessions"][0]["role"] = serde_json::json!("boss");
        assert!(serde_json::from_value::<WorkspaceTemplate>(wrong_role).is_err());
    }

    #[test]
    fn templates_round_trip_and_start_unset() {
        let tmp = TempDir::new("main-store-templates");
        let store = MainStore::new(tmp.path().to_owned());
        assert_eq!(store.load_templates(), None);
        let mut team = session("C:\\Work\\Repo");
        team.mode = TemplateMode::Team;
        team.title = Some("Review".to_owned());
        let templates = vec![template("template-1", "Morning", vec![team])];
        store.save_templates(templates.clone()).unwrap();
        assert_eq!(store.load_templates(), Some(templates.clone()));
        assert_eq!(
            serde_json::to_value(&templates).unwrap(),
            serde_json::json!([{ "id": "template-1", "name": "Morning", "sessions": [
                { "profileId": "claude", "folder": "C:\\Work\\Repo", "mode": "team",
                  "title": "Review" }] }])
        );
        // An empty list is stored too: the migration ran.
        store.save_templates(Vec::new()).unwrap();
        assert_eq!(store.load_templates(), Some(Vec::new()));
        // A file someone else wrote that fails the checks gives no templates at all.
        std::fs::write(
            tmp.path().join(TEMPLATES_FILE),
            r#"[{"id":"t","name":"x","sessions":[{"profileId":"p","folder":"","mode":"private"}]}]"#,
        )
        .unwrap();
        assert_eq!(store.load_templates(), Some(Vec::new()));
    }

    #[test]
    fn templates_are_checked_like_the_window_checks_them() {
        let ok = template("t-1", "Work", vec![session("/home/a/repo")]);
        assert!(validate_templates(std::slice::from_ref(&ok)).is_ok());
        let mut bad_mode = serde_json::to_value(&ok).unwrap();
        bad_mode["sessions"][0]["mode"] = serde_json::json!("root");
        assert!(serde_json::from_value::<WorkspaceTemplate>(bad_mode).is_err());
        let mut titled = session("/x");
        titled.title = Some("a\u{7}b".to_owned());
        let mut no_program = session("/x");
        no_program.profile_id = " ".to_owned();
        for bad in [
            vec![template("T", "Work", vec![session("/x")])],
            vec![template("t", " ", vec![session("/x")])],
            vec![template(
                "t",
                &"n".repeat(MAX_TEMPLATE_NAME_CHARS + 1),
                vec![session("/x")],
            )],
            vec![template("t", "Work", Vec::new())],
            vec![template(
                "t",
                "Work",
                vec![session("/x"); MAX_TEMPLATE_SESSIONS + 1],
            )],
            vec![template("t", "Work", vec![session("")])],
            vec![template("t", "Work", vec![session("/a\nb")])],
            vec![template(
                "t",
                "Work",
                vec![session(&"f".repeat(MAX_FOLDER_UNITS + 1))],
            )],
            vec![template("t", "Work", vec![titled.clone()])],
            vec![template("t", "Work", vec![no_program.clone()])],
            vec![ok.clone(), template("t-1", "Other", vec![session("/x")])],
            (0..=MAX_TEMPLATES)
                .map(|i| template(&format!("t-{i}"), &format!("n{i}"), vec![session("/x")]))
                .collect(),
        ] {
            assert!(validate_templates(&bad).is_err(), "{bad:?}");
        }
    }

    fn prefs() -> MainPrefs {
        MainPrefs {
            start_mode: StartMode::Team,
            last_program: Some("claude".to_owned()),
            favorites: vec!["C:\\Work\\Repo".to_owned()],
            recent: vec!["/home/a/repo".to_owned(), "C:\\Work\\Repo".to_owned()],
            restore_sessions: false,
            attention_enabled: false,
            taskbar_flash: false,
            notify_desktop: false,
            chime: false,
            quiet_hours: QuietHours {
                enabled: true,
                from: 0,
                to: 1439,
            },
            daily_budget: Some(12.5),
            budget_early_percent: Some(70),
            budget_warned: Some(BudgetWarned {
                day: "2026-11-14".to_owned(),
                level: BudgetLevel::Early,
            }),
            morning_review: false,
            morning_offered_day: Some("2026-11-13".to_owned()),
            dictation_enabled: Some(true),
        }
    }

    #[test]
    fn main_prefs_round_trip_and_start_unset() {
        let tmp = TempDir::new("main-store-prefs");
        let store = MainStore::new(tmp.path().to_owned());
        assert_eq!(store.load_prefs(), None);
        store.save_prefs(prefs()).unwrap();
        assert_eq!(store.load_prefs(), Some(prefs()));
        assert_eq!(
            serde_json::to_value(prefs()).unwrap(),
            serde_json::json!({ "startMode": "team", "lastProgram": "claude",
                "favorites": ["C:\\Work\\Repo"], "recent": ["/home/a/repo", "C:\\Work\\Repo"],
                "restoreSessions": false, "attentionEnabled": false, "taskbarFlash": false,
                "notifyDesktop": false, "chime": false,
                "quietHours": { "enabled": true, "from": 0, "to": 1439 },
                "dailyBudget": 12.5, "budgetEarlyPercent": 70,
                "budgetWarned": { "day": "2026-11-14", "level": "early" },
                "morningReview": false, "morningOfferedDay": "2026-11-13",
                "dictationEnabled": true })
        );
        // A file from before dictation still loads, with the choice left open for the migration.
        let older = r#"{"startMode":"team","lastProgram":null,"favorites":[],"recent":[],"restoreSessions":false,"attentionEnabled":true,"taskbarFlash":true,"notifyDesktop":true,"chime":true,"quietHours":{"enabled":false,"from":0,"to":0}}"#;
        std::fs::write(tmp.path().join(PREFS_FILE), older).unwrap();
        let loaded = store.load_prefs().unwrap();
        assert!(!loaded.restore_sessions);
        assert_eq!(loaded.dictation_enabled, None);
        // The defaults are stored too: the migration ran.
        store.save_prefs(MainPrefs::default()).unwrap();
        assert_eq!(store.load_prefs(), Some(MainPrefs::default()));
        let defaults = MainPrefs::default();
        assert_eq!(defaults.start_mode, StartMode::Private);
        assert!(defaults.restore_sessions && defaults.attention_enabled && defaults.chime);
        assert!(defaults.taskbar_flash && defaults.notify_desktop);
        assert_eq!(
            (defaults.daily_budget, defaults.budget_warned),
            (None, None)
        );
        assert_eq!(defaults.budget_early_percent, Some(DEFAULT_EARLY_PERCENT));
        assert!(defaults.morning_review);
        assert_eq!(defaults.morning_offered_day, None);
        // A file written before the budget existed keeps its preferences.
        std::fs::write(
            tmp.path().join(PREFS_FILE),
            r#"{"startMode":"team","lastProgram":null,"favorites":[],"recent":[],"restoreSessions":false,"attentionEnabled":true,"taskbarFlash":true,"notifyDesktop":true,"chime":true,"quietHours":{"enabled":false,"from":0,"to":0}}"#,
        )
        .unwrap();
        let older = store.load_prefs().unwrap();
        assert_eq!(older.start_mode, StartMode::Team);
        assert!(!older.restore_sessions);
        assert_eq!(older.daily_budget, None);
        // It gets the early warning and the morning summary with their defaults.
        assert_eq!(older.budget_early_percent, Some(DEFAULT_EARLY_PERCENT));
        assert!(older.morning_review);
        assert_eq!(
            defaults.dictation_enabled,
            Some(false),
            "dictation stays off"
        );
        // Damaged or foreign content: stored, but the defaults.
        for foreign in [
            r#"{"startMode":"root"}"#,
            r#"{"startMode":"team","lastProgram":null,"favorites":[],"recent":["/a\nb"],"restoreSessions":true,"attentionEnabled":true,"taskbarFlash":true,"notifyDesktop":true,"chime":true,"quietHours":{"enabled":false,"from":0,"to":0}}"#,
            r#"{"startMode":"team","lastProgram":null,"favorites":[],"recent":[],"restoreSessions":true,"attentionEnabled":true,"taskbarFlash":true,"notifyDesktop":true,"chime":true,"quietHours":{"enabled":false,"from":0,"to":0},"x":1}"#,
        ] {
            std::fs::write(tmp.path().join(PREFS_FILE), foreign).unwrap();
            assert_eq!(store.load_prefs(), Some(MainPrefs::default()), "{foreign}");
        }
    }

    #[test]
    fn main_prefs_are_checked_like_the_window_checks_them() {
        assert!(validate_prefs(&prefs()).is_ok());
        let at_limit = MainPrefs {
            recent: vec!["/r".to_owned(); MAX_RECENT_FOLDERS],
            favorites: vec!["f".repeat(MAX_FOLDER_UNITS); MAX_FAVORITE_FOLDERS],
            last_program: Some("p".repeat(MAX_PROFILE_ID_UNITS)),
            daily_budget: Some(MAX_BUDGET_USD),
            budget_early_percent: Some(99),
            budget_warned: Some(BudgetWarned {
                day: "9999-12-31".to_owned(),
                level: BudgetLevel::Reached,
            }),
            morning_offered_day: Some("9999-12-31".to_owned()),
            ..prefs()
        };
        assert!(validate_prefs(&at_limit).is_ok());
        let one_cent = MainPrefs {
            daily_budget: Some(MIN_BUDGET_USD),
            ..prefs()
        };
        assert!(validate_prefs(&one_cent).is_ok());
        let unset = MainPrefs {
            daily_budget: None,
            budget_early_percent: None,
            budget_warned: None,
            morning_offered_day: None,
            ..prefs()
        };
        assert!(validate_prefs(&unset).is_ok());
        for budget in [
            0.0,
            0.001,
            -1.0,
            MAX_BUDGET_USD + 0.01,
            f64::NAN,
            f64::INFINITY,
        ] {
            let prefs = MainPrefs {
                daily_budget: Some(budget),
                ..prefs()
            };
            assert!(validate_prefs(&prefs).is_err(), "{budget}");
        }
        for day in [
            "",
            "2026-11-1",
            "2026-11-145",
            "2026/11/14",
            "2026-00-14",
            "2026-13-14",
            "2026-11-00",
            "2026-11-32",
            "２０２６-11-14",
            "2026-1a-14",
            "2026-11-14\n",
            " 2026-11-14",
        ] {
            let warned = MainPrefs {
                budget_warned: Some(BudgetWarned {
                    day: day.to_owned(),
                    level: BudgetLevel::Reached,
                }),
                ..prefs()
            };
            assert!(validate_prefs(&warned).is_err(), "{day:?}");
            let offered = MainPrefs {
                morning_offered_day: Some(day.to_owned()),
                ..prefs()
            };
            assert!(validate_prefs(&offered).is_err(), "{day:?}");
        }
        for percent in [0, 100, 255] {
            let prefs = MainPrefs {
                budget_early_percent: Some(percent),
                ..prefs()
            };
            assert!(validate_prefs(&prefs).is_err(), "{percent}");
        }
        let bad = [
            MainPrefs {
                last_program: Some(" ".to_owned()),
                ..prefs()
            },
            MainPrefs {
                last_program: Some("a\u{7}b".to_owned()),
                ..prefs()
            },
            MainPrefs {
                last_program: Some("p".repeat(MAX_PROFILE_ID_UNITS + 1)),
                ..prefs()
            },
            MainPrefs {
                recent: vec!["/r".to_owned(); MAX_RECENT_FOLDERS + 1],
                ..prefs()
            },
            MainPrefs {
                favorites: vec!["/f".to_owned(); MAX_FAVORITE_FOLDERS + 1],
                ..prefs()
            },
            MainPrefs {
                recent: vec![String::new()],
                ..prefs()
            },
            MainPrefs {
                favorites: vec!["/a\tb".to_owned()],
                ..prefs()
            },
            MainPrefs {
                recent: vec!["f".repeat(MAX_FOLDER_UNITS + 1)],
                ..prefs()
            },
            MainPrefs {
                quiet_hours: QuietHours {
                    enabled: true,
                    from: MINUTES_PER_DAY,
                    to: 0,
                },
                ..prefs()
            },
            MainPrefs {
                quiet_hours: QuietHours {
                    enabled: true,
                    from: 0,
                    to: MINUTES_PER_DAY,
                },
                ..prefs()
            },
        ];
        for prefs in bad {
            assert!(validate_prefs(&prefs).is_err(), "{prefs:?}");
        }
        let mut team = serde_json::to_value(MainPrefs::default()).unwrap();
        team["startMode"] = serde_json::json!("Team");
        assert!(serde_json::from_value::<MainPrefs>(team).is_err());
    }

    #[test]
    fn an_old_warned_day_becomes_the_reached_level_of_that_day() {
        let tmp = TempDir::new("main-store-warned");
        let store = MainStore::new(tmp.path().to_owned());
        let base = r#""startMode":"private","lastProgram":null,"favorites":[],"recent":[],"restoreSessions":true,"attentionEnabled":true,"taskbarFlash":true,"notifyDesktop":true,"chime":true,"quietHours":{"enabled":false,"from":0,"to":0},"dailyBudget":5"#;
        let write = |field: &str| {
            std::fs::write(tmp.path().join(PREFS_FILE), format!("{{{base},{field}}}")).unwrap();
        };
        write(r#""budgetWarnedDay":"2026-11-19""#);
        let loaded = store.load_prefs().unwrap();
        assert_eq!(
            loaded.budget_warned,
            Some(BudgetWarned {
                day: "2026-11-19".to_owned(),
                level: BudgetLevel::Reached,
            })
        );
        assert_eq!(loaded.daily_budget, Some(5.0));
        // Saved again, it is written in the new form only.
        store.save_prefs(loaded.clone()).unwrap();
        let text = std::fs::read_to_string(tmp.path().join(PREFS_FILE)).unwrap();
        assert!(text.contains(r#""budgetWarned":{"day":"2026-11-19","level":"reached"}"#));
        assert!(!text.contains("budgetWarnedDay"));
        assert_eq!(store.load_prefs(), Some(loaded));
        // The new form under the old name, and an explicit null, load too.
        for (field, expected) in [
            (
                r#""budgetWarnedDay":{"day":"2026-11-20","level":"early"}"#,
                Some(BudgetLevel::Early),
            ),
            (r#""budgetWarnedDay":null"#, None),
            (r#""budgetWarned":null"#, None),
        ] {
            write(field);
            let loaded = store.load_prefs().unwrap();
            assert_eq!(loaded.budget_warned.map(|w| w.level), expected, "{field}");
            assert_eq!(loaded.daily_budget, Some(5.0), "{field}");
        }
        // An explicit null turns the early warning off; missing gives the default.
        write(r#""budgetEarlyPercent":null"#);
        assert_eq!(store.load_prefs().unwrap().budget_early_percent, None);
        // Broken or doubled values: the file counts as damaged and gives the defaults.
        for damaged in [
            r#""budgetWarnedDay":"2026-13-01""#,
            r#""budgetWarned":{"day":"2026-11-20","level":"over"}"#,
            r#""budgetWarned":{"day":"2026-11-20","level":"early","x":1}"#,
            r#""budgetWarned":{"day":"2026-11-20"}"#,
            r#""budgetWarned":7"#,
            r#""budgetWarnedDay":"2026-11-19","budgetWarned":{"day":"2026-11-20","level":"early"}"#,
            r#""morningOfferedDay":"yesterday""#,
            r#""budgetEarlyPercent":100"#,
        ] {
            write(damaged);
            assert_eq!(store.load_prefs(), Some(MainPrefs::default()), "{damaged}");
        }
    }

    #[test]
    fn waiting_messages_survive_a_restart_but_not_a_damaged_file() {
        let tmp = TempDir::new("main-store-inbox");
        let store = MainStore::new(tmp.path().to_owned());
        assert_eq!(store.load_inbox(), Inbox::default());
        let mut inbox = Inbox::default();
        inbox
            .push("app", "web", "show the text".into(), 5, Some(1))
            .unwrap();
        store.save_inbox(&inbox).unwrap();
        let loaded = store.load_inbox();
        assert_eq!(loaded.messages().len(), 1);
        assert_eq!(loaded.messages()[0].text, "show the text");
        // The session id belongs to the run that is over.
        assert_eq!(loaded.messages()[0].sender, None);

        let path = tmp.path().join(INBOX_FILE);
        for damaged in [
            b"{".to_vec(),
            br#"{"nextId":1,"messages":[{"id":1,"from":"app","to":"web","text":"a\u001b[201~","at":1}]}"#
                .to_vec(),
            vec![b' '; crate::partner_inbox::MAX_FILE_BYTES + 1],
        ] {
            std::fs::write(&path, damaged).unwrap();
            assert_eq!(store.load_inbox(), Inbox::default());
        }
    }

    fn closed(cwd: &str, id: Option<&str>) -> ClosedSession {
        ClosedSession {
            profile_id: "claude".into(),
            cwd: cwd.into(),
            title: "Claude Code".into(),
            color: Some("#4a8fe7".into()),
            emoji: Some("\u{1F680}".into()),
            cli_session_id: id.map(str::to_owned),
            closed_at: 1_700_000_000_000,
            model: None,
        }
    }

    #[test]
    fn a_recent_session_keeps_its_model() {
        use crate::start_model::{ModelSource, StartModel};
        let tmp = TempDir::new("main-store-closed-model");
        let store = MainStore::new(tmp.path().to_owned());
        let entries = vec![ClosedSession {
            model: Some(StartModel {
                source: ModelSource::Provider,
                id: "opus".into(),
                effort: None,
            }),
            ..closed(r"C:\work\app", None)
        }];
        store.save_recently_closed(entries.clone()).unwrap();
        assert_eq!(store.load_recently_closed(), entries);
        // An invalid model in the file costs only the model, not the entry or the list.
        let path = tmp.path().join(RECENTLY_CLOSED_FILE);
        for bad in [
            serde_json::json!({ "source": "lmstudio", "id": "a b" }),
            serde_json::json!({ "source": "cloud", "id": "x" }),
            serde_json::json!(7),
        ] {
            let mut json: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            json[0]["model"] = bad;
            std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
            let loaded = store.load_recently_closed();
            assert_eq!(loaded.len(), 1);
            assert_eq!(loaded[0].model, None);
            assert_eq!(loaded[0].cwd, entries[0].cwd);
        }
    }

    #[test]
    fn recently_closed_sessions_round_trip_and_start_empty() {
        let tmp = TempDir::new("main-store-closed");
        let store = MainStore::new(tmp.path().to_owned());
        assert_eq!(store.load_recently_closed(), Vec::new());
        let entries = vec![
            closed(r"C:\work\app", Some("0f8fad5b-d9cb-469f-a165-70867728950e")),
            ClosedSession {
                profile_id: "pwsh".into(),
                color: None,
                emoji: None,
                ..closed("/home/me/web", None)
            },
        ];
        store.save_recently_closed(entries.clone()).unwrap();
        assert_eq!(store.load_recently_closed(), entries);
        // The window reads camelCase; a file without the optional fields still loads.
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(tmp.path().join(RECENTLY_CLOSED_FILE)).unwrap())
                .unwrap();
        assert_eq!(
            json[0]["cliSessionId"],
            "0f8fad5b-d9cb-469f-a165-70867728950e"
        );
        assert_eq!(json[0]["closedAt"], 1_700_000_000_000_u64);
        std::fs::write(
            tmp.path().join(RECENTLY_CLOSED_FILE),
            r#"[{"profileId":"cmd","cwd":"C:\\","title":"cmd","closedAt":1}]"#,
        )
        .unwrap();
        assert_eq!(store.load_recently_closed().len(), 1);
        store.save_recently_closed(Vec::new()).unwrap();
        assert_eq!(store.load_recently_closed(), Vec::new());
    }

    #[test]
    fn recently_closed_sessions_are_checked_like_the_window_checks_them() {
        let ok = closed("/srv/app", Some("0f8fad5b-d9cb-469f-a165-70867728950e"));
        assert!(validate_recently_closed(std::slice::from_ref(&ok)).is_ok());
        let bad = [
            ClosedSession {
                profile_id: " ".into(),
                ..ok.clone()
            },
            ClosedSession {
                profile_id: "a\u{7}".into(),
                ..ok.clone()
            },
            ClosedSession {
                profile_id: "p".repeat(MAX_PROFILE_ID_UNITS + 1),
                ..ok.clone()
            },
            ClosedSession {
                cwd: String::new(),
                ..ok.clone()
            },
            ClosedSession {
                cwd: "/srv/a\nb".into(),
                ..ok.clone()
            },
            ClosedSession {
                cwd: "a".repeat(MAX_FOLDER_UNITS + 1),
                ..ok.clone()
            },
            ClosedSession {
                title: "  ".into(),
                ..ok.clone()
            },
            ClosedSession {
                title: "a\u{1b}[2J".into(),
                ..ok.clone()
            },
            ClosedSession {
                title: "t".repeat(crate::manager::MAX_TITLE_CHARS + 1),
                ..ok.clone()
            },
            ClosedSession {
                color: Some("red".into()),
                ..ok.clone()
            },
            ClosedSession {
                emoji: Some(String::new()),
                ..ok.clone()
            },
            ClosedSession {
                emoji: Some("\u{1F600}".repeat(5)),
                ..ok.clone()
            },
            ClosedSession {
                cli_session_id: Some("not-a-uuid".into()),
                ..ok.clone()
            },
            // The window writes ids in lower case; another spelling is no id it wrote.
            ClosedSession {
                cli_session_id: Some(KEY.into()),
                ..ok.clone()
            },
            ClosedSession {
                closed_at: MAX_CLOSED_AT + 1,
                ..ok.clone()
            },
        ];
        for entry in bad {
            assert!(
                validate_recently_closed(std::slice::from_ref(&entry)).is_err(),
                "{entry:?}"
            );
        }
        // One conversation resumed by two entries would run twice.
        let twice = [
            ok.clone(),
            closed("/srv/other", ok.cli_session_id.as_deref()),
        ];
        assert!(validate_recently_closed(&twice).is_err());
        let many: Vec<ClosedSession> = (0..=MAX_RECENTLY_CLOSED)
            .map(|index| closed(&format!("/srv/{index}"), None))
            .collect();
        assert!(validate_recently_closed(&many).is_err());
        assert!(validate_recently_closed(&many[..MAX_RECENTLY_CLOSED]).is_ok());

        let tmp = TempDir::new("main-store-closed-bad");
        let store = MainStore::new(tmp.path().to_owned());
        assert!(store.save_recently_closed(many).is_err());
        assert_eq!(store.load_recently_closed(), Vec::new());
        // A planted file with one bad entry starts nothing at all.
        for planted in [
            r#"[{"profileId":"pwsh","cwd":"","title":"x","closedAt":1}]"#.to_owned(),
            r#"[{"profileId":"pwsh","cwd":"/x","title":"x","closedAt":1,"args":["-c"]}]"#
                .to_owned(),
            "{".to_owned(),
            " ".repeat(MAX_RECENTLY_CLOSED_BYTES + 1),
        ] {
            std::fs::write(tmp.path().join(RECENTLY_CLOSED_FILE), planted).unwrap();
            assert_eq!(store.load_recently_closed(), Vec::new());
        }
    }
}
