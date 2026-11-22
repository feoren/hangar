//! Anonymous usage statistics (ADR 0019): the counters of one UTC day, the rules for when a day's
//! report is due, and the report itself. Off until the person says yes; nothing here counts while
//! it is off, and turning it off drops what was counted. The report carries allowlisted values
//! only ([`crate::telemetry_catalog`]): no id, no path, no command, no text the person typed.
//!
//! Days: the counters belong to the UTC day they were made on. On the first count, tick or start
//! of a later day, the previous day's counters become the pending report when that day is
//! yesterday and was not reported yet; anything older is dropped. One report per day at most,
//! a failed send is tried again only while its day is still yesterday, a few times.
//!
//! Sending and storing live in [`crate::telemetry_service`].

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::civil;
use crate::session::SessionMode;
use crate::telemetry_catalog::{self as catalog, Feature};

/// In the local app data folder, next to the other state of the main window.
pub const STATE_FILE: &str = "usage-statistics.json";
/// A debug build's own file: a dev instance running next to the installed Hangar never takes over
/// or overwrites its answer and counters.
pub const DEV_STATE_FILE: &str = "usage-statistics.dev.json";

/// The state file of this build.
pub fn state_file() -> &'static str {
    if cfg!(debug_assertions) {
        DEV_STATE_FILE
    } else {
        STATE_FILE
    }
}
/// Far above a real file: two maps of at most a few dozen keys and one snapshot of settings.
pub const MAX_STATE_BYTES: usize = 32 * 1024;
/// Sends of one day's report at most; only network trouble and server errors count as worth
/// another try.
pub const MAX_ATTEMPTS: u8 = 3;
/// Time between two tries of the same report.
pub const RETRY_MS: u64 = 15 * 60 * 1000;
const DAY_MS: u64 = 86_400_000;

/// A UTC calendar day, as days since 1970-01-01; written as `YYYY-MM-DD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Day(i64);

impl Day {
    pub fn from_unix_ms(ms: u64) -> Self {
        Self(i64::try_from(ms / DAY_MS).unwrap_or(i64::MAX))
    }

    pub fn previous(self) -> Self {
        Self(self.0.saturating_sub(1))
    }

    /// `YYYY-MM-DD` of a real date, nothing else.
    pub fn parse(text: &str) -> Option<Self> {
        let bytes = text.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return None;
        }
        let number = |range: std::ops::Range<usize>| -> Option<u32> {
            let part = text.get(range)?;
            part.bytes()
                .all(|b| b.is_ascii_digit())
                .then(|| part.parse().ok())?
        };
        let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
        if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
            return None;
        }
        let days = civil::days_from_civil(i64::from(year), month, day);
        // 2026-02-30 would land on a day in March.
        (civil::civil_from_days(days) == (i64::from(year), month, day)).then_some(Self(days))
    }

    pub fn text(self) -> String {
        let (year, month, day) = civil::civil_from_days(self.0);
        format!("{year:04}-{month:02}-{day:02}")
    }
}

impl Serialize for Day {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.text())
    }
}

impl<'de> Deserialize<'de> for Day {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom("invalid day"))
    }
}

/// The settings as the window sends them: the app's own keys. Every value is compared with a
/// list and never kept as given.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsInput {
    pub language: String,
    pub theme: String,
    pub terminal_scheme: String,
    pub terminal_font: String,
    pub terminal_font_size: u32,
    pub cursor_style: String,
    pub viewer_theme: String,
    pub dot_animation: String,
    pub attention: bool,
    pub restore_sessions: bool,
    pub side_panel_detached: bool,
    pub sidebar_collapsed: bool,
}

/// The `settings` of a report: schema values only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub theme: String,
    pub terminal_scheme: String,
    pub terminal_font: String,
    /// Left out when outside the schema's range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_font_size: Option<u32>,
    pub cursor_style: String,
    pub viewer_theme: String,
    pub dot_animation: String,
    pub attention: bool,
    pub restore_sessions: bool,
    pub side_panel_detached: bool,
    pub sidebar_collapsed: bool,
}

/// The terminal font size in steps, so an unusual size does not make one installation stand
/// out; `None` outside the schema's range. Every step is itself a valid size.
pub fn font_size(size: u32) -> Option<u32> {
    if !(catalog::FONT_SIZE_MIN..=catalog::FONT_SIZE_MAX).contains(&size) {
        return None;
    }
    Some(match size {
        ..=11 => 10,
        12..=14 => 13,
        15..=18 => 16,
        _ => 20,
    })
}

/// The number of shared projects in steps, for the same reason.
pub fn shared_projects(count: u32) -> u32 {
    match count {
        0 => 0,
        1 => 1,
        2..=3 => 2,
        4..=10 => 5,
        _ => 10,
    }
}

/// Which window may count which feature through `telemetry_count`: the panel only the commit
/// history it shows, the main window everything the backend does not count itself.
pub fn window_may_count(panel: bool, feature: Feature) -> bool {
    if panel {
        feature == Feature::CommitHistoryOpened
    } else {
        !matches!(
            feature,
            Feature::NotesCreated | Feature::FolderRevealed | Feature::FileViewerOpened
        )
    }
}

impl Settings {
    pub fn from_input(input: &SettingsInput) -> Self {
        Self {
            theme: catalog::pick(&input.theme, catalog::THEMES).to_owned(),
            terminal_scheme: catalog::map(&input.terminal_scheme, catalog::TERMINAL_SCHEMES)
                .to_owned(),
            terminal_font: catalog::pick(&input.terminal_font, catalog::TERMINAL_FONTS).to_owned(),
            terminal_font_size: font_size(input.terminal_font_size),
            cursor_style: catalog::pick(&input.cursor_style, catalog::CURSOR_STYLES).to_owned(),
            viewer_theme: catalog::map(&input.viewer_theme, catalog::VIEWER_THEMES).to_owned(),
            dot_animation: catalog::pick(&input.dot_animation, catalog::DOT_ANIMATIONS).to_owned(),
            attention: input.attention,
            restore_sessions: input.restore_sessions,
            side_panel_detached: input.side_panel_detached,
            sidebar_collapsed: input.sidebar_collapsed,
        }
    }

    /// A snapshot read back from the file, every value checked again: a changed file sends
    /// nothing outside the lists either.
    fn checked(self) -> Self {
        Self {
            theme: catalog::pick(&self.theme, catalog::THEMES).to_owned(),
            terminal_scheme: catalog::wire(&self.terminal_scheme, catalog::TERMINAL_SCHEMES)
                .to_owned(),
            terminal_font: catalog::pick(&self.terminal_font, catalog::TERMINAL_FONTS).to_owned(),
            terminal_font_size: self.terminal_font_size.and_then(font_size),
            cursor_style: catalog::pick(&self.cursor_style, catalog::CURSOR_STYLES).to_owned(),
            viewer_theme: catalog::wire(&self.viewer_theme, catalog::VIEWER_THEMES).to_owned(),
            dot_animation: catalog::pick(&self.dot_animation, catalog::DOT_ANIMATIONS).to_owned(),
            ..self
        }
    }
}

/// What one day counted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Usage {
    /// Program key ([`catalog::is_program`]) to new sessions the person started.
    pub sessions_started: BTreeMap<String, u32>,
    /// Program key to minutes its sessions ran while Hangar ran.
    pub session_minutes: BTreeMap<String, u32>,
    pub private_sessions: u32,
    pub team_sessions: u32,
    pub features: BTreeMap<Feature, u32>,
}

fn bump(value: &mut u32) {
    *value = value.saturating_add(1).min(catalog::MAX_COUNT);
}

fn checked_programs(map: BTreeMap<String, u32>) -> BTreeMap<String, u32> {
    map.into_iter()
        .filter(|(key, _)| catalog::is_program(key))
        .map(|(key, value)| (key, value.min(catalog::MAX_COUNT)))
        .collect()
}

impl Usage {
    fn checked(self) -> Self {
        Self {
            sessions_started: checked_programs(self.sessions_started),
            session_minutes: checked_programs(self.session_minutes),
            private_sessions: self.private_sessions.min(catalog::MAX_COUNT),
            team_sessions: self.team_sessions.min(catalog::MAX_COUNT),
            features: self
                .features
                .into_iter()
                .map(|(feature, value)| (feature, value.min(catalog::MAX_COUNT)))
                .collect(),
        }
    }
}

/// A finished day waiting to be sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pending {
    pub date: Day,
    pub usage: Usage,
    pub settings: Option<Settings>,
    pub language: Option<String>,
    pub attempts: u8,
    /// Unix ms before which the next try waits.
    pub not_before_ms: u64,
}

/// Everything kept between runs. A missing or damaged file starts over: not asked yet, nothing
/// counted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct State {
    /// `None`: not asked yet; the window asks once.
    pub consent: Option<bool>,
    /// The first report of this installation went out; later reports say `firstReport: false`,
    /// also after the statistics were turned off and on again.
    pub first_report_done: bool,
    /// The day of the last report sent or given up; never sent twice.
    pub last_sent: Option<Day>,
    /// The UTC day [`Self::usage`] belongs to.
    pub day: Option<Day>,
    pub usage: Usage,
    /// The latest snapshot the window sent.
    pub settings: Option<Settings>,
    /// The latest UI language, a value of [`catalog::LANGUAGES`].
    pub language: Option<String>,
    pub pending: Option<Pending>,
}

impl State {
    /// The file's content, or a fresh start when it is damaged; every value checked again.
    pub fn from_bytes(bytes: &[u8]) -> Self {
        serde_json::from_slice::<Self>(bytes)
            .map(Self::checked)
            .unwrap_or_default()
    }

    fn checked(self) -> Self {
        let language = |value: Option<String>| {
            value
                .map(|language| catalog::pick(&language, catalog::LANGUAGES))
                .filter(|language| *language != catalog::OTHER)
                .map(str::to_owned)
        };
        Self {
            usage: self.usage.checked(),
            settings: self.settings.map(Settings::checked),
            language: language(self.language),
            pending: self.pending.map(|pending| Pending {
                usage: pending.usage.checked(),
                settings: pending.settings.map(Settings::checked),
                language: language(pending.language),
                attempts: pending.attempts.min(MAX_ATTEMPTS),
                ..pending
            }),
            ..self
        }
    }

    pub fn enabled(&self) -> bool {
        self.consent == Some(true)
    }

    /// The person's answer, from the question or the switch. Off drops everything counted and
    /// the settings snapshot; on starts counting today.
    pub fn set_consent(&mut self, enabled: bool) {
        if enabled == self.enabled() && self.consent.is_some() {
            return;
        }
        self.consent = Some(enabled);
        self.day = None;
        self.usage = Usage::default();
        self.pending = None;
        if !enabled {
            self.settings = None;
            self.language = None;
        }
    }

    /// Moves the counters to `today`. The previous day's become the pending report when that day
    /// is yesterday and was not reported; older days and a pending report that is no longer
    /// yesterday's are dropped.
    pub fn roll(&mut self, today: Day) {
        if !self.enabled() {
            return;
        }
        let yesterday = today.previous();
        if self.pending.as_ref().is_some_and(|p| p.date != yesterday) {
            self.pending = None;
        }
        if self.day == Some(today) {
            return;
        }
        let usage = std::mem::take(&mut self.usage);
        if self.day == Some(yesterday) && self.last_sent != Some(yesterday) {
            self.pending = Some(Pending {
                date: yesterday,
                usage,
                settings: self.settings.clone(),
                language: self.language.clone(),
                attempts: 0,
                not_before_ms: 0,
            });
        }
        self.day = Some(today);
    }

    /// A new session the person started (not a restored one).
    pub fn session_started(&mut self, today: Day, program: &'static str, mode: SessionMode) {
        if !self.enabled() {
            return;
        }
        self.roll(today);
        bump(
            self.usage
                .sessions_started
                .entry(program.to_owned())
                .or_default(),
        );
        match mode {
            SessionMode::Private => bump(&mut self.usage.private_sessions),
            SessionMode::Team => bump(&mut self.usage.team_sessions),
        }
    }

    /// One minute for each running session, by its program key.
    pub fn minute(&mut self, today: Day, running: &[&'static str]) {
        if !self.enabled() {
            return;
        }
        self.roll(today);
        for program in running {
            bump(
                self.usage
                    .session_minutes
                    .entry((*program).to_owned())
                    .or_default(),
            );
        }
    }

    pub fn feature(&mut self, today: Day, feature: Feature) {
        if !self.enabled() {
            return;
        }
        self.roll(today);
        bump(self.usage.features.entry(feature).or_default());
    }

    pub fn set_settings(&mut self, today: Day, input: &SettingsInput) {
        if !self.enabled() {
            return;
        }
        self.roll(today);
        self.settings = Some(Settings::from_input(input));
        let language = catalog::pick(&input.language, catalog::LANGUAGES);
        self.language = (language != catalog::OTHER).then(|| language.to_owned());
    }

    /// The report to send now: yesterday's, once its next try is due.
    pub fn due(&self, today: Day, now_ms: u64) -> Option<&Pending> {
        self.pending
            .as_ref()
            .filter(|p| self.enabled() && p.date == today.previous() && now_ms >= p.not_before_ms)
    }

    /// The pending report of `date` arrived.
    pub fn sent(&mut self, date: Day) {
        if self.pending.as_ref().is_some_and(|p| p.date == date) {
            self.pending = None;
            self.last_sent = Some(date);
            self.first_report_done = true;
        }
    }

    /// The pending report of `date` did not arrive. Network trouble and server errors wait for
    /// another try; a refused report, or the last try, gives the day up.
    pub fn failed(&mut self, date: Day, transient: bool, now_ms: u64) {
        let Some(pending) = self.pending.as_mut().filter(|p| p.date == date) else {
            return;
        };
        pending.attempts = pending.attempts.saturating_add(1);
        if transient && pending.attempts < MAX_ATTEMPTS {
            pending.not_before_ms = now_ms.saturating_add(RETRY_MS);
        } else {
            self.give_up(date);
        }
    }

    /// The day is never reported (a debug build, a report too large).
    pub fn give_up(&mut self, date: Day) {
        if self.pending.as_ref().is_some_and(|p| p.date == date) {
            self.pending = None;
            self.last_sent = Some(date);
        }
    }
}

/// What a report says about this machine; fixed for the run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Machine {
    pub os: &'static str,
    /// Windows `10` or `11`, the macOS major version, `linux`; `None` when unknown.
    pub os_release: Option<String>,
    pub arch: Option<&'static str>,
}

/// The release a report may name for `os`, from the Windows build number or the macOS version.
pub fn os_release(os: &str, raw: Option<&str>) -> Option<String> {
    match os {
        "windows" => {
            let build: u32 = raw?.trim().parse().ok()?;
            // Windows 11 kept major version 10; its builds start at 22000.
            Some(if build >= 22_000 { "11" } else { "10" }.to_owned())
        }
        "macos" => {
            let major: u32 = raw?.trim().split('.').next()?.parse().ok()?;
            (catalog::MACOS_RELEASE_MIN..=catalog::MACOS_RELEASE_MAX)
                .contains(&major)
                .then(|| major.to_string())
        }
        "linux" => Some("linux".to_owned()),
        _ => None,
    }
}

/// `std::env::consts::OS` as the schema names it.
pub fn os_name(os: &str) -> &'static str {
    match os {
        "windows" => "windows",
        "macos" => "macos",
        "linux" => "linux",
        _ => catalog::OTHER,
    }
}

/// `std::env::consts::ARCH` as the schema names it.
pub fn arch_name(arch: &str) -> Option<&'static str> {
    match arch {
        "x86_64" => Some("x64"),
        "aarch64" => Some("arm64"),
        _ => None,
    }
}

/// `ProductVersion` of macOS' `SystemVersion.plist`.
pub fn plist_product_version(text: &str) -> Option<&str> {
    let after = text.split("<key>ProductVersion</key>").nth(1)?;
    let start = after.find("<string>")? + "<string>".len();
    let end = after[start..].find("</string>")? + start;
    Some(after[start..end].trim())
}

/// `X.Y.Z` of the package version without its build number, and the channel: beta while the
/// major version is 0, as the window shows it.
pub fn app_version(package: &str) -> (String, &'static str) {
    let number = package.split(['+', '-']).next().unwrap_or_default();
    let parts: Vec<&str> = number.split('.').collect();
    let valid = parts.len() == 3
        && parts
            .iter()
            .all(|part| (1..=3).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit()));
    let channel = if parts.first() == Some(&"0") {
        "beta"
    } else {
        "stable"
    };
    if valid {
        (number.to_owned(), channel)
    } else {
        (catalog::OTHER.to_owned(), channel)
    }
}

/// The file name of a program without folder and extension, in lower case.
pub fn command_stem(command: &str) -> String {
    let name = command.rsplit(['/', '\\']).next().unwrap_or(command);
    let lower = name.trim().to_lowercase();
    for extension in [".exe", ".cmd", ".bat", ".com", ".ps1"] {
        if let Some(stem) = lower.strip_suffix(extension) {
            return stem.to_owned();
        }
    }
    lower
}

/// The program key of a session: a shell by its program, an agent by its catalog id, a program
/// of the person's own as `custom`, anything else as `other`. Never the command itself.
pub fn program_key(command: &str, catalog_id: Option<&str>, own_profile: bool) -> &'static str {
    let stem = command_stem(command);
    if let Some((_, key)) = catalog::SHELLS.iter().find(|(shell, _)| *shell == stem) {
        return key;
    }
    if let Some(id) = catalog_id {
        return catalog::pick(id, catalog::AGENTS);
    }
    if own_profile {
        catalog::CUSTOM
    } else {
        catalog::OTHER
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPart {
    pub version: String,
    pub channel: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemPart {
    pub os: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub os_release: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arch: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsagePart {
    pub sessions_started: BTreeMap<String, u32>,
    pub session_minutes: BTreeMap<String, u32>,
    pub private_sessions: u32,
    pub team_sessions: u32,
    /// All eight, unused ones with 0.
    pub features: BTreeMap<Feature, u32>,
}

/// Whether the person is signed in to a Hangar account and how many projects share with the team.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Team {
    pub signed_in: bool,
    pub shared_projects: u32,
}

/// One daily report, schema 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub schema: u32,
    pub date: Day,
    pub first_report: bool,
    pub app: AppPart,
    pub system: SystemPart,
    /// Left out when the window never sent a snapshot that day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings: Option<Settings>,
    pub usage: UsagePart,
    pub team: Team,
}

pub fn build_report(
    pending: &Pending,
    first_report: bool,
    package_version: &str,
    machine: &Machine,
    team: Team,
) -> Report {
    let (version, channel) = app_version(package_version);
    let usage = pending.usage.clone().checked();
    Report {
        schema: catalog::SCHEMA,
        date: pending.date,
        first_report,
        app: AppPart { version, channel },
        system: SystemPart {
            os: machine.os,
            os_release: machine.os_release.clone(),
            arch: machine.arch,
            language: pending.language.clone(),
        },
        settings: pending.settings.clone().map(Settings::checked),
        usage: UsagePart {
            sessions_started: usage.sessions_started,
            session_minutes: usage.session_minutes,
            private_sessions: usage.private_sessions,
            team_sessions: usage.team_sessions,
            features: Feature::ALL
                .iter()
                .map(|feature| (*feature, usage.features.get(feature).copied().unwrap_or(0)))
                .collect(),
        },
        team: Team {
            signed_in: team.signed_in,
            shared_projects: shared_projects(team.shared_projects)
                .min(catalog::MAX_SHARED_PROJECTS),
        },
    }
}

/// The request body; `None` when it would exceed what the server takes.
pub fn encode(report: &Report) -> Option<Vec<u8>> {
    serde_json::to_vec(report)
        .ok()
        .filter(|body| body.len() <= catalog::MAX_REPORT_BYTES)
}

/// The stored state of `path`: missing, unreadable or damaged starts over.
pub fn load(path: &Path) -> State {
    match crate::persist::read_limited(path, MAX_STATE_BYTES) {
        Ok(Some(bytes)) => State::from_bytes(&bytes),
        Ok(None) => State::default(),
        Err(reason) => {
            eprintln!("hangar: usage statistics start over ({reason})");
            State::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = DAY_MS;

    fn day(text: &str) -> Day {
        Day::parse(text).unwrap()
    }

    fn input() -> SettingsInput {
        SettingsInput {
            language: "de".to_owned(),
            theme: "system".to_owned(),
            terminal_scheme: "meridian".to_owned(),
            terminal_font: "JetBrains Mono".to_owned(),
            terminal_font_size: 14,
            cursor_style: "block".to_owned(),
            viewer_theme: "harbor".to_owned(),
            dot_animation: "bounce".to_owned(),
            attention: true,
            restore_sessions: true,
            side_panel_detached: false,
            sidebar_collapsed: false,
        }
    }

    fn enabled() -> State {
        let mut state = State::default();
        state.set_consent(true);
        state
    }

    fn machine() -> Machine {
        Machine {
            os: "windows",
            os_release: Some("11".to_owned()),
            arch: Some("x64"),
        }
    }

    #[test]
    fn days_are_utc_calendar_days() {
        assert_eq!(Day::from_unix_ms(0).text(), "1970-01-01");
        // 2026-11-12 00:00:00 UTC and one millisecond before.
        let midnight = 1_794_441_600_000;
        assert_eq!(Day::from_unix_ms(midnight).text(), "2026-11-12");
        assert_eq!(Day::from_unix_ms(midnight - 1).text(), "2026-11-11");
        assert_eq!(day("2026-03-01").previous().text(), "2026-02-28");
        for bad in [
            "2026-02-30",
            "2026-13-01",
            "26-11-12",
            "2026-1-12",
            "2026-11-1x",
            "",
        ] {
            assert_eq!(Day::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn nothing_counts_before_or_without_consent() {
        let today = day("2026-11-12");
        for consent in [None, Some(false)] {
            let mut state = State {
                consent,
                ..State::default()
            };
            state.session_started(today, "claude", SessionMode::Private);
            state.minute(today, &["claude"]);
            state.feature(today, Feature::NotesCreated);
            state.set_settings(today, &input());
            state.roll(today.previous().previous());
            assert_eq!(state.usage, Usage::default());
            assert_eq!(state.settings, None);
            assert_eq!(state.day, None);
            assert_eq!(state.due(today, u64::MAX), None);
        }
    }

    #[test]
    fn turning_off_drops_counters_settings_and_pending_report() {
        let today = day("2026-11-12");
        let mut state = enabled();
        state.session_started(today.previous(), "codex", SessionMode::Team);
        state.set_settings(today.previous(), &input());
        state.feature(today, Feature::BroadcastSent);
        assert!(state.pending.is_some());
        state.set_consent(false);
        assert_eq!(state.consent, Some(false));
        assert_eq!(state.usage, Usage::default());
        assert_eq!(state.pending, None);
        assert_eq!(state.settings, None);
        assert_eq!(state.language, None);
        assert_eq!(state.day, None);
        // On again: counting starts afresh.
        state.set_consent(true);
        assert_eq!(state.usage, Usage::default());
    }

    #[test]
    fn counters_and_caps() {
        let today = day("2026-11-12");
        let mut state = enabled();
        state.session_started(today, "claude", SessionMode::Private);
        state.session_started(today, "claude", SessionMode::Team);
        state.session_started(today, "shell:pwsh", SessionMode::Private);
        state.minute(today, &["claude", "claude", "shell:pwsh"]);
        state.feature(today, Feature::PathMentioned);
        for _ in 0..catalog::MAX_COUNT + 5 {
            state.feature(today, Feature::FileViewerOpened);
        }
        let usage = &state.usage;
        assert_eq!(usage.sessions_started["claude"], 2);
        assert_eq!(usage.sessions_started["shell:pwsh"], 1);
        assert_eq!(usage.session_minutes["claude"], 2);
        assert_eq!(usage.private_sessions, 2);
        assert_eq!(usage.team_sessions, 1);
        assert_eq!(usage.features[&Feature::PathMentioned], 1);
        assert_eq!(
            usage.features[&Feature::FileViewerOpened],
            catalog::MAX_COUNT
        );
    }

    #[test]
    fn yesterday_becomes_the_report_and_older_days_are_dropped() {
        let today = day("2026-11-12");
        let mut state = enabled();
        state.session_started(today.previous(), "claude", SessionMode::Private);
        // First count of a new day while running: yesterday is due.
        state.feature(today, Feature::NotesCreated);
        let pending = state.due(today, 0).unwrap();
        assert_eq!(pending.date, today.previous());
        assert_eq!(pending.usage.sessions_started["claude"], 1);
        assert_eq!(state.usage.features[&Feature::NotesCreated], 1);
        assert!(state.usage.sessions_started.is_empty());

        // Two days later without a start: both days are gone.
        let mut stale = enabled();
        stale.session_started(today.previous().previous(), "claude", SessionMode::Private);
        stale.roll(today);
        assert_eq!(stale.pending, None);
        assert!(stale.usage.sessions_started.is_empty());

        // A pending report that was not sent by the next day is dropped as well.
        let tomorrow = Day(today.0 + 1);
        state.roll(tomorrow);
        assert_eq!(state.due(tomorrow, 0).map(|p| p.date), Some(today));
        let mut late = enabled();
        late.minute(today.previous(), &["codex"]);
        late.roll(today);
        late.roll(Day(today.0 + 2));
        assert_eq!(late.pending, None);
    }

    #[test]
    fn at_most_one_report_per_day() {
        let today = day("2026-11-12");
        let mut state = enabled();
        state.minute(today.previous(), &["claude"]);
        state.roll(today);
        state.sent(today.previous());
        assert_eq!(state.last_sent, Some(today.previous()));
        assert_eq!(state.due(today, 0), None);
        // Counting the same day again (a clock set back) never makes a second report of it.
        state.day = Some(today.previous());
        state.minute(today.previous(), &["claude"]);
        state.roll(today);
        assert_eq!(state.pending, None);
    }

    #[test]
    fn transient_failures_retry_a_few_times_then_give_up() {
        let today = day("2026-11-12");
        let mut state = enabled();
        state.minute(today.previous(), &["claude"]);
        state.roll(today);
        let date = today.previous();
        state.failed(date, true, 1_000);
        assert_eq!(state.due(today, 1_000), None);
        assert!(state.due(today, 1_000 + RETRY_MS).is_some());
        state.failed(date, true, 2_000);
        state.failed(date, true, 3_000);
        assert_eq!(state.pending, None);
        assert_eq!(state.last_sent, Some(date));

        // A refused report is not sent again.
        let mut refused = enabled();
        refused.minute(date, &["claude"]);
        refused.roll(today);
        refused.failed(date, false, 0);
        assert_eq!(refused.pending, None);
        assert!(!refused.first_report_done);
    }

    #[test]
    fn first_report_once_per_installation() {
        let today = day("2026-11-12");
        let mut state = enabled();
        state.minute(today.previous(), &["claude"]);
        state.roll(today);
        assert!(!state.first_report_done);
        state.sent(today.previous());
        assert!(state.first_report_done);
        state.set_consent(false);
        state.set_consent(true);
        assert!(state.first_report_done);
        // A day given up (debug build) is no first report.
        let mut debug = enabled();
        debug.minute(today.previous(), &["claude"]);
        debug.roll(today);
        debug.give_up(today.previous());
        assert!(!debug.first_report_done);
        assert_eq!(debug.pending, None);
    }

    #[test]
    fn settings_map_to_schema_values() {
        let settings = Settings::from_input(&input());
        assert_eq!(settings.terminal_scheme, "oneDarkPro");
        assert_eq!(settings.viewer_theme, "github");
        assert_eq!(settings.terminal_font_size, Some(13));
        let odd = SettingsInput {
            theme: "C:\\Users\\someone".to_owned(),
            terminal_scheme: "my scheme".to_owned(),
            terminal_font: "Comic Sans".to_owned(),
            terminal_font_size: 99,
            cursor_style: "blink".to_owned(),
            viewer_theme: "oneDarkPro".to_owned(),
            dot_animation: "dance".to_owned(),
            language: "xx".to_owned(),
            ..input()
        };
        let settings = Settings::from_input(&odd);
        for value in [
            &settings.theme,
            &settings.terminal_scheme,
            &settings.terminal_font,
            &settings.cursor_style,
            &settings.viewer_theme,
            &settings.dot_animation,
        ] {
            assert_eq!(value, "other");
        }
        assert_eq!(settings.terminal_font_size, None);
        let mut state = enabled();
        state.set_settings(day("2026-11-12"), &odd);
        assert_eq!(state.language, None);
    }

    #[test]
    fn a_changed_file_sends_nothing_outside_the_lists() {
        let text = r#"{
            "consent": true,
            "day": "2026-11-12",
            "usage": {
                "sessionsStarted": {"claude": 3, "/home/someone/tool": 1},
                "sessionMinutes": {"claude": 99999},
                "features": {"notesCreated": 2}
            },
            "settings": {
                "theme": "dark", "terminalScheme": "meridian", "terminalFont": "Hack",
                "terminalFontSize": 3, "cursorStyle": "bar", "viewerTheme": "github",
                "dotAnimation": "wave", "attention": true, "restoreSessions": false,
                "sidePanelDetached": false, "sidebarCollapsed": true
            },
            "language": "someone@example.org"
        }"#;
        let state = State::from_bytes(text.as_bytes());
        assert!(state.enabled());
        assert_eq!(
            state.usage.sessions_started.keys().collect::<Vec<_>>(),
            ["claude"]
        );
        assert_eq!(state.usage.session_minutes["claude"], catalog::MAX_COUNT);
        let settings = state.settings.unwrap();
        assert_eq!(settings.terminal_scheme, "other");
        assert_eq!(settings.viewer_theme, "github");
        assert_eq!(settings.terminal_font_size, None);
        assert_eq!(state.language, None);
    }

    #[test]
    fn a_damaged_file_starts_over() {
        for bad in [
            "",
            "{",
            "[]",
            r#"{"consent": "yes"}"#,
            r#"{"day": "yesterday"}"#,
        ] {
            assert_eq!(State::from_bytes(bad.as_bytes()), State::default(), "{bad}");
        }
    }

    #[test]
    fn program_keys_never_carry_the_command() {
        assert_eq!(program_key("pwsh", None, true), "shell:pwsh");
        assert_eq!(
            program_key(
                r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
                None,
                true
            ),
            "shell:powershell"
        );
        assert_eq!(
            program_key(r"C:\Windows\System32\wsl.exe", Some("wsl-ubuntu"), false),
            "shell:wsl"
        );
        assert_eq!(program_key("bash", None, true), "shell:bash");
        assert_eq!(program_key("/bin/zsh", None, true), "shell:zsh");
        assert_eq!(program_key("claude", Some("claude"), false), "claude");
        assert_eq!(program_key("claude.cmd", Some("claude"), true), "claude");
        assert_eq!(program_key("goose", Some("goose"), false), "other");
        assert_eq!(
            program_key("/opt/secret-project/run.sh", None, true),
            "custom"
        );
        assert_eq!(program_key("something", None, false), "other");
    }

    #[test]
    fn machine_values() {
        assert_eq!(os_release("windows", Some("19045")).as_deref(), Some("10"));
        assert_eq!(os_release("windows", Some("26100")).as_deref(), Some("11"));
        assert_eq!(os_release("windows", Some("x")), None);
        assert_eq!(os_release("macos", Some("14.5")).as_deref(), Some("14"));
        assert_eq!(os_release("macos", Some("10.15.7")), None);
        assert_eq!(
            os_release("linux", Some("6.8.0-generic")).as_deref(),
            Some("linux")
        );
        assert_eq!(os_release("linux", None).as_deref(), Some("linux"));
        assert_eq!(os_name("freebsd"), "other");
        assert_eq!(arch_name("x86_64"), Some("x64"));
        assert_eq!(arch_name("aarch64"), Some("arm64"));
        assert_eq!(arch_name("x86"), None);
        let plist = "<dict>\n\t<key>ProductName</key>\n\t<string>macOS</string>\n\t<key>ProductVersion</key>\n\t<string>14.5</string>\n</dict>";
        assert_eq!(plist_product_version(plist), Some("14.5"));
        assert_eq!(plist_product_version("<dict></dict>"), None);
    }

    #[test]
    fn version_without_build_and_channel() {
        assert_eq!(app_version("0.14.0+648"), ("0.14.0".to_owned(), "beta"));
        assert_eq!(app_version("1.2.3"), ("1.2.3".to_owned(), "stable"));
        assert_eq!(app_version("1.2"), ("other".to_owned(), "stable"));
        assert_eq!(app_version("0.1234.0"), ("other".to_owned(), "beta"));
    }

    /// The report has exactly the schema's fields; nothing else, no free text.
    #[test]
    fn report_follows_schema_one() {
        let today = day("2026-11-12");
        let mut state = enabled();
        let yesterday = today.previous();
        state.set_settings(yesterday, &input());
        state.session_started(yesterday, "claude", SessionMode::Private);
        state.session_started(yesterday, "shell:pwsh", SessionMode::Team);
        state.minute(yesterday, &["claude"]);
        state.feature(yesterday, Feature::CommitHistoryOpened);
        state.roll(today);
        let pending = state.due(today, 0).unwrap();
        let team = Team {
            signed_in: true,
            shared_projects: 250,
        };
        let report = build_report(pending, true, "0.14.0+648", &machine(), team);
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "schema": 1,
                "date": "2026-11-11",
                "firstReport": true,
                "app": { "version": "0.14.0", "channel": "beta" },
                "system": { "os": "windows", "osRelease": "11", "arch": "x64", "language": "de" },
                "settings": {
                    "theme": "system",
                    "terminalScheme": "oneDarkPro",
                    "terminalFont": "JetBrains Mono",
                    "terminalFontSize": 13,
                    "cursorStyle": "block",
                    "viewerTheme": "github",
                    "dotAnimation": "bounce",
                    "attention": true,
                    "restoreSessions": true,
                    "sidePanelDetached": false,
                    "sidebarCollapsed": false
                },
                "usage": {
                    "sessionsStarted": { "claude": 1, "shell:pwsh": 1 },
                    "sessionMinutes": { "claude": 1 },
                    "privateSessions": 1,
                    "teamSessions": 1,
                    "features": {
                        "fileViewerOpened": 0,
                        "commitHistoryOpened": 1,
                        "notesCreated": 0,
                        "sidePanelDetached": 0,
                        "broadcastSent": 0,
                        "pathMentioned": 0,
                        "folderRevealed": 0,
                        "updateInstalled": 0
                    }
                },
                "team": { "signedIn": true, "sharedProjects": 10 }
            })
        );
    }

    #[test]
    fn report_without_snapshot_leaves_settings_and_unknowns_out() {
        let pending = Pending {
            date: day("2026-11-11"),
            usage: Usage::default(),
            settings: None,
            language: None,
            attempts: 0,
            not_before_ms: 0,
        };
        let bare = Machine {
            os: "linux",
            os_release: Some("linux".to_owned()),
            arch: None,
        };
        let json = serde_json::to_value(build_report(
            &pending,
            false,
            "0.14.0",
            &bare,
            Team::default(),
        ))
        .unwrap();
        assert!(json.get("settings").is_none());
        assert_eq!(
            json["system"],
            serde_json::json!({"os": "linux", "osRelease": "linux"})
        );
        assert_eq!(json["usage"]["sessionsStarted"], serde_json::json!({}));
    }

    #[test]
    fn the_largest_report_stays_below_the_limit() {
        let all = |value: u32| -> BTreeMap<String, u32> {
            catalog::AGENTS
                .iter()
                .copied()
                .chain(catalog::SHELLS.iter().map(|(_, key)| *key))
                .chain([catalog::CUSTOM, catalog::OTHER])
                .map(|key| (key.to_owned(), value))
                .collect()
        };
        let pending = Pending {
            date: day("2026-11-11"),
            usage: Usage {
                sessions_started: all(u32::MAX),
                session_minutes: all(u32::MAX),
                private_sessions: u32::MAX,
                team_sessions: u32::MAX,
                features: Feature::ALL.iter().map(|f| (*f, u32::MAX)).collect(),
            },
            settings: Some(Settings::from_input(&SettingsInput {
                terminal_font: "DejaVu Sans Mono".to_owned(),
                ..input()
            })),
            language: Some("de".to_owned()),
            attempts: 0,
            not_before_ms: 0,
        };
        let report = build_report(
            &pending,
            true,
            "999.999.999+99999",
            &machine(),
            Team::default(),
        );
        assert_eq!(report.usage.sessions_started["claude"], catalog::MAX_COUNT);
        let body = encode(&report).unwrap();
        assert!(
            body.len() < catalog::MAX_REPORT_BYTES,
            "{} bytes",
            body.len()
        );
    }

    #[test]
    fn font_size_and_shared_projects_are_sent_in_steps() {
        let steps: Vec<Option<u32>> = [7, 8, 11, 12, 14, 15, 18, 19, 32, 33]
            .into_iter()
            .map(font_size)
            .collect();
        assert_eq!(
            steps,
            [
                None,
                Some(10),
                Some(10),
                Some(13),
                Some(13),
                Some(16),
                Some(16),
                Some(20),
                Some(20),
                None
            ]
        );
        // Every step is a valid size, and stepping again keeps it.
        for step in [10, 13, 16, 20] {
            assert_eq!(font_size(step), Some(step));
        }
        let projects: Vec<u32> = [0, 1, 2, 3, 4, 10, 11, 250]
            .into_iter()
            .map(shared_projects)
            .collect();
        assert_eq!(projects, [0, 1, 2, 2, 5, 5, 10, 10]);
    }

    #[test]
    fn windows_count_only_their_own_features() {
        for feature in Feature::ALL {
            assert_eq!(
                window_may_count(true, feature),
                feature == Feature::CommitHistoryOpened,
                "{feature:?}"
            );
        }
        for feature in [
            Feature::NotesCreated,
            Feature::FolderRevealed,
            Feature::FileViewerOpened,
        ] {
            assert!(!window_may_count(false, feature), "{feature:?}");
        }
        for feature in [
            Feature::CommitHistoryOpened,
            Feature::SidePanelDetached,
            Feature::BroadcastSent,
            Feature::PathMentioned,
            Feature::UpdateInstalled,
        ] {
            assert!(window_may_count(false, feature), "{feature:?}");
        }
    }

    #[test]
    fn debug_builds_keep_their_own_state_file() {
        assert_eq!(state_file() == DEV_STATE_FILE, cfg!(debug_assertions));
        assert_ne!(STATE_FILE, DEV_STATE_FILE);
    }

    #[test]
    fn days_since_epoch_stay_in_range() {
        assert_eq!(
            Day::from_unix_ms(DAY * 3).previous(),
            Day::from_unix_ms(DAY * 2)
        );
    }
}
