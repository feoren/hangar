//! The usage statistics at run time (ADR 0019): the stored state behind a lock, the counters the
//! commands report, a thread that counts session minutes and sends yesterday's report, and the
//! request itself. The rules live in [`crate::telemetry`].
//!
//! Storing: counters change in memory; the file is written only when something changed, on the
//! minute tick, at once for an answer, for an update about to be installed and when Hangar quits.
//! The answer on disk wins: a process whose file says "off" stops counting and never writes its
//! own "on" back.
//!
//! Sending: only release builds ([`SEND_ALLOWED`]), only with the person's consent in memory and
//! on disk, only to [`catalog::ENDPOINT`], with short time limits, no cookies and no redirects. A
//! failure is written to stderr with the day and the reason, never the report.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::lock;
use crate::session::SessionMode;
use crate::telemetry::{self, Day, Machine, SettingsInput, State, Team};
use crate::telemetry_catalog::{self as catalog, Feature};

/// Debug builds count and build reports but never send them, so development does not end up in
/// the statistics.
pub const SEND_ALLOWED: bool = !cfg!(debug_assertions);
/// The whole request, connecting included.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// The first look for a due report waits a little after the start: the window sends its settings
/// first, and a machine just woken may still be connecting.
pub const FIRST_CHECK: Duration = Duration::from_secs(30);
/// One minute of session time per tick.
pub const TICK: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    /// No connection came about: the server never saw the report.
    #[error("no connection")]
    Connect,
    #[error("timeout")]
    Timeout,
    #[error("network error")]
    Network,
    #[error("HTTP {0}")]
    Status(u16),
}

impl SendError {
    /// Worth another try only when the server surely did not keep the report: no connection, or
    /// an explicit "later" (429, 503). A timeout after connecting or another server error may
    /// have stored it already, and a second report of the day would count it twice.
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Connect => true,
            Self::Status(status) => *status == 429 || *status == 503,
            Self::Timeout | Self::Network => false,
        }
    }
}

/// Where a report goes; a fake in tests.
pub trait Transport: Send + Sync {
    fn post(&self, body: Vec<u8>) -> Result<(), SendError>;
}

/// `POST` to the official endpoint with reqwest.
pub struct HttpTransport {
    client: reqwest::Client,
}

impl HttpTransport {
    pub fn new() -> Result<Self, SendError> {
        crate::updates::ensure_crypto_provider();
        // reqwest keeps no cookies without its `cookies` feature, which Hangar does not enable.
        let client = reqwest::Client::builder()
            .user_agent("Hangar")
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| SendError::Network)?;
        Ok(Self { client })
    }
}

impl Transport for HttpTransport {
    fn post(&self, body: Vec<u8>) -> Result<(), SendError> {
        let request = self
            .client
            .post(catalog::ENDPOINT)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body);
        // Called from the statistics thread, never from inside the async runtime.
        tauri::async_runtime::block_on(async move {
            let response = request.send().await.map_err(|error| {
                // A connect timeout is a connect error as well: checked first.
                if error.is_connect() {
                    SendError::Connect
                } else if error.is_timeout() {
                    SendError::Timeout
                } else {
                    SendError::Network
                }
            })?;
            let status = response.status();
            // The body is never read: 204 has none, and an error text is of no use here.
            if status.is_success() {
                Ok(())
            } else {
                Err(SendError::Status(status.as_u16()))
            }
        })
    }
}

/// Stands in when no HTTP client could be built: every send fails, and the day is given up.
pub struct NoTransport;

impl Transport for NoTransport {
    fn post(&self, _body: Vec<u8>) -> Result<(), SendError> {
        Err(SendError::Network)
    }
}

/// This machine as a report names it.
pub fn detect_machine() -> Machine {
    let os = telemetry::os_name(std::env::consts::OS);
    Machine {
        os,
        os_release: telemetry::os_release(os, os_version().as_deref()),
        arch: telemetry::arch_name(std::env::consts::ARCH),
    }
}

/// The Windows build number (`CurrentBuildNumber`).
#[cfg(windows)]
fn os_version() -> Option<String> {
    use crate::install_source::registry::{open, string_value};
    use windows_sys::Win32::System::Registry::HKEY_LOCAL_MACHINE;

    let key = open(
        HKEY_LOCAL_MACHINE,
        r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
        0,
    )?;
    string_value(&key, "CurrentBuildNumber")
}

/// The macOS version from `SystemVersion.plist`: a file read, no program started.
#[cfg(target_os = "macos")]
fn os_version() -> Option<String> {
    let path = std::path::Path::new("/System/Library/CoreServices/SystemVersion.plist");
    let bytes = crate::persist::read_limited(path, 64 * 1024).ok()??;
    telemetry::plist_product_version(&String::from_utf8_lossy(&bytes)).map(str::to_owned)
}

/// Linux is only ever `linux`: no distribution, no kernel version.
#[cfg(not(any(windows, target_os = "macos")))]
fn os_version() -> Option<String> {
    None
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// Writes a whole file (path, content).
pub type WriteFile = dyn Fn(&std::path::Path, &[u8]) -> io::Result<()> + Send + Sync;

pub struct Options {
    /// The state file; `None` keeps everything in memory (tests).
    pub path: Option<PathBuf>,
    pub send_allowed: bool,
    /// The package version (`CARGO_PKG_VERSION`).
    pub version: String,
    pub machine: Machine,
    /// Unix ms; the system clock outside tests.
    pub clock: Arc<dyn Fn() -> u64 + Send + Sync>,
    /// Writes the state file; [`crate::persist::write_atomic`] outside tests.
    pub write: Arc<WriteFile>,
}

impl Options {
    /// `dir` is the local app data folder; the file name depends on the build
    /// ([`telemetry::state_file`]).
    pub fn system(dir: PathBuf) -> Self {
        Self {
            path: Some(dir.join(telemetry::state_file())),
            send_allowed: SEND_ALLOWED,
            version: env!("CARGO_PKG_VERSION").to_owned(),
            machine: detect_machine(),
            clock: Arc::new(now_ms),
            write: Arc::new(crate::persist::write_atomic),
        }
    }
}

/// The state and what the file holds of it, so only a change is written.
struct Core {
    state: State,
    saved: State,
}

struct Inner {
    core: Mutex<Core>,
    options: Options,
    transport: Arc<dyn Transport>,
    stop: AtomicBool,
}

/// Cheap to clone for blocking threads.
#[derive(Clone)]
pub struct TelemetryService {
    inner: Arc<Inner>,
}

impl TelemetryService {
    pub fn new(options: Options, transport: Arc<dyn Transport>) -> Self {
        let state = options
            .path
            .as_deref()
            .map(telemetry::load)
            .unwrap_or_default();
        Self {
            inner: Arc::new(Inner {
                core: Mutex::new(Core {
                    saved: state.clone(),
                    state,
                }),
                options,
                transport,
                stop: AtomicBool::new(false),
            }),
        }
    }

    fn today(&self) -> Day {
        Day::from_unix_ms((self.inner.options.clock)())
    }

    fn core(&self) -> MutexGuard<'_, Core> {
        lock(&self.inner.core)
    }

    /// Writes the state when it differs from the last successful write. A failed write leaves
    /// `saved` as it was, so the next tick tries again.
    fn flush(&self, core: &mut Core) -> io::Result<()> {
        let Some(path) = &self.inner.options.path else {
            core.saved = core.state.clone();
            return Ok(());
        };
        if core.state == core.saved {
            return Ok(());
        }
        let written = serde_json::to_vec(&core.state)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
            .and_then(|json| (self.inner.options.write)(path, &json));
        match written {
            Ok(()) => {
                core.saved = core.state.clone();
                Ok(())
            }
            Err(error) => {
                eprintln!("hangar: usage statistics not saved: {error}");
                Err(error)
            }
        }
    }

    /// The answer the file holds now; `None` without a file (tests, nothing stored yet).
    fn disk_consent(&self) -> Option<Option<bool>> {
        let path = self.inner.options.path.as_deref()?;
        match crate::persist::read_limited(path, telemetry::MAX_STATE_BYTES) {
            Ok(Some(bytes)) => Some(State::from_bytes(&bytes).consent),
            _ => None,
        }
    }

    /// Takes the file's state when its answer changed since this process last wrote it: another
    /// Hangar process answered, and nothing of this process's counting is written over that.
    /// Compared with the last successful write, not with memory: an "off" this process could not
    /// write yet must not be undone by the older "on" still in the file.
    fn adopt_disk(&self, core: &mut Core) {
        let Some(path) = &self.inner.options.path else {
            return;
        };
        if self
            .disk_consent()
            .is_some_and(|consent| consent != core.saved.consent)
        {
            let state = telemetry::load(path);
            core.saved = state.clone();
            core.state = state;
        }
    }

    /// Changes the state in memory while the statistics are on; the minute tick writes it.
    fn update(&self, change: impl FnOnce(&mut State, Day)) {
        let today = self.today();
        let mut core = self.core();
        if core.state.enabled() {
            change(&mut core.state, today);
        }
    }

    /// `None`: the person was not asked yet.
    pub fn consent(&self) -> Option<bool> {
        self.core().state.consent
    }

    /// The person's answer, written at once. It holds in memory even when the write fails (an
    /// "off" then stops counting and sending right away); the error is returned, and the next
    /// tick writes again.
    pub fn set_consent(&self, enabled: bool) -> io::Result<Option<bool>> {
        let mut core = self.core();
        core.state.set_consent(enabled);
        self.flush(&mut core)?;
        Ok(core.state.consent)
    }

    pub fn session_started(&self, program: &'static str, mode: SessionMode) {
        self.update(|state, today| state.session_started(today, program, mode));
    }

    /// One use of `feature`. An update about to be installed is written at once: the installer
    /// may end Hangar before the next tick.
    pub fn count(&self, feature: Feature) {
        self.update(|state, today| state.feature(today, feature));
        if feature == Feature::UpdateInstalled {
            let mut core = self.core();
            let _ = self.flush(&mut core);
        }
    }

    pub fn set_settings(&self, input: &SettingsInput) {
        self.update(|state, today| state.set_settings(today, input));
    }

    /// One look: the answer on disk first, then a minute for each running session (`minute`),
    /// the day rolled over, the state written when it changed, and yesterday's report sent when
    /// it is due.
    pub fn tick(&self, minute: bool, running: &[&'static str], team: impl FnOnce() -> Team) {
        let today = self.today();
        let due = {
            let mut core = self.core();
            self.adopt_disk(&mut core);
            if !core.state.enabled() {
                // An answer whose write failed is written again here.
                let _ = self.flush(&mut core);
                return;
            }
            if minute && !running.is_empty() {
                core.state.minute(today, running);
            } else {
                core.state.roll(today);
            }
            let _ = self.flush(&mut core);
            core.state
                .due(today, (self.inner.options.clock)())
                .cloned()
                .map(|pending| (pending, !core.state.first_report_done))
        };
        if let Some((pending, first)) = due {
            self.deliver(&pending, first, team());
        }
    }

    /// Whether a report may leave right now: on in memory and on in the file, checked again
    /// just before the request, so an answer given meanwhile, here or in another process, holds.
    fn may_send(&self) -> bool {
        let mut core = self.core();
        self.adopt_disk(&mut core);
        // "Off" in memory always wins, whatever the file says.
        if !core.state.enabled() {
            return false;
        }
        match &self.inner.options.path {
            Some(_) => self.disk_consent() == Some(Some(true)),
            None => true,
        }
    }

    fn deliver(&self, pending: &telemetry::Pending, first: bool, team: Team) {
        let date = pending.date;
        let report = telemetry::build_report(
            pending,
            first,
            &self.inner.options.version,
            &self.inner.options.machine,
            team,
        );
        let outcome = match telemetry::encode(&report) {
            None => Outcome::GiveUp("the report is too large"),
            Some(_) if !self.inner.options.send_allowed => Outcome::GiveUp("debug build"),
            Some(_) if !self.may_send() => return,
            Some(body) => match self.inner.transport.post(body) {
                Ok(()) => Outcome::Sent,
                Err(error) => Outcome::Failed(error),
            },
        };
        let mut core = self.core();
        match outcome {
            Outcome::Sent => core.state.sent(date),
            Outcome::GiveUp(reason) => {
                eprintln!(
                    "hangar: usage statistics for {} not sent: {reason}",
                    date.text()
                );
                core.state.give_up(date);
            }
            Outcome::Failed(error) => {
                eprintln!(
                    "hangar: usage statistics for {} not sent: {error}",
                    date.text()
                );
                let now = (self.inner.options.clock)();
                core.state.failed(date, error.is_transient(), now);
            }
        }
        let _ = self.flush(&mut core);
    }

    /// Starts the thread: a first look after [`FIRST_CHECK`], then a tick every [`TICK`].
    /// `running` gives the program key of every running session, `team` the account and team
    /// facts for a report.
    pub fn start(
        &self,
        running: impl Fn() -> Vec<&'static str> + Send + 'static,
        team: impl Fn() -> Team + Send + 'static,
    ) -> io::Result<()> {
        let service = self.clone();
        std::thread::Builder::new()
            .name("usage-statistics".into())
            .spawn(move || {
                if !service.sleep(FIRST_CHECK) {
                    return;
                }
                service.tick(false, &[], &team);
                while service.sleep(TICK) {
                    // Nothing is read about the sessions while the statistics are off.
                    if service.consent() == Some(true) {
                        service.tick(true, &running(), &team);
                    } else {
                        service.tick(false, &[], &team);
                    }
                }
            })
            .map(|_| ())
    }

    /// Sleeps in short steps; `false` once the service stops.
    fn sleep(&self, total: Duration) -> bool {
        let step = Duration::from_secs(1);
        let mut slept = Duration::ZERO;
        while slept < total {
            if self.inner.stop.load(Ordering::Relaxed) {
                return false;
            }
            std::thread::sleep(step);
            slept += step;
        }
        !self.inner.stop.load(Ordering::Relaxed)
    }

    /// Ends the thread and writes what was counted since the last tick; Hangar quits.
    pub fn stop(&self) {
        self.inner.stop.store(true, Ordering::Relaxed);
        let mut core = self.core();
        self.adopt_disk(&mut core);
        let _ = self.flush(&mut core);
    }
}

enum Outcome {
    Sent,
    GiveUp(&'static str),
    Failed(SendError),
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU64;

    use super::*;
    use crate::test_support::TempDir;

    /// 2026-11-12 08:00 UTC.
    const MORNING: u64 = 1_794_441_600_000 + 8 * 3_600_000;
    const DAY: u64 = 86_400_000;

    #[derive(Default)]
    struct Recorder {
        bodies: Mutex<Vec<serde_json::Value>>,
        answer: Mutex<Option<SendError>>,
    }

    impl Transport for Recorder {
        fn post(&self, body: Vec<u8>) -> Result<(), SendError> {
            lock(&self.bodies).push(serde_json::from_slice(&body).unwrap());
            match lock(&self.answer).clone() {
                Some(error) => Err(error),
                None => Ok(()),
            }
        }
    }

    struct Harness {
        service: TelemetryService,
        recorder: Arc<Recorder>,
        clock: Arc<AtomicU64>,
        /// Every write fails while set (a file held by a virus scanner).
        write_fails: Arc<AtomicBool>,
    }

    impl Harness {
        fn new(path: Option<PathBuf>, send_allowed: bool) -> Self {
            let recorder = Arc::new(Recorder::default());
            let clock = Arc::new(AtomicU64::new(MORNING));
            let time = Arc::clone(&clock);
            let write_fails = Arc::new(AtomicBool::new(false));
            let fails = Arc::clone(&write_fails);
            let service = TelemetryService::new(
                Options {
                    path,
                    send_allowed,
                    version: "0.14.0+648".to_owned(),
                    machine: Machine {
                        os: "windows",
                        os_release: Some("11".to_owned()),
                        arch: Some("x64"),
                    },
                    clock: Arc::new(move || time.load(Ordering::Relaxed)),
                    write: Arc::new(move |path, data| {
                        if fails.load(Ordering::Relaxed) {
                            Err(io::Error::new(io::ErrorKind::PermissionDenied, "held"))
                        } else {
                            crate::persist::write_atomic(path, data)
                        }
                    }),
                },
                recorder.clone(),
            );
            Self {
                service,
                recorder,
                clock,
                write_fails,
            }
        }

        fn advance(&self, ms: u64) {
            self.clock.fetch_add(ms, Ordering::Relaxed);
        }

        fn sent(&self) -> Vec<serde_json::Value> {
            lock(&self.recorder.bodies).clone()
        }
    }

    fn no_team() -> Team {
        Team::default()
    }

    #[test]
    fn debug_builds_never_send() {
        assert_eq!(SEND_ALLOWED, !cfg!(debug_assertions));
        let h = Harness::new(None, false);
        h.service.set_consent(true).unwrap();
        h.service.session_started("claude", SessionMode::Private);
        h.advance(DAY);
        h.service.tick(false, &[], no_team);
        assert!(h.sent().is_empty());
        // The day is given up, not kept for a later build.
        assert_eq!(h.service.core().state.pending, None);
    }

    #[test]
    fn nothing_is_counted_or_sent_without_consent() {
        let dir = TempDir::new("telemetry-no-consent");
        let path = dir.path().join(telemetry::STATE_FILE);
        let h = Harness::new(Some(path.clone()), true);
        assert_eq!(h.service.consent(), None);
        h.service.session_started("claude", SessionMode::Private);
        h.service.count(Feature::NotesCreated);
        h.service.tick(true, &["claude"], no_team);
        h.advance(DAY);
        h.service.tick(true, &["claude"], no_team);
        assert!(h.sent().is_empty());
        // Not even a file before the person answered.
        assert!(!path.exists());
        h.service.set_consent(false).unwrap();
        h.advance(DAY);
        h.service.tick(true, &["claude"], no_team);
        assert!(h.sent().is_empty());
    }

    #[test]
    fn one_report_for_yesterday_with_first_report_once() {
        let h = Harness::new(None, true);
        h.service.set_consent(true).unwrap();
        h.service.session_started("claude", SessionMode::Team);
        h.service.tick(true, &["claude"], no_team);
        h.service.count(Feature::FolderRevealed);
        // Midnight passes while Hangar runs.
        h.advance(DAY);
        let team = || Team {
            signed_in: true,
            shared_projects: 2,
        };
        h.service.tick(true, &["claude"], team);
        h.service.tick(true, &["claude"], team);
        let sent = h.sent();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["date"], "2026-11-12");
        assert_eq!(sent[0]["firstReport"], true);
        assert_eq!(sent[0]["usage"]["sessionsStarted"]["claude"], 1);
        assert_eq!(sent[0]["usage"]["sessionMinutes"]["claude"], 1);
        assert_eq!(sent[0]["usage"]["teamSessions"], 1);
        assert_eq!(sent[0]["usage"]["features"]["folderRevealed"], 1);
        assert_eq!(sent[0]["team"]["sharedProjects"], 2);

        h.advance(DAY);
        h.service.tick(false, &[], no_team);
        let sent = h.sent();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1]["date"], "2026-11-13");
        assert_eq!(sent[1]["firstReport"], false);
        // The minutes of the ticks after midnight belong to the 13th.
        assert_eq!(sent[1]["usage"]["sessionMinutes"]["claude"], 2);
    }

    #[test]
    fn a_failed_send_is_dropped_or_retried_later() {
        let h = Harness::new(None, true);
        h.service.set_consent(true).unwrap();
        h.service.count(Feature::BroadcastSent);
        h.advance(DAY);
        *lock(&h.recorder.answer) = Some(SendError::Connect);
        h.service.tick(false, &[], no_team);
        h.service.tick(false, &[], no_team);
        assert_eq!(h.sent().len(), 1, "the retry waits");
        *lock(&h.recorder.answer) = None;
        h.advance(telemetry::RETRY_MS);
        h.service.tick(false, &[], no_team);
        assert_eq!(h.sent().len(), 2);
        h.advance(telemetry::RETRY_MS);
        h.service.tick(false, &[], no_team);
        assert_eq!(h.sent().len(), 2, "sent once, never again");

        let refused = Harness::new(None, true);
        refused.service.set_consent(true).unwrap();
        refused.service.count(Feature::BroadcastSent);
        refused.advance(DAY);
        *lock(&refused.recorder.answer) = Some(SendError::Status(400));
        refused.service.tick(false, &[], no_team);
        refused.advance(telemetry::RETRY_MS);
        refused.service.tick(false, &[], no_team);
        assert_eq!(refused.sent().len(), 1);

        // A timeout after connecting or another server error may have stored the report: the
        // day is given up rather than counted twice.
        for error in [
            SendError::Timeout,
            SendError::Status(500),
            SendError::Network,
        ] {
            let unsure = Harness::new(None, true);
            unsure.service.set_consent(true).unwrap();
            unsure.service.count(Feature::BroadcastSent);
            unsure.advance(DAY);
            *lock(&unsure.recorder.answer) = Some(error.clone());
            unsure.service.tick(false, &[], no_team);
            unsure.advance(telemetry::RETRY_MS);
            unsure.service.tick(false, &[], no_team);
            assert_eq!(unsure.sent().len(), 1, "{error}");
        }
    }

    #[test]
    fn the_state_survives_a_restart_and_turning_off_clears_it() {
        let dir = TempDir::new("telemetry-restart");
        let path = dir.path().join(telemetry::STATE_FILE);
        let h = Harness::new(Some(path.clone()), true);
        h.service.set_consent(true).unwrap();
        h.service.set_settings(&SettingsInput {
            language: "fr".to_owned(),
            theme: "dark".to_owned(),
            terminal_scheme: "hangar".to_owned(),
            terminal_font: "default".to_owned(),
            terminal_font_size: 14,
            cursor_style: "bar".to_owned(),
            viewer_theme: "meridian".to_owned(),
            dot_animation: "wave".to_owned(),
            attention: true,
            restore_sessions: false,
            side_panel_detached: true,
            sidebar_collapsed: false,
        });
        h.service
            .session_started("shell:pwsh", SessionMode::Private);
        // Counting alone writes nothing; quitting writes it.
        assert_eq!(telemetry::load(&path).usage, telemetry::Usage::default());
        h.service.stop();

        // The next day's start sends what the previous run counted.
        let next = Harness::new(Some(path.clone()), true);
        next.clock.store(MORNING + DAY, Ordering::Relaxed);
        assert_eq!(next.service.consent(), Some(true));
        next.service.tick(false, &[], no_team);
        let sent = next.sent();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0]["system"]["language"], "fr");
        assert_eq!(sent[0]["settings"]["theme"], "dark");
        assert_eq!(sent[0]["settings"]["sidePanelDetached"], true);
        assert_eq!(sent[0]["usage"]["sessionsStarted"]["shell:pwsh"], 1);

        next.service.count(Feature::NotesCreated);
        next.service.set_consent(false).unwrap();
        let stored = telemetry::load(&path);
        assert_eq!(stored.consent, Some(false));
        assert_eq!(stored.usage, telemetry::Usage::default());
        assert_eq!(stored.settings, None);
        assert!(stored.first_report_done);
    }

    #[test]
    fn a_damaged_file_starts_over_without_consent() {
        let dir = TempDir::new("telemetry-damaged");
        let path = dir.path().join(telemetry::STATE_FILE);
        std::fs::write(&path, b"{\"consent\": tr").unwrap();
        let h = Harness::new(Some(path), true);
        assert_eq!(h.service.consent(), None);
    }

    #[test]
    fn send_errors_worth_a_retry() {
        assert!(SendError::Connect.is_transient());
        assert!(SendError::Status(429).is_transient());
        assert!(SendError::Status(503).is_transient());
        assert!(!SendError::Timeout.is_transient());
        assert!(!SendError::Network.is_transient());
        for status in [400, 413, 415, 500, 502, 504] {
            assert!(!SendError::Status(status).is_transient());
        }
    }

    /// "Off" that could not be written: the older "on" in the file is not taken back, nothing is
    /// sent, and the next tick writes the "off".
    #[test]
    fn an_unwritten_off_is_kept_and_written_later() {
        let dir = TempDir::new("telemetry-unwritten-off");
        let path = dir.path().join(telemetry::STATE_FILE);
        let h = Harness::new(Some(path.clone()), true);
        h.service.set_consent(true).unwrap();
        h.service.count(Feature::BroadcastSent);
        h.service.tick(false, &[], no_team);

        h.write_fails.store(true, Ordering::Relaxed);
        assert!(h.service.set_consent(false).is_err());
        assert_eq!(h.service.consent(), Some(false));
        assert_eq!(telemetry::load(&path).consent, Some(true));
        h.advance(DAY);
        h.service.tick(true, &["claude"], no_team);
        assert_eq!(
            h.service.consent(),
            Some(false),
            "the file's older yes is not taken"
        );
        assert!(h.sent().is_empty());

        h.write_fails.store(false, Ordering::Relaxed);
        h.service.tick(false, &[], no_team);
        assert_eq!(telemetry::load(&path).consent, Some(false));
        assert!(h.sent().is_empty());
    }

    /// "Off" in memory wins at the last check even while the file still says yes.
    #[test]
    fn off_in_memory_never_sends() {
        let dir = TempDir::new("telemetry-off-wins");
        let path = dir.path().join(telemetry::STATE_FILE);
        let h = Harness::new(Some(path), true);
        h.service.set_consent(true).unwrap();
        h.service.count(Feature::BroadcastSent);
        h.advance(DAY);
        h.write_fails.store(true, Ordering::Relaxed);
        h.service.tick(false, &[], || {
            assert!(h.service.set_consent(false).is_err());
            Team::default()
        });
        assert!(h.sent().is_empty());
    }

    /// Two processes on one file: the one that hears "off" from the other stops at once, never
    /// sends and never writes its "on" back.
    #[test]
    fn an_answer_in_another_process_wins() {
        let dir = TempDir::new("telemetry-two-processes");
        let path = dir.path().join(telemetry::STATE_FILE);
        let a = Harness::new(Some(path.clone()), true);
        a.service.set_consent(true).unwrap();
        let b = Harness::new(Some(path.clone()), true);
        assert_eq!(b.service.consent(), Some(true));
        b.service.session_started("claude", SessionMode::Private);
        b.service.tick(true, &["claude"], no_team);

        a.service.set_consent(false).unwrap();
        b.advance(DAY);
        b.service.tick(true, &["claude"], no_team);
        b.service.stop();
        assert!(b.sent().is_empty());
        assert_eq!(b.service.consent(), Some(false));
        assert_eq!(telemetry::load(&path).consent, Some(false));

        let c = Harness::new(Some(path.clone()), true);
        c.clock.store(MORNING + DAY, Ordering::Relaxed);
        c.service.tick(true, &["claude"], no_team);
        assert!(c.sent().is_empty());
        assert_eq!(c.service.consent(), Some(false));
    }

    /// Turned off after the tick found the report due, before the request: it stays home.
    #[test]
    fn the_answer_is_checked_again_right_before_sending() {
        let dir = TempDir::new("telemetry-last-check");
        let path = dir.path().join(telemetry::STATE_FILE);
        let h = Harness::new(Some(path.clone()), true);
        h.service.set_consent(true).unwrap();
        h.service.count(Feature::BroadcastSent);
        h.advance(DAY);
        let other = Harness::new(Some(path), true);
        h.service.tick(false, &[], || {
            other.service.set_consent(false).unwrap();
            Team::default()
        });
        assert!(h.sent().is_empty());
        assert_eq!(h.service.consent(), Some(false));
    }

    #[test]
    fn the_file_is_written_only_on_change_and_an_update_at_once() {
        let dir = TempDir::new("telemetry-writes");
        let path = dir.path().join(telemetry::STATE_FILE);
        let h = Harness::new(Some(path.clone()), true);
        h.service.set_consent(true).unwrap();
        h.service.count(Feature::PathMentioned);
        assert_eq!(telemetry::load(&path).usage, telemetry::Usage::default());
        h.service.count(Feature::UpdateInstalled);
        let stored = telemetry::load(&path).usage.features;
        assert_eq!(stored.get(&Feature::UpdateInstalled), Some(&1));
        assert_eq!(stored.get(&Feature::PathMentioned), Some(&1));
        // Nothing changed since: the tick leaves the file alone.
        std::fs::remove_file(&path).unwrap();
        h.service.tick(false, &[], no_team);
        assert!(!path.exists());
    }

    #[test]
    fn reports_go_to_the_official_endpoint_over_https() {
        assert!(catalog::ENDPOINT.starts_with("https://hangar.feoren.com/"));
        assert_eq!(
            reqwest::Url::parse(catalog::ENDPOINT).unwrap().as_str(),
            catalog::ENDPOINT
        );
        assert!(reqwest::Url::parse(catalog::INFO_URL).is_ok());
    }
}
