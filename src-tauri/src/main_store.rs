//! State only the main window may read and write: the security switches (local claims, the agent
//! gate, the command guard), the queued messages per session, the settings that decide what
//! reaches an agent or the machine ([`ProtectedSettings`]: the paste warning, automatic updates,
//! HANGAR.md for every agent, the snippets), the workspace templates (program and folder of
//! each session they start) and the start and notice preferences ([`MainPrefs`]: private or team
//! start, the last program, the favorite and recent folders, session restore, whether and when
//! a waiting session shows itself, the daily budget with the day its notice last showed, and
//! whether the dictation key is on) and the partner projects ([`crate::partners`]: which
//! projects may send each other messages).
//!
//! The panel and file viewer windows share the main window's origin and with it its
//! `localStorage`. A compromised viewer (a crafted Markdown file) could otherwise turn the gate
//! off for the next start, queue a message or plant a snippet that Hangar later pastes into an
//! agent session, switch the paste warning or updates off, plant a template that starts a
//! program in a folder of its choice, point the next start at such a folder, start it private
//! (without the gate), silence the notices of a session that waits for the person, hide the
//! budget notice (a budget out of reach, today marked as warned), or turn the dictation key on, so
//! that holding it opens the microphone. Here the files sit in the local app data folder, written
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
use crate::partners::PartnerBook;
use crate::persist::{normalize_key, read_limited, write_atomic};

pub const SWITCHES_FILE: &str = "security-switches.json";
pub const QUEUE_FILE: &str = "queued-messages.json";
pub const SETTINGS_FILE: &str = "protected-settings.json";
pub const TEMPLATES_FILE: &str = "workspace-templates.json";
pub const PREFS_FILE: &str = "main-prefs.json";
pub const SUB_AGENTS_FILE: &str = "sub-agent-settings.json";
pub const PARTNERS_FILE: &str = "partner-projects.json";
/// Every project with its longest folder in escaped JSON, the pairs by name, with room.
const MAX_PARTNERS_BYTES: usize = crate::partners::MAX_PROJECTS * (MAX_FOLDER_UNITS * 6 + 128)
    + crate::partners::MAX_PAIRS * 96
    + 1024;
/// A few programs with a short model each.
const MAX_SUB_AGENTS_BYTES: usize = 16 * 1024;
/// Queued messages kept at most, one per session ([`crate::persist::MAX_SESSIONS`]).
pub const MAX_QUEUED: usize = crate::persist::MAX_SESSIONS;
/// Length of one queued message in UTF-16 units, as the window counts (`MAX_QUEUED_CHARS`).
pub const MAX_QUEUED_UNITS: usize = 20_000;
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
    #[error(transparent)]
    Partners(#[from] crate::partners::PartnerError),
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
}

impl Default for ProtectedSettings {
    fn default() -> Self {
        Self {
            warn_multiline_paste: true,
            auto_update: true,
            hangar_md: true,
            snippets: Vec::new(),
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

/// One session of a template (`templates.ts`, `TemplateSession`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TemplateSession {
    pub profile_id: String,
    pub folder: String,
    pub mode: TemplateMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

/// A named set of sessions that starts again with one click (`templates.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceTemplate {
    pub id: String,
    pub name: String,
    pub sessions: Vec<TemplateSession>,
}

/// A name or title as the window cleans it: not blank, no control characters, not too long.
fn valid_label(text: &str, max_chars: usize) -> bool {
    !text.trim().is_empty() && !has_control(text) && text.chars().count() <= max_chars
}

/// The bounds the window keeps (`parseTemplates`): not too many templates and sessions, IDs
/// valid and unique, every session with a program and a folder. Names are not compared: the
/// window ignores case by the user's locale, which no check here could match exactly.
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
    /// The local day (`YYYY-MM-DD`) the budget notice last showed; it shows once a day.
    #[serde(default)]
    pub budget_warned_day: Option<String>,
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
            budget_warned_day: None,
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
/// hours within a day, a budget from [`MIN_BUDGET_USD`] up to [`MAX_BUDGET_USD`], a warned day
/// as a date.
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
    if let Some(day) = &prefs.budget_warned_day {
        if !valid_day(day) {
            return Err(MainStoreError::InvalidPrefs("warned day"));
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
        };
        store.save_sub_agents(settings).unwrap();
        let loaded = store.load_sub_agents();
        // The empty choice is dropped.
        assert_eq!(loaded.models.len(), 1);
        assert!(loaded.provide_off.contains(&DefProgram::Codex));
        assert_eq!(
            serde_json::to_value(&loaded).unwrap(),
            serde_json::json!({ "models": { "claude": { "model": "sonnet", "effort": null } },
                "provideOff": ["codex"] })
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
        };
        store.save_settings(settings.clone()).unwrap();
        assert_eq!(store.load_settings(), Some(settings.clone()));
        assert_eq!(
            serde_json::to_value(&settings).unwrap(),
            serde_json::json!({ "warnMultilinePaste": false, "autoUpdate": false,
                "hangarMd": false, "snippets": [{ "id": "default-1", "text": "Review the diff" }] })
        );
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
        }
    }

    fn template(id: &str, name: &str, sessions: Vec<TemplateSession>) -> WorkspaceTemplate {
        WorkspaceTemplate {
            id: id.to_owned(),
            name: name.to_owned(),
            sessions,
        }
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
            budget_warned_day: Some("2026-11-14".to_owned()),
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
                "dailyBudget": 12.5, "budgetWarnedDay": "2026-11-14", "dictationEnabled": true })
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
            (defaults.daily_budget, defaults.budget_warned_day),
            (None, None)
        );
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
            budget_warned_day: Some("9999-12-31".to_owned()),
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
            budget_warned_day: None,
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
            let prefs = MainPrefs {
                budget_warned_day: Some(day.to_owned()),
                ..prefs()
            };
            assert!(validate_prefs(&prefs).is_err(), "{day:?}");
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
}
