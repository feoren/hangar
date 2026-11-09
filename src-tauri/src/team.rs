//! Team connection, pure part (team service step F part 2a; design:
//! docs/wiki/pages/syntheses/team-service-v1.md, "Step F as built (part 2a)").
//!
//! A signed-in person turns on "Share with team" for a project; Hangar then keeps one WebSocket
//! to the team relay for it, verifies the team's membership log and holds the current epoch's
//! secret. This module holds what is decided without I/O: the window's view of a team, the
//! reconnect policy (close codes, backoff, the one quick retry), the timers of a live connection
//! (ticket renewal, the relay's connection age, pings, silence), and the stored formats (the
//! enabled projects, the device identity and the epoch secrets in the keychain, the event log
//! cursor and replay marks in a file). The protocol itself is `team_engine.rs`, the socket
//! `team_relay.rs`, the workers `team_service.rs`.
//!
//! No secret appears in `Debug`, an error, a log line or an event.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use hangar_team_crypto::epoch::TeamSecret;
use hangar_team_crypto::identity::SafetyNumber;
use hangar_team_protocol::e2e::PublicKey;
use hangar_team_protocol::e2e::{ChainHead, LogHash};
use hangar_team_protocol::ids::{Epoch, Seq, StreamId};
use hangar_team_protocol::wire::{Cursor, Holder};
use serde::{Deserialize, Serialize, Serializer};
use sha2::{Digest as _, Sha256};
use zeroize::Zeroizing;

/// Event to the main window with every new [`TeamSnapshot`].
pub const EVENT: &str = "team-changed";
/// Keychain service of device identities and epoch secrets. Separate from the account's and the
/// profile secrets' services; debug builds keep their own, like the account
/// (`account::KEYCHAIN_SERVICE`), so a dev build never acts as the installed app's device.
#[cfg(not(debug_assertions))]
pub const KEYCHAIN_SERVICE: &str = "hangar-team";
#[cfg(debug_assertions)]
pub const KEYCHAIN_SERVICE: &str = "hangar-team-dev";
/// The projects with "Share with team" on, per keychain service and server, in the config folder.
pub const CONFIG_FILE: &str = "team.json";
/// Per keychain service and team, the event log cursor, the send counter and the replay marks
/// (nothing secret), and the lock file of the running connection, in the local data folder:
/// `team/<service>/<key>.json` and `.lock`. Debug and release builds keep separate keychain
/// services, so each build has its own devices and must not share their state.
pub const STATE_DIR: &str = "team";
/// Epoch secrets kept per team: the newest ones. The Windows credential store holds at most
/// 1280 UTF-16 units per entry (`secrets::MAX_VALUE_UNITS`); eight secrets and the log head take
/// under 700.
pub const KEPT_EPOCHS: usize = 8;
/// Bytes of a random device id (hex in the protocol).
const DEVICE_ID_BYTES: usize = 16;

// ---------------------------------------------------------------------------------------------
// The window's view
// ---------------------------------------------------------------------------------------------

/// A team's connection as the window shows it. Transitions (`team_service.rs`):
/// `Off -> Connecting` (turned on), `Connecting -> Connected | WaitingForConfirmation |
/// NeedsAttention | Removed | ProjectDeleted`, `Connected -> Connecting` (connection lost, with
/// `retrying`), any state `-> Off` (turned off).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TeamStatus {
    Off,
    Connecting {
        /// An earlier attempt failed; the next one waits for the backoff.
        retrying: bool,
    },
    Connected {
        epoch: u32,
        members: usize,
        devices: usize,
    },
    /// This device is not confirmed yet (not in the membership log, or without the current
    /// epoch's key); the person compares the safety number on the confirming device.
    WaitingForConfirmation {
        safety_number: Vec<String>,
    },
    NeedsAttention {
        reason: AttentionReason,
    },
    /// The person no longer belongs to the project (close 4403, or the log removed this device).
    Removed,
    /// The project was deleted on the website (close 4404).
    ProjectDeleted,
}

/// Why a team needs the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AttentionReason {
    /// The membership log does not verify, or contradicts what this device verified before (a
    /// fork, or a relay that rewound or rewrote the history).
    HistoryMismatch,
    /// Not signed in to the account, or the sign-in ended.
    SignedOut,
    /// The keychain refused to store or read the device's keys.
    Keychain,
    /// The relay speaks a newer protocol.
    NeedsUpdate,
    /// The relay refused this device (a fresh ticket was refused too) or answered nonsense.
    Refused,
    /// The team's state file on this device exists but cannot be read; starting over would
    /// forget the event log cursor and the replay marks.
    Storage,
    /// Another Hangar window on this computer runs this team's connection; this one waits and
    /// takes over once that one ends.
    OtherInstance,
    /// This computer's device for the team belongs to another account (another person signed
    /// in on the same keychain). Its keys stay untouched; sharing waits until that person signs
    /// in again or the device is removed from the team.
    OtherAccount,
}

/// A device waiting for this device's confirmation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingConfirmation {
    /// Stable per team, holder and keys.
    pub id: String,
    pub project_id: String,
    pub member: String,
    pub device: String,
    pub kind: PendingKind,
    pub safety_number: Vec<String>,
    /// The log removed this member before: confirming admits them again. The website decides
    /// who is a member and may hand an id out again, so the window says so plainly.
    pub readmitted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PendingKind {
    /// Another device of the signed-in person (add device, endorsed by this one).
    OwnDevice,
    /// The first device of a member the log does not list yet (add member, owner or admin).
    NewMember,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamEntry {
    pub project_id: String,
    pub status: TeamStatus,
    /// The teammates' sessions and their collisions with this device's sessions, and the
    /// repositories the project shares from this device.
    pub activity: crate::team_share::TeamActivity,
}

/// Payload of `team-changed`: every team that is on or not off, every pending confirmation, and
/// the repositories of this device's running sessions (offered for linking to a project).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamSnapshot {
    pub teams: Vec<TeamEntry>,
    pub pending: Vec<PendingConfirmation>,
    pub repos: Vec<crate::team_share::RepoChoice>,
}

/// The safety number as the window shows it: six groups of five digits.
pub fn safety_groups(number: &SafetyNumber) -> Vec<String> {
    number.groups().iter().map(|g| format!("{g:05}")).collect()
}

/// Errors of the team commands; the window shows a text per `kind`.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TeamError {
    #[error("Not signed in")]
    NotSignedIn,
    #[error("Unknown project")]
    UnknownProject,
    #[error("Unknown confirmation request")]
    UnknownRequest,
    #[error("The keychain failed: {0}")]
    Keychain(String),
    #[error("Settings could not be saved: {0}")]
    Config(String),
    #[error("The team connection is busy; try again")]
    Busy,
    #[error("No running session works in that repository")]
    UnknownRepository,
}

impl TeamError {
    fn kind(&self) -> &'static str {
        match self {
            Self::NotSignedIn => "notSignedIn",
            Self::UnknownProject => "unknownProject",
            Self::UnknownRequest => "unknownRequest",
            Self::Keychain(_) => "keychain",
            Self::Config(_) => "config",
            Self::Busy => "busy",
            Self::UnknownRepository => "unknownRepository",
        }
    }

    fn detail(&self) -> Option<String> {
        match self {
            Self::Keychain(text) | Self::Config(text) => Some(text.clone()),
            _ => None,
        }
    }
}

/// `{ kind, detail }`, like the account's errors.
impl Serialize for TeamError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct as _;
        let mut state = serializer.serialize_struct("TeamError", 2)?;
        state.serialize_field("kind", self.kind())?;
        state.serialize_field("detail", &self.detail())?;
        state.end()
    }
}

// ---------------------------------------------------------------------------------------------
// Reconnect policy
// ---------------------------------------------------------------------------------------------

/// Why a team stops connecting until the person acts (turns sharing off and on, signs in, or
/// updates Hangar).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    Removed,
    ProjectDeleted,
    Attention(AttentionReason),
}

impl StopReason {
    pub fn status(self) -> TeamStatus {
        match self {
            Self::Removed => TeamStatus::Removed,
            Self::ProjectDeleted => TeamStatus::ProjectDeleted,
            Self::Attention(reason) => TeamStatus::NeedsAttention { reason },
        }
    }
}

/// How one connection ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEnd {
    /// The relay closed the socket, with its close code if it sent one.
    Closed(Option<u16>),
    /// Connecting failed, the socket broke, or the relay went silent.
    Lost,
    /// The relay answered `hello` with `err forbidden` (a ticket that went stale on the way).
    HelloForbidden,
    /// Hangar reconnects on its own before the relay's connection age runs out.
    MaxAge,
    /// The event log has a gap: reconnecting replays or resyncs.
    Gap,
    /// The protocol or the storage decided to stop.
    Stop(StopReason),
}

/// What the worker does after a connection ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    /// Fetch a fresh ticket and connect again after `delay`.
    Reconnect {
        delay: Duration,
    },
    Stop(StopReason),
}

/// The relay's close codes (crate `hangar-team-protocol`, module `close`) and Hangar's own
/// endings, turned into a reconnect or a stop. Every connection fetches a fresh ticket, so
/// "fetch a fresh ticket and reconnect" is the plain reconnect.
///
/// One quick (immediate) reconnect is available after each connection that proved itself
/// ([`ReconnectPolicy::settled`]: the membership log was fetched and checked, or the connection
/// lasted [`Timing::stable_after`]): for 4401 (role changed), the proactive reconnect before the
/// relay's connection age, a log gap, and the one retry after `err forbidden` on `hello`. Further
/// endings back off (1 s doubling to 60 s, with jitter), so a relay that accepts `hello` and then
/// keeps closing cannot make Hangar spin. A second `err forbidden` in a row stops with
/// [`AttentionReason::Refused`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconnectPolicy {
    attempt: u32,
    quick_available: bool,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            attempt: 0,
            quick_available: true,
        }
    }
}

/// The longest backoff.
pub const BACKOFF_MAX: Duration = Duration::from_secs(60);
const BACKOFF_FLOOR: Duration = Duration::from_secs(1);

impl ReconnectPolicy {
    /// The connection proved itself: the backoff starts over and one quick reconnect is
    /// available. A mere `welcome` does not count, so a relay closing right after it backs off.
    pub fn settled(&mut self) {
        self.attempt = 0;
        self.quick_available = true;
    }

    /// Whether the last connection attempts failed (the window shows "reconnecting").
    pub fn retrying(&self) -> bool {
        self.attempt > 0
    }

    /// The decision after `end`; `jitter` in `[0, 1)` spreads reconnecting clients.
    pub fn after(&mut self, end: SessionEnd, jitter: f64) -> Next {
        match end {
            SessionEnd::Stop(reason) => Next::Stop(reason),
            SessionEnd::Closed(Some(hangar_team_protocol::close::CLOSE_REMOVED)) => {
                Next::Stop(StopReason::Removed)
            }
            SessionEnd::Closed(Some(hangar_team_protocol::close::CLOSE_TEAM_DELETED)) => {
                Next::Stop(StopReason::ProjectDeleted)
            }
            SessionEnd::HelloForbidden => {
                if self.quick_available {
                    self.quick_available = false;
                    Next::Reconnect {
                        delay: Duration::ZERO,
                    }
                } else {
                    Next::Stop(StopReason::Attention(AttentionReason::Refused))
                }
            }
            SessionEnd::Closed(Some(hangar_team_protocol::close::CLOSE_ROLE_CHANGED))
            | SessionEnd::MaxAge
            | SessionEnd::Gap
                if self.quick_available =>
            {
                self.quick_available = false;
                Next::Reconnect {
                    delay: Duration::ZERO,
                }
            }
            // 1008 (no valid hello in time), other codes, a broken socket: back off.
            _ => Next::Reconnect {
                delay: self.backoff(jitter),
            },
        }
    }

    fn backoff(&mut self, jitter: f64) -> Duration {
        let cap = BACKOFF_FLOOR
            .saturating_mul(1u32 << self.attempt.min(6))
            .min(BACKOFF_MAX);
        self.attempt = self.attempt.saturating_add(1);
        // Equal jitter: half the cap fixed, half random.
        let jitter = if jitter.is_finite() {
            jitter.clamp(0.0, 1.0)
        } else {
            0.5
        };
        cap.mul_f64(0.5 + 0.5 * jitter).max(BACKOFF_FLOOR)
    }
}

// ---------------------------------------------------------------------------------------------
// Timers of a live connection
// ---------------------------------------------------------------------------------------------

/// Timing of a connection; shorter in tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    /// `ticket.renew` with a fresh website ticket this often (protocol: about every 5 minutes).
    pub renew_every: Duration,
    /// Reconnect after this long, before the relay ends the connection at its maximum age
    /// (600 s on the relay; Hangar goes at 540 s).
    pub max_age: Duration,
    /// A `ping` this often (the relay's design: every 25 s).
    pub ping_every: Duration,
    /// Nothing heard from the relay for this long: the connection counts as lost.
    pub silence: Duration,
    /// How often an approving device asks the relay for devices waiting for confirmation.
    pub devices_every: Duration,
    /// The longest wait for the socket and the TLS and WebSocket handshakes.
    pub connect_timeout: Duration,
    /// Scales the backoff (1 s in the app).
    pub backoff_unit: Duration,
    /// A connection open this long counts as settled even before its log check finished.
    pub stable_after: Duration,
    /// How often a window waiting for another window's connection checks whether it ended.
    pub lock_retry: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            renew_every: Duration::from_secs(300),
            max_age: Duration::from_secs(540),
            ping_every: Duration::from_secs(25),
            silence: Duration::from_secs(60),
            devices_every: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(15),
            backoff_unit: Duration::from_secs(1),
            stable_after: Duration::from_secs(30),
            lock_retry: Duration::from_secs(5),
        }
    }
}

/// What a live connection's clock asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    Renew,
    Ping,
    Devices,
    /// The connection is old enough to be replaced ([`SessionEnd::MaxAge`]).
    MaxAge,
    /// The relay went silent ([`SessionEnd::Lost`]).
    Silent,
}

/// The deadlines of one connection, in milliseconds since it opened (injected time, so the
/// tests need no clock).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionClock {
    timing: Timing,
    renew_at: u64,
    ping_at: u64,
    devices_at: u64,
    max_age_at: u64,
    last_heard: u64,
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

impl ConnectionClock {
    /// A connection opened at `now`.
    pub fn new(timing: Timing, now: u64) -> Self {
        Self {
            timing,
            renew_at: now.saturating_add(millis(timing.renew_every)),
            ping_at: now.saturating_add(millis(timing.ping_every)),
            devices_at: now.saturating_add(millis(timing.devices_every)),
            max_age_at: now.saturating_add(millis(timing.max_age)),
            last_heard: now,
        }
    }

    /// A frame arrived at `now`.
    pub fn heard(&mut self, now: u64) {
        self.last_heard = self.last_heard.max(now);
    }

    /// The earliest deadline, to sleep until.
    pub fn next_wake(&self) -> u64 {
        [
            self.renew_at,
            self.ping_at,
            self.devices_at,
            self.max_age_at,
            self.last_heard.saturating_add(millis(self.timing.silence)),
        ]
        .into_iter()
        .min()
        .unwrap_or(u64::MAX)
    }

    /// Everything due at `now`, the connection-ending ones first; repeating ones move on.
    pub fn due(&mut self, now: u64) -> Vec<Due> {
        let mut due = Vec::new();
        if now >= self.last_heard.saturating_add(millis(self.timing.silence)) {
            due.push(Due::Silent);
        }
        if now >= self.max_age_at {
            due.push(Due::MaxAge);
        }
        if now >= self.renew_at {
            due.push(Due::Renew);
            self.renew_at = now.saturating_add(millis(self.timing.renew_every));
        }
        if now >= self.ping_at {
            due.push(Due::Ping);
            self.ping_at = now.saturating_add(millis(self.timing.ping_every));
        }
        if now >= self.devices_at {
            due.push(Due::Devices);
            self.devices_at = now.saturating_add(millis(self.timing.devices_every));
        }
        due
    }
}

// ---------------------------------------------------------------------------------------------
// Stored formats
// ---------------------------------------------------------------------------------------------

/// The key of one team on one server: the first 16 bytes of SHA-256 over both, length-prefixed,
/// in hex. Names the keychain entries and the state file without spelling out ids or addresses.
pub fn team_key(base: &str, project: &str) -> String {
    let mut sha = Sha256::new();
    for part in [
        b"hangar-team/app/v1".as_slice(),
        base.as_bytes(),
        project.as_bytes(),
    ] {
        let len = u32::try_from(part.len()).unwrap_or(u32::MAX);
        sha.update(len.to_be_bytes());
        sha.update(part);
    }
    hex(&sha.finalize()[..16])
}

/// Keychain account of the device identity of a team.
pub fn device_account(key: &str) -> String {
    format!("device/{key}")
}

/// Keychain account of the epoch secrets and the verified log head of a team.
pub fn keys_account(key: &str) -> String {
    format!("keys/{key}")
}

pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    push_hex(&mut out, bytes);
    out
}

/// Appends `bytes` in hex to `out` without an intermediate string, so a secret's hex form exists
/// only in the caller's (zeroized) buffer.
fn push_hex(out: &mut String, bytes: &[u8]) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        out.push(char::from(DIGITS[usize::from(byte >> 4)]));
        out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
}

fn unhex(text: &str) -> Option<Zeroizing<Vec<u8>>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Zeroizing::new(Vec::with_capacity(text.len() / 2));
    for pair in text.as_bytes().chunks(2) {
        let pair = std::str::from_utf8(pair).ok()?;
        out.push(u8::from_str_radix(pair, 16).ok()?);
    }
    Some(out)
}

/// A new random device id (hex), from `fill`.
pub fn new_device_id(
    fill: impl FnOnce(&mut [u8]) -> Result<(), hangar_team_crypto::EntropyError>,
) -> Result<String, hangar_team_crypto::EntropyError> {
    let mut bytes = [0u8; DEVICE_ID_BYTES];
    fill(&mut bytes)?;
    Ok(hex(&bytes))
}

/// The device entry in the keychain: `v2 <device id> <member in hex> <exported identity in hex>`.
/// The member is the account the device acts for, bound at its first `welcome`; an entry of the
/// first format (`v1 <device id> <identity>`, no member) is bound at its next `welcome`.
pub struct StoredDevice {
    pub device: String,
    /// `None` until the relay's first `welcome` named the account (and for `v1` entries).
    pub member: Option<String>,
    pub secret: Zeroizing<Vec<u8>>,
}

impl std::fmt::Debug for StoredDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredDevice")
            .field("device", &self.device)
            .field("member", &self.member)
            .finish_non_exhaustive()
    }
}

impl StoredDevice {
    pub fn encode(&self) -> Zeroizing<String> {
        let member = self.member.as_deref().unwrap_or_default();
        let mut out = Zeroizing::new(String::with_capacity(
            6 + self.device.len() + 2 * member.len() + 2 * self.secret.len(),
        ));
        out.push_str("v2 ");
        out.push_str(&self.device);
        out.push(' ');
        if member.is_empty() {
            out.push('-');
        } else {
            push_hex(&mut out, member.as_bytes());
        }
        out.push(' ');
        push_hex(&mut out, &self.secret);
        out
    }

    pub fn decode(text: &str) -> Option<Self> {
        let mut parts = text.split(' ');
        let version = parts.next()?;
        let device = parts.next()?;
        let member = match version {
            "v1" => None,
            "v2" => match parts.next()? {
                "-" => None,
                hex => {
                    let bytes = unhex(hex)?;
                    let member = String::from_utf8(bytes.to_vec()).ok()?;
                    (!member.is_empty()).then_some(member)
                }
            },
            _ => return None,
        };
        let secret = unhex(parts.next()?)?;
        let valid = parts.next().is_none()
            && !device.is_empty()
            && device.bytes().all(|b| b.is_ascii_hexdigit());
        valid.then(|| Self {
            device: device.to_owned(),
            member,
            secret,
        })
    }
}

/// The keys entry in the keychain: the newest verified log head and the newest
/// [`KEPT_EPOCHS`] epoch secrets. `v1 <index>:<hash hex>|- <epoch>:<secret hex>...`
#[derive(Default)]
pub struct StoredKeys {
    pub head: Option<ChainHead>,
    pub epochs: BTreeMap<Epoch, TeamSecret>,
}

impl std::fmt::Debug for StoredKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredKeys")
            .field("head", &self.head.map(|h| h.index))
            .field("epochs", &self.epochs.keys().collect::<Vec<_>>())
            .finish_non_exhaustive()
    }
}

impl StoredKeys {
    /// Keeps only the newest [`KEPT_EPOCHS`] secrets.
    pub fn trim(&mut self) {
        while self.epochs.len() > KEPT_EPOCHS {
            if self.epochs.pop_first().is_none() {
                break;
            }
        }
    }

    pub fn encode(&self) -> Zeroizing<String> {
        // Room for every secret up front: a growing buffer would leave copies behind unzeroized.
        let mut out = Zeroizing::new(String::with_capacity(96 + self.epochs.len() * 80));
        out.push_str("v1 ");
        match &self.head {
            Some(head) => {
                out.push_str(&format!("{}:{}", head.index, head.hash.to_hex()));
            }
            None => out.push('-'),
        }
        for (epoch, secret) in &self.epochs {
            out.push_str(&format!(" {}:", epoch.0));
            push_hex(&mut out, secret.export().as_slice());
        }
        out
    }

    pub fn decode(text: &str) -> Option<Self> {
        let mut parts = text.split(' ');
        if parts.next()? != "v1" {
            return None;
        }
        let head = match parts.next()? {
            "-" => None,
            head => {
                let (index, hash) = head.split_once(':')?;
                let bytes = unhex(hash)?;
                let hash: [u8; 32] = bytes.as_slice().try_into().ok()?;
                Some(ChainHead {
                    index: index.parse().ok()?,
                    hash: LogHash::from_bytes(hash),
                })
            }
        };
        let mut epochs = BTreeMap::new();
        for part in parts {
            let (epoch, secret) = part.split_once(':')?;
            let bytes = unhex(secret)?;
            let mut array = Zeroizing::new([0u8; 32]);
            if bytes.len() != array.len() {
                return None;
            }
            array.copy_from_slice(&bytes);
            epochs.insert(Epoch(epoch.parse().ok()?), TeamSecret::from_bytes(array));
        }
        Some(Self { head, epochs })
    }
}

/// One replay mark: the highest record counter accepted from a sender and signing key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayMark {
    pub holder: Holder,
    pub sign: PublicKey,
    pub counter: u64,
}

/// The highest counter shown from a teammate's claim detail for one of its sessions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DetailMark {
    pub holder: Holder,
    /// The session key the detail was sealed for (its record id).
    pub session: String,
    pub counter: u64,
}

/// The highest counter of a teammate's claim details whose session marks were dropped to stay
/// within bounds: a detail of a session without a mark must be newer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HolderMark {
    pub holder: Holder,
    pub counter: u64,
}

/// The team's state file (nothing secret): where the event log was read up to, the last record
/// counter used, the replay marks, the high-water marks of the live state shown (presence
/// status and claim details), and the confirmation requests the person rejected.
///
/// The counter only rises: besides this file, every start raises it to [`counter_floor`] of the
/// clock, so a backup put back or a file that went missing cannot make the device repeat one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalState {
    #[serde(default)]
    pub cursor: Option<Cursor>,
    /// `SendCounter::last`; stored before a record with the next counter is sent.
    #[serde(default)]
    pub counter: u64,
    #[serde(default)]
    pub marks: Vec<ReplayMark>,
    /// Per teammate device and signing key: the newest presence status counter shown. Live
    /// state the relay may send again, so equal counters pass; older ones never do, also after an
    /// `offline` event, a reconnect or a restart.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub presence_marks: Vec<ReplayMark>,
    /// Per teammate device and session key: the newest claim detail counter shown.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail_marks: Vec<DetailMark>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail_floors: Vec<HolderMark>,
    #[serde(default)]
    pub rejected: Vec<String>,
}

/// Rejected requests remembered per team, newest last.
const MAX_REJECTED: usize = 256;

/// The lowest send counter a start may resume from: Unix milliseconds times 1024. Counters stay
/// unique across a lost or restored state file as long as this device sends fewer than 1024
/// records per millisecond and its clock does not go back meanwhile.
pub fn counter_floor(now_ms: u64) -> u64 {
    now_ms.saturating_mul(1 << 10)
}

/// The folder of one keychain service's team files.
fn service_dir(dir: &Path, service: &str) -> PathBuf {
    dir.join(STATE_DIR).join(service)
}

/// The lock file held while a window runs the connection of team `key`.
pub fn lock_path(dir: &Path, service: &str, key: &str) -> PathBuf {
    service_dir(dir, service).join(format!("{key}.lock"))
}

impl LocalState {
    pub fn path(dir: &Path, service: &str, key: &str) -> PathBuf {
        service_dir(dir, service).join(format!("{key}.json"))
    }

    /// The stored state; empty when there is no file yet. A file that exists but cannot be read
    /// or parsed is an error: starting empty would forget the replay marks and the cursor.
    pub fn load(dir: &Path, service: &str, key: &str) -> std::io::Result<Self> {
        match std::fs::read_to_string(Self::path(dir, service, key)) {
            Ok(text) => serde_json::from_str(&text).map_err(std::io::Error::other),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, dir: &Path, service: &str, key: &str) -> std::io::Result<()> {
        let text = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::persist::write_atomic(&Self::path(dir, service, key), &text)
    }

    pub fn remove(dir: &Path, service: &str, key: &str) {
        let _ = std::fs::remove_file(Self::path(dir, service, key));
    }

    pub fn reject(&mut self, id: &str) {
        if !self.rejected.iter().any(|r| r == id) {
            self.rejected.push(id.to_owned());
        }
        if self.rejected.len() > MAX_REJECTED {
            let drop = self.rejected.len() - MAX_REJECTED;
            self.rejected.drain(..drop);
        }
    }

    pub fn cursor_parts(&self) -> Option<(&StreamId, Seq)> {
        self.cursor.as_ref().map(|c| (&c.stream, c.seq))
    }
}

/// `team.json`: the projects with "Share with team" on, per keychain service (one per build, like
/// `account.json`'s sign-ins) and server address. A build reads and writes only its own part and
/// rereads the file before writing, so the other build's list is kept.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamConfig {
    #[serde(default)]
    pub enabled_by_service: BTreeMap<String, BTreeMap<String, Vec<String>>>,
    /// Per keychain service, server and project: the repositories whose sessions the project
    /// shares (presence and claims). Nothing is shared from a repository the person did not
    /// link, so a private repository never reaches a team by accident.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub repos_by_service: BTreeMap<String, BTreeMap<String, BTreeMap<String, Vec<LinkedRepo>>>>,
}

/// A repository folder linked to a shared project: its identity in the team protocol (the root
/// commit, `RepoId` of the protocol crate), the clone on this device (its git common folder as
/// `claims::folder_key` normalizes it, so every worktree of that clone counts) and the folder
/// name it had when it was linked.
///
/// The root commit alone is not enough: every other clone and every fork of the same project
/// shares it, and a private fork must not reach the team because its upstream was linked. A new
/// folder with a known root commit is shared only once the person ticks it.
///
/// `folder` is `None` for links made before folders were bound (root commit only). Such a link
/// shares nothing: the window asks the person to tick the folders again (the safer migration,
/// since nothing tells which of today's clones the person meant back then).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkedRepo {
    pub root: String,
    #[serde(default)]
    pub folder: Option<String>,
    pub name: String,
}

impl LinkedRepo {
    /// Whether this link shares the clone at `folder` of the repository `root`.
    pub fn shares(&self, root: &str, folder: &str) -> bool {
        self.root == root && self.folder.as_deref() == Some(folder)
    }
}

impl TeamConfig {
    /// The stored choices; empty when there is no file yet. A file that exists but cannot be
    /// read or parsed is an error: writing over it would drop every other project's choice and
    /// the other build's part.
    pub fn load(config_dir: &Path) -> std::io::Result<Self> {
        match std::fs::read_to_string(config_dir.join(CONFIG_FILE)) {
            Ok(text) => serde_json::from_str(&text).map_err(std::io::Error::other),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save(&self, config_dir: &Path) -> std::io::Result<()> {
        let text = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        crate::persist::write_atomic(&config_dir.join(CONFIG_FILE), &text)
    }

    pub fn enabled_for(&self, service: &str, base: &str) -> &[String] {
        self.enabled_by_service
            .get(service)
            .and_then(|servers| servers.get(base))
            .map_or(&[], Vec::as_slice)
    }

    pub fn linked(&self, service: &str, base: &str, project: &str) -> &[LinkedRepo] {
        self.repos_by_service
            .get(service)
            .and_then(|servers| servers.get(base))
            .and_then(|projects| projects.get(project))
            .map_or(&[], Vec::as_slice)
    }

    /// Links or unlinks a repository folder (by root commit and folder); `true` when the list
    /// changed. Linking a folder drops a link of the same root commit from before folders were
    /// bound: the person confirmed the folder it stood for.
    pub fn link(
        &mut self,
        service: &str,
        base: &str,
        project: &str,
        repo: &LinkedRepo,
        on: bool,
    ) -> bool {
        let servers = self.repos_by_service.entry(service.to_owned()).or_default();
        let projects = servers.entry(base.to_owned()).or_default();
        let list = projects.entry(project.to_owned()).or_default();
        let same = |r: &LinkedRepo| r.root == repo.root && r.folder == repo.folder;
        let before = list.len();
        if on && repo.folder.is_some() {
            list.retain(|r| r.root != repo.root || r.folder.is_some());
        }
        let present = list.iter().any(same);
        let mut changed = list.len() != before;
        if present != on {
            changed = true;
            if on {
                list.push(repo.clone());
            } else {
                list.retain(|r| !same(r));
            }
        }
        if list.is_empty() {
            projects.remove(project);
        }
        if projects.is_empty() {
            servers.remove(base);
        }
        if servers.is_empty() {
            self.repos_by_service.remove(service);
        }
        changed
    }

    /// `true` when the list changed.
    pub fn set(&mut self, service: &str, base: &str, project: &str, on: bool) -> bool {
        let servers = self
            .enabled_by_service
            .entry(service.to_owned())
            .or_default();
        let list = servers.entry(base.to_owned()).or_default();
        let changed = list.iter().any(|p| p == project) != on;
        if changed {
            if on {
                list.push(project.to_owned());
            } else {
                list.retain(|p| p != project);
            }
        }
        if list.is_empty() {
            servers.remove(base);
        }
        if servers.is_empty() {
            self.enabled_by_service.remove(service);
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLOSE_HELLO_TIMEOUT: u16 = hangar_team_protocol::close::CLOSE_HELLO_TIMEOUT;

    fn delay(next: Next) -> Duration {
        match next {
            Next::Reconnect { delay } => delay,
            Next::Stop(reason) => panic!("stopped: {reason:?}"),
        }
    }

    #[test]
    fn close_codes_decide_between_reconnect_and_stop() {
        let mut policy = ReconnectPolicy::default();
        assert_eq!(
            policy.after(SessionEnd::Closed(Some(4403)), 0.5),
            Next::Stop(StopReason::Removed)
        );
        assert_eq!(
            policy.after(SessionEnd::Closed(Some(4404)), 0.5),
            Next::Stop(StopReason::ProjectDeleted)
        );
        // 4401: a fresh ticket at once.
        assert_eq!(
            delay(policy.after(SessionEnd::Closed(Some(4401)), 0.5)),
            Duration::ZERO
        );
        // 1008 (hello timeout) backs off.
        assert!(
            delay(policy.after(SessionEnd::Closed(Some(CLOSE_HELLO_TIMEOUT)), 0.0))
                >= BACKOFF_FLOOR
        );
        assert_eq!(
            policy.after(
                SessionEnd::Stop(StopReason::Attention(AttentionReason::HistoryMismatch)),
                0.5
            ),
            Next::Stop(StopReason::Attention(AttentionReason::HistoryMismatch))
        );
    }

    #[test]
    fn a_stale_ticket_on_hello_is_retried_once() {
        let mut policy = ReconnectPolicy::default();
        assert_eq!(
            delay(policy.after(SessionEnd::HelloForbidden, 0.5)),
            Duration::ZERO
        );
        assert_eq!(
            policy.after(SessionEnd::HelloForbidden, 0.5),
            Next::Stop(StopReason::Attention(AttentionReason::Refused))
        );
        // After a settled connection the retry is available again.
        policy.settled();
        assert_eq!(
            delay(policy.after(SessionEnd::HelloForbidden, 0.5)),
            Duration::ZERO
        );
    }

    #[test]
    fn backoff_doubles_with_jitter_from_one_to_sixty_seconds() {
        let mut policy = ReconnectPolicy::default();
        let mut lows = Vec::new();
        for _ in 0..10 {
            lows.push(delay(policy.after(SessionEnd::Lost, 0.0)));
        }
        assert_eq!(lows[0], Duration::from_secs(1));
        assert_eq!(lows[1], Duration::from_secs(1));
        assert_eq!(lows[2], Duration::from_secs(2));
        assert_eq!(lows[3], Duration::from_secs(4));
        assert_eq!(lows[9], Duration::from_secs(30));
        assert!(policy.retrying());
        let high = delay(policy.after(SessionEnd::Lost, 0.999_999));
        assert!(
            high <= BACKOFF_MAX && high > Duration::from_secs(59),
            "{high:?}"
        );
        // Not a number counts as the middle.
        assert_eq!(
            delay(policy.after(SessionEnd::Lost, f64::NAN)),
            Duration::from_secs(45)
        );
        policy.settled();
        assert!(!policy.retrying());
        assert_eq!(
            delay(policy.after(SessionEnd::Lost, 0.0)),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn only_one_quick_reconnect_per_accepted_hello() {
        let mut policy = ReconnectPolicy::default();
        assert_eq!(delay(policy.after(SessionEnd::MaxAge, 0.5)), Duration::ZERO);
        // The relay closing with 4401 right after: back off instead of spinning.
        assert!(delay(policy.after(SessionEnd::Closed(Some(4401)), 0.0)) >= BACKOFF_FLOOR);
        assert!(delay(policy.after(SessionEnd::Gap, 0.0)) >= BACKOFF_FLOOR);
        policy.settled();
        assert_eq!(delay(policy.after(SessionEnd::Gap, 0.5)), Duration::ZERO);
    }

    fn timing() -> Timing {
        Timing {
            renew_every: Duration::from_secs(300),
            max_age: Duration::from_secs(540),
            ping_every: Duration::from_secs(25),
            silence: Duration::from_secs(60),
            devices_every: Duration::from_secs(30),
            ..Timing::default()
        }
    }

    #[test]
    fn the_clock_renews_every_five_minutes_and_reconnects_before_the_max_age() {
        let mut clock = ConnectionClock::new(timing(), 1_000);
        assert_eq!(clock.next_wake(), 26_000);
        let mut renewals = 0;
        let mut ended = None;
        let mut now = 1_000;
        while now < 1_000 + 600_000 {
            clock.heard(now);
            for due in clock.due(now) {
                match due {
                    Due::Renew => renewals += 1,
                    Due::MaxAge if ended.is_none() => ended = Some(now),
                    Due::Silent => panic!("heard every second"),
                    _ => {}
                }
            }
            now += 1_000;
        }
        assert_eq!(ended, Some(541_000));
        // At 301 s; the next would be at 601 s.
        assert_eq!(renewals, 1);
    }

    #[test]
    fn the_clock_reports_silence_and_pings() {
        let mut clock = ConnectionClock::new(timing(), 0);
        assert_eq!(clock.due(24_999), Vec::<Due>::new());
        assert_eq!(clock.due(25_000), vec![Due::Ping]);
        assert_eq!(clock.due(30_000), vec![Due::Devices]);
        clock.heard(40_000);
        assert!(!clock.due(99_999).contains(&Due::Silent));
        assert!(clock.due(100_000).contains(&Due::Silent));
    }

    #[test]
    fn stored_device_and_keys_round_trip_and_refuse_garbage() {
        let device = StoredDevice {
            device: "00ff".to_owned(),
            member: None,
            secret: Zeroizing::new(vec![1, 2, 255]),
        };
        let text = device.encode();
        assert_eq!(text.as_str(), "v2 00ff - 0102ff");
        let back = StoredDevice::decode(&text).unwrap();
        assert_eq!(back.device, "00ff");
        assert_eq!(back.member, None);
        assert_eq!(back.secret.as_slice(), &[1, 2, 255]);
        assert!(!format!("{back:?}").contains("0102ff"));
        // Bound to an account: the member travels in hex, so any opaque id fits.
        let bound = StoredDevice {
            member: Some("anna b@x".to_owned()),
            ..device
        };
        let back = StoredDevice::decode(&bound.encode()).unwrap();
        assert_eq!(back.member.as_deref(), Some("anna b@x"));
        // An entry of the first format has no member yet.
        let old = StoredDevice::decode("v1 00ff 0102ff").unwrap();
        assert_eq!(old.member, None);
        assert_eq!(old.secret.as_slice(), &[1, 2, 255]);
        for bad in [
            "",
            "v2 00ff 01",
            "v3 00ff - 01",
            "v2 00ff zz 01",
            "v1 0 0",
            "v1 zz 01",
            "v1 00 01 extra",
            "v1  01",
        ] {
            assert!(StoredDevice::decode(bad).is_none(), "{bad}");
        }

        let mut keys = StoredKeys::default();
        assert_eq!(keys.encode().as_str(), "v1 -");
        keys.head = Some(ChainHead {
            index: 7,
            hash: LogHash::from_bytes([3; 32]),
        });
        for epoch in 1..=10u32 {
            let byte = u8::try_from(epoch).unwrap();
            keys.epochs.insert(
                Epoch(epoch),
                TeamSecret::from_bytes(Zeroizing::new([byte; 32])),
            );
        }
        keys.trim();
        assert_eq!(keys.epochs.len(), KEPT_EPOCHS);
        assert_eq!(keys.epochs.keys().next(), Some(&Epoch(3)));
        let text = keys.encode();
        assert!(text.encode_utf16().count() < crate::secrets::MAX_VALUE_UNITS);
        let back = StoredKeys::decode(&text).unwrap();
        assert_eq!(back.head, keys.head);
        assert_eq!(back.epochs.len(), KEPT_EPOCHS);
        assert_eq!(back.epochs.get(&Epoch(10)), keys.epochs.get(&Epoch(10)));
        assert!(!format!("{back:?}").contains("0a0a"));
        for bad in ["v1", "v1 x:00", "v1 - 1:00", "v1 1:zz", "v0 -"] {
            assert!(StoredKeys::decode(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn team_keys_differ_per_server_and_project() {
        let a = team_key("https://a/api/v1", "1");
        assert_eq!(a.len(), 32);
        assert_ne!(a, team_key("https://a/api/v1", "2"));
        assert_ne!(a, team_key("https://b/api/v1", "1"));
        // Length prefixes: moving a character between the fields changes the key.
        assert_ne!(team_key("ab", "c"), team_key("a", "bc"));
        assert_eq!(device_account(&a), format!("device/{a}"));
        assert_eq!(keys_account(&a), format!("keys/{a}"));
    }

    #[test]
    fn config_and_local_state_round_trip_through_files() {
        let temp = crate::test_support::TempDir::new("team-config");
        let dir = temp.path();
        let mut config = TeamConfig::load(dir).unwrap();
        assert!(config.set("svc", "https://a", "1", true));
        assert!(!config.set("svc", "https://a", "1", true));
        assert!(config.set("svc", "https://a", "2", true));
        config.save(dir).unwrap();
        let mut back = TeamConfig::load(dir).unwrap();
        assert_eq!(back.enabled_for("svc", "https://a"), ["1", "2"]);
        assert!(back.set("svc", "https://a", "1", false));
        assert!(back.set("svc", "https://a", "2", false));
        assert!(back.enabled_by_service.is_empty());

        let mut state = LocalState::load(dir, "svc", "k").unwrap();
        assert_eq!(state, LocalState::default());
        state.cursor = Some(Cursor {
            stream: StreamId::new("s1").unwrap(),
            seq: Seq(4),
        });
        state.counter = 9;
        for i in 0..300 {
            state.reject(&format!("r{i}"));
        }
        state.reject("r299");
        assert_eq!(state.rejected.len(), MAX_REJECTED);
        state.save(dir, "svc", "k").unwrap();
        assert_eq!(LocalState::load(dir, "svc", "k").unwrap(), state);
        LocalState::remove(dir, "svc", "k");
        assert_eq!(
            LocalState::load(dir, "svc", "k").unwrap(),
            LocalState::default()
        );
    }

    #[test]
    fn two_keychain_services_keep_their_own_teams_in_one_folder() {
        let temp = crate::test_support::TempDir::new("team-services");
        let dir = temp.path();
        // The release build turns sharing on; the debug build on the same folder does not see it.
        let mut config = TeamConfig::load(dir).unwrap();
        config.set("hangar-team", "https://a", "1", true);
        config.save(dir).unwrap();
        let mut config = TeamConfig::load(dir).unwrap();
        assert!(config
            .enabled_for("hangar-team-dev", "https://a")
            .is_empty());
        config.set("hangar-team-dev", "https://a", "2", true);
        config.save(dir).unwrap();
        let back = TeamConfig::load(dir).unwrap();
        assert_eq!(back.enabled_for("hangar-team", "https://a"), ["1"]);
        assert_eq!(back.enabled_for("hangar-team-dev", "https://a"), ["2"]);

        // The same team key: separate state and lock files.
        let release = LocalState {
            counter: 7,
            ..LocalState::default()
        };
        release.save(dir, "hangar-team", "k").unwrap();
        assert_eq!(
            LocalState::load(dir, "hangar-team-dev", "k").unwrap(),
            LocalState::default()
        );
        assert_eq!(LocalState::load(dir, "hangar-team", "k").unwrap(), release);
        assert_ne!(
            lock_path(dir, "hangar-team", "k"),
            lock_path(dir, "hangar-team-dev", "k")
        );
    }

    #[test]
    fn an_unreadable_config_file_is_an_error_not_an_empty_list() {
        let temp = crate::test_support::TempDir::new("team-config-broken");
        let dir = temp.path();
        std::fs::write(dir.join(CONFIG_FILE), "{ not json").unwrap();
        assert!(TeamConfig::load(dir).is_err());
        std::fs::remove_file(dir.join(CONFIG_FILE)).unwrap();
        std::fs::create_dir(dir.join(CONFIG_FILE)).unwrap();
        assert!(TeamConfig::load(dir).is_err());
    }

    #[test]
    fn linked_repositories_are_kept_per_service_server_and_project() {
        let temp = crate::test_support::TempDir::new("team-config-repos");
        let dir = temp.path();
        let repo = LinkedRepo {
            root: "a".repeat(40),
            folder: Some("c:/src/hangar/.git".to_owned()),
            name: "hangar".to_owned(),
        };
        let mut config = TeamConfig::default();
        assert!(config.link("svc", "https://a", "1", &repo, true));
        assert!(!config.link("svc", "https://a", "1", &repo, true));
        config.save(dir).unwrap();
        let mut back = TeamConfig::load(dir).unwrap();
        assert_eq!(
            back.linked("svc", "https://a", "1"),
            std::slice::from_ref(&repo)
        );
        assert!(back.linked("svc", "https://a", "2").is_empty());
        assert!(back.linked("other", "https://a", "1").is_empty());
        // Another clone (or a fork) of the same repository is a link of its own.
        let fork = LinkedRepo {
            folder: Some("c:/src/hangar-fork/.git".to_owned()),
            ..repo.clone()
        };
        let linked = back.linked("svc", "https://a", "1");
        assert!(linked[0].shares(&repo.root, "c:/src/hangar/.git"));
        assert!(!linked
            .iter()
            .any(|r| r.shares(&repo.root, "c:/src/hangar-fork/.git")));
        assert!(back.link("svc", "https://a", "1", &fork, true));
        assert!(back.link("svc", "https://a", "1", &repo, false));
        assert!(back.link("svc", "https://a", "1", &fork, false));
        assert!(back.repos_by_service.is_empty());
    }

    #[test]
    fn a_link_from_before_folders_shares_nothing_until_a_folder_is_ticked() {
        // team.json as the root-only version wrote it.
        let text = r#"{"enabledByService":{},"reposByService":{"svc":{"https://a":{"1":[
            {"root":"aaaa","name":"hangar"}]}}}}"#;
        let mut config: TeamConfig = serde_json::from_str(text).unwrap();
        let legacy = config.linked("svc", "https://a", "1")[0].clone();
        assert_eq!(legacy.folder, None);
        assert!(!legacy.shares("aaaa", "c:/src/hangar/.git"));
        // Ticking a folder of that repository replaces it.
        let folder = LinkedRepo {
            root: "aaaa".to_owned(),
            folder: Some("c:/src/hangar/.git".to_owned()),
            name: "hangar".to_owned(),
        };
        assert!(config.link("svc", "https://a", "1", &folder, true));
        assert_eq!(
            config.linked("svc", "https://a", "1"),
            std::slice::from_ref(&folder)
        );
    }

    #[test]
    fn an_unreadable_state_file_is_an_error_not_an_empty_state() {
        let temp = crate::test_support::TempDir::new("team-state-broken");
        let dir = temp.path();
        let path = LocalState::path(dir, "svc", "k");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ not json").unwrap();
        assert!(LocalState::load(dir, "svc", "k").is_err());
        // A folder where the file belongs cannot be read either.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(LocalState::load(dir, "svc", "k").is_err());
    }

    #[test]
    fn the_counter_floor_follows_the_clock() {
        assert_eq!(counter_floor(0), 0);
        assert_eq!(counter_floor(1_700_000_000_000), 1_700_000_000_000 * 1024);
        assert!(counter_floor(1_700_000_000_001) > counter_floor(1_700_000_000_000) + 1000);
        assert_eq!(counter_floor(u64::MAX), u64::MAX);
    }

    #[test]
    fn statuses_serialize_for_the_window() {
        let json = |status: &TeamStatus| serde_json::to_value(status).unwrap();
        assert_eq!(json(&TeamStatus::Off), serde_json::json!({"state": "off"}));
        assert_eq!(
            json(&TeamStatus::Connected {
                epoch: 2,
                members: 3,
                devices: 4
            }),
            serde_json::json!({"state": "connected", "epoch": 2, "members": 3, "devices": 4})
        );
        assert_eq!(
            json(&TeamStatus::WaitingForConfirmation {
                safety_number: vec!["00001".to_owned()]
            }),
            serde_json::json!({"state": "waitingForConfirmation", "safetyNumber": ["00001"]})
        );
        assert_eq!(
            json(&TeamStatus::NeedsAttention {
                reason: AttentionReason::HistoryMismatch
            }),
            serde_json::json!({"state": "needsAttention", "reason": "historyMismatch"})
        );
        assert_eq!(
            json(&TeamStatus::ProjectDeleted),
            serde_json::json!({"state": "projectDeleted"})
        );
        for (reason, name) in [
            (AttentionReason::Storage, "storage"),
            (AttentionReason::OtherInstance, "otherInstance"),
            (AttentionReason::OtherAccount, "otherAccount"),
        ] {
            assert_eq!(
                json(&TeamStatus::NeedsAttention { reason }),
                serde_json::json!({"state": "needsAttention", "reason": name})
            );
        }
        assert_eq!(
            serde_json::to_value(TeamError::Keychain("x".to_owned())).unwrap(),
            serde_json::json!({"kind": "keychain", "detail": "x"})
        );
    }
}
