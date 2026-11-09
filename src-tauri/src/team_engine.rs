//! The team protocol of one project on one connection, without I/O (team service step F part
//! 2a). The worker (`team_service.rs`) feeds it the relay's frames and its timers and carries
//! out the [`Effect`]s it returns, in order: a [`Effect::StoreKeys`] before a following
//! [`Effect::Send`] is the app's duty "store a rotation's secret before `key.rotate`"
//! (team-crypto, obligations of the app), and a failed store drops the rest.
//!
//! On every accepted `hello` the engine registers the device's keys (`device.keys`), fetches the
//! whole membership log (`memberlog.get`, paged) into a fresh `LogVerifier` and checks it
//! against the head this device verified before (fork or rewind: [`AttentionReason::HistoryMismatch`]).
//! Then it reconciles:
//!
//! - empty log and the person owns the project on the website: create the team (entry 1 with
//!   this device), then the first epoch (`key.rotate`);
//! - this device not in the log: wait for confirmation, showing its safety number;
//! - in the log without the current epoch's secret: the wrapped key from `key.rotated`, else
//!   `key.get`, opened with `unwrap_any` against the log's commitment;
//! - an owner or admin rotates when the log says a rotation is due (a device or member left);
//! - devices that wait for this device's confirmation come from `devices.get`: another device
//!   of the same person (add device, endorsed here) or, for an owner or admin, a new member's
//!   first device (add member). Confirming appends the entry and then wraps the current key to
//!   the new device (`key.wrap`).
//!
//! Once connected with the epoch's secret it shares (part 2b, `team_share.rs`): a `presence.set`
//! with the sealed status of this device's sessions in the project's linked repositories, and per
//! session with changed files a `claim.set` (keyed hashes plus the sealed detail; `claim.clear`
//! when the session ends). It opens the others' `presence.update` and the detail of every
//! `claim.overlap` with `open_verified` against the verified log. Both are live state the relay
//! sends again after a reconnect, so they are not run through the `ReplayGuard` (which refuses a
//! second delivery). Instead [`LiveMarks`] keep the newest counter shown per teammate device
//! (status) and per teammate session (detail) and refuse anything older; an equal counter is the
//! same record sent again and passes. The marks live in the state file and survive the relay's
//! unsigned `offline`, a reconnect and a restart, so the relay cannot bring back an old status or
//! detail. Records of an epoch older than the current one are refused too. The marks are written
//! at most every [`MARKS_STORE_EVERY_MS`] (and on the next ping, a new connection and the end of
//! one), all together, so the file always holds a consistent earlier snapshot.
//!
//! There is no age limit on top: a status is sent only when it changes (no periodic re-seal), so
//! a quiet teammate's newest status may be hours old and still current.
//!
//! An overlap counts only for a session the teammate's newest signed status lists: after
//! `claim.clear` the relay could send the last detail of an ended session again, or name a session
//! key it made up. Without a status from that device, an overlap shows as the same area at most.
//!
//! A signed live record whose log head has another hash at an index this device verified is a
//! fork only that sender (or the relay towards it) shows: the sending device is set aside
//! ([`DivergentDevice`] in the Team view) and its live records are ignored until this device's
//! verified log changes. The team stops ([`AttentionReason::HistoryMismatch`]) only when this
//! device's own log fetch contradicts what it verified before, so a single device cannot stop
//! everyone else's sharing.
//!
//! The device identity is bound to the account it was first welcomed for: a `welcome` naming
//! another member stops with [`AttentionReason::OtherAccount`] and touches no key.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use hangar_team_crypto::identity::{safety_number, DeviceIdentity, DevicePublicKeys};
use hangar_team_crypto::memberlog::{
    DeviceEntry, Endorsement, EntryBody, EntryKind, LogVerifier, TeamState,
};
use hangar_team_crypto::record::{
    EpochCipher, OpenError, Opened, RecordContext, RecordKind, ReplayGuard, SendCounter,
};
use hangar_team_crypto::rotation::{plan_rotation, wrap_for_device};
use hangar_team_crypto::{Entropy, EntropyError};
use hangar_team_protocol::e2e::{ChainHead, PublicKey};
use hangar_team_protocol::hash::{KeyedHash, RepoId};
use hangar_team_protocol::ids::{
    Ciphertext, DeviceId, Epoch, MemberId, OpId, Sealed, SessionKey, TeamId, Ticket, UnixMillis,
};
use hangar_team_protocol::log::{ClientCursor, Observed};
use hangar_team_protocol::wire::{
    check_welcome, decode_server, ClaimClear, ClaimOverlap, ClaimSet, ClientFrame, ClientMessage,
    DeviceInfo, DeviceRemove, ErrorCode, Hello, Holder, KeyGet, MatchLevel, MemberLogEntries,
    MemberLogGet, PresenceSet, PresenceState, PresenceUpdate, ProtoRange, Role, ServerFrame,
    ServerMessage, SessionRef, TicketRenew,
};
use sha2::{Digest as _, Sha256};

use crate::claims::{LineRange, OverlapLevel};
use crate::session::SessionId;
use crate::team::{
    hex, safety_groups, AttentionReason, DetailMark, HolderMark, LocalState, PendingConfirmation,
    PendingKind, ReplayMark, SessionEnd, StopReason, StoredDevice, StoredKeys, TeamError,
    TeamStatus,
};
use crate::team_share::{
    self, DivergentDevice, LocalShare, OtherDetail, PeerStatus, PeerView, SessionState,
    TeamActivity, TeammateFile, TeammateOverlap,
};

/// What the worker does, in this order.
#[derive(Debug)]
pub enum Effect {
    /// Encode and send the frame.
    Send(ClientFrame),
    /// Write [`Engine::keys`] (log head and epoch secrets) to the keychain; on failure the
    /// worker drops every later effect and stops with [`AttentionReason::Keychain`].
    StoreKeys,
    /// Write [`Engine::local`] (cursor, counter, marks, rejected requests) to the state file; on
    /// failure the worker drops every later effect and ends the connection.
    StoreLocal,
    /// Write [`Engine::stored_device`] to the keychain: the device was bound to the account its
    /// first `welcome` named. On failure as [`Effect::StoreKeys`].
    StoreDevice,
    /// The verified log removed this device: delete its identity and secrets and turn sharing
    /// off (team-crypto L4). Never the answer to a relay event alone.
    WipeDevice,
    /// The membership log was fetched and checked on this connection for the first time: the
    /// connection proved itself (the reconnect policy starts over).
    Settled,
    /// Read the person's projects on the website again (authenticated HTTPS) and hand the role
    /// to [`Engine::set_website_role`], or end the connection when the project is gone there.
    /// The answer to relay events only the website can confirm (`member.removed` of this
    /// member, `member.role_changed`).
    CheckWebsite,
    /// Status or pending confirmations changed.
    Changed,
    /// End this connection.
    End(SessionEnd),
}

/// What an op id stands for, to read its answer.
#[derive(Debug, Clone)]
enum Op {
    Hello,
    DeviceKeys,
    LogGet,
    DevicesGet,
    KeyGet(Epoch),
    /// `memberlog.append` creating the team, confirming a device (wrapped to afterwards), or
    /// removing a member.
    Append(Option<Holder>),
    Rotate(Epoch),
    Wrap,
    Remove,
    Quiet,
}

/// Wrapped keys from `key.rotated` kept for epochs whose log entry is not verified yet; more are
/// never needed, since a newer epoch makes the older ones useless.
const MAX_INLINE: usize = 4;

/// Live records whose sender names a log entry this device has not verified yet, kept for one
/// retry after the next log fetch.
const MAX_STASHED: usize = 64;

/// Claim detail marks kept per teammate device; a device with more sessions raises its floor.
const MAX_DETAIL_SESSIONS: usize = 64;

/// Overlaps kept per session of this device and device in the log, so a teammate cannot grow the
/// view (and the notices it raises) without bound by inventing session keys.
const MAX_OVERLAPS_PER_DEVICE: usize = 64;

/// The live marks go to the state file at most this often while they rise; the next ping, a new
/// connection and the end of one write what is left.
const MARKS_STORE_EVERY_MS: u64 = 5_000;

/// A teammate device's presence as last reported.
#[derive(Debug, Clone)]
struct Peer {
    presence: PresenceState,
    status: PeerStatus,
    /// `status` is a signed status this device opened, not the empty one of a presence the
    /// relay reported without (or with an unreadable) status: only a signed one ends overlaps.
    signed: bool,
}

/// How a live record's counter compares with the newest one shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mark {
    /// Older: an old record shown again; ignored.
    Stale,
    /// The same record again (the relay resends live state).
    Seen,
    /// Newer: the mark rose and goes into the state file.
    Raised,
}

/// High-water marks of the teammates' live state shown on this device: presence status per
/// device, claim detail per device and session key. Never cleared by `offline`, `hello` or a
/// restart (they are stored with [`LocalState`]); trimmed to the devices the verified log lists
/// and to [`MAX_DETAIL_SESSIONS`] sessions per device.
///
/// The presence mark is per device, not per signing key: a device id admitted again comes with
/// new keys, but its counter continues above the old ones (`team::counter_floor`), so a status
/// signed with the old key stays older than anything shown since.
#[derive(Debug, Default, PartialEq, Eq)]
struct LiveMarks {
    presence: BTreeMap<Holder, u64>,
    detail: BTreeMap<(Holder, String), u64>,
    /// Per device: the highest counter of a detail mark dropped to stay within bounds; a detail of
    /// a session without a mark must be newer.
    floors: BTreeMap<Holder, u64>,
}

impl LiveMarks {
    fn from_local(local: &LocalState) -> Self {
        // A file of the first format may hold several marks per device (one per signing key):
        // the highest one counts.
        let mut presence: BTreeMap<Holder, u64> = BTreeMap::new();
        for mark in &local.presence_marks {
            let counter = presence.entry(mark.holder.clone()).or_insert(0);
            *counter = (*counter).max(mark.counter);
        }
        Self {
            presence,
            detail: local
                .detail_marks
                .iter()
                .map(|m| ((m.holder.clone(), m.session.clone()), m.counter))
                .collect(),
            floors: local
                .detail_floors
                .iter()
                .map(|m| (m.holder.clone(), m.counter))
                .collect(),
        }
    }

    fn store(&self, local: &mut LocalState) {
        local.presence_marks = self
            .presence
            .iter()
            .map(|(holder, counter)| HolderMark {
                holder: holder.clone(),
                counter: *counter,
            })
            .collect();
        local.detail_marks = self
            .detail
            .iter()
            .map(|((holder, session), counter)| DetailMark {
                holder: holder.clone(),
                session: session.clone(),
                counter: *counter,
            })
            .collect();
        local.detail_floors = self
            .floors
            .iter()
            .map(|(holder, counter)| HolderMark {
                holder: holder.clone(),
                counter: *counter,
            })
            .collect();
    }

    fn raise(mark: &mut u64, counter: u64) -> Mark {
        match counter.cmp(mark) {
            std::cmp::Ordering::Less => Mark::Stale,
            std::cmp::Ordering::Equal => Mark::Seen,
            std::cmp::Ordering::Greater => {
                *mark = counter;
                Mark::Raised
            }
        }
    }

    fn presence(&mut self, holder: &Holder, counter: u64) -> Mark {
        let mark = self.presence.entry(holder.clone()).or_insert(0);
        Self::raise(mark, counter)
    }

    fn detail(&mut self, holder: &Holder, session: &str, counter: u64) -> Mark {
        let key = (holder.clone(), session.to_owned());
        if let Some(mark) = self.detail.get_mut(&key) {
            return Self::raise(mark, counter);
        }
        if counter <= self.floors.get(holder).copied().unwrap_or(0) {
            return Mark::Stale;
        }
        self.detail.insert(key, counter);
        // The device's marks are adjacent in the map: count them and find the lowest without
        // walking every other device's (at most MAX_DETAIL_SESSIONS + 1 entries).
        let mut count = 0;
        let mut lowest: Option<(&String, u64)> = None;
        for ((_, held), at) in self
            .detail
            .range((holder.clone(), String::new())..)
            .take_while(|((h, _), _)| h == holder)
        {
            count += 1;
            if lowest.is_none_or(|(_, low)| *at < low) {
                lowest = Some((held, *at));
            }
        }
        if count > MAX_DETAIL_SESSIONS {
            if let Some((dropped, at)) = lowest.map(|(s, at)| (s.clone(), at)) {
                self.detail.remove(&(holder.clone(), dropped.clone()));
                let floor = self.floors.entry(holder.clone()).or_insert(0);
                *floor = (*floor).max(at);
                if dropped == session {
                    return Mark::Stale;
                }
            }
        }
        Mark::Raised
    }

    /// Drops the marks of devices `known` does not vouch for; `true` when any went.
    fn retain(&mut self, known: impl Fn(&Holder) -> bool) -> bool {
        let before = (self.presence.len(), self.detail.len(), self.floors.len());
        self.presence.retain(|holder, _| known(holder));
        self.detail.retain(|(holder, _), _| known(holder));
        self.floors.retain(|holder, _| known(holder));
        before != (self.presence.len(), self.detail.len(), self.floors.len())
    }
}

/// A `claim.overlap` of one of this device's sessions with a teammate's session.
#[derive(Debug, Clone)]
struct OverlapEntry {
    level: MatchLevel,
    files: u32,
    /// The relay's shared file in this device's spelling, when the detail does not say.
    hint: Option<String>,
    detail: Option<OtherDetail>,
}

/// The overlaps per session of this device, then per teammate session: one session's count is
/// one map's length.
type Overlaps = BTreeMap<SessionId, BTreeMap<SessionRef, OverlapEntry>>;

/// A teammate device that reports a different team history (see the module comment).
#[derive(Debug, Clone)]
struct Divergent {
    /// This device's verified log head index when it happened; a later head clears it.
    at: u64,
    /// The display name it sent last, for the Team view.
    name: String,
}

/// How opening a sealed record went.
enum Open {
    Opened(Opened),
    /// The sender names a newer log or epoch: fetch the log and try once more.
    Later,
    /// The signed sender's log differs from this device's at an index both verified.
    Fork,
    Failed,
}

/// The randomness of the engine: the operating system's in the app, scripted in tests.
pub type EntropySource = Box<dyn Entropy + Send + Sync>;

/// Lends the engine's boxed source to the crypto crate's `&mut impl Entropy`.
struct Lend<'a>(&'a mut (dyn Entropy + Send + Sync));

impl Entropy for Lend<'_> {
    fn fill(&mut self, dest: &mut [u8]) -> Result<(), EntropyError> {
        self.0.fill(dest)
    }
}

/// A device waiting for this one's confirmation.
#[derive(Debug, Clone)]
struct Pending {
    view: PendingConfirmation,
    info: DeviceInfo,
}

/// Everything the engine starts from.
pub struct EngineSetup {
    pub project: String,
    pub team: TeamId,
    pub device: DeviceId,
    pub identity: DeviceIdentity,
    /// The account the device was bound to (`None`: not welcomed yet).
    pub member: Option<MemberId>,
    /// The person's role in the project on the website (the ticket's role); caps the log role.
    pub website_role: Role,
    pub keys: StoredKeys,
    pub local: LocalState,
    /// Unix milliseconds (log entries' `issued_at`, the send counter's floor).
    pub now: Box<dyn Fn() -> u64 + Send + Sync>,
    pub entropy: EntropySource,
}

pub struct Engine {
    project: String,
    team: TeamId,
    device: DeviceId,
    identity: DeviceIdentity,
    member: Option<MemberId>,
    website_role: Role,
    keys: StoredKeys,
    local: LocalState,
    now: Box<dyn Fn() -> u64 + Send + Sync>,
    entropy: EntropySource,
    counter: SendCounter,
    guard: ReplayGuard,
    /// The teammates' newest live state shown; kept across connections.
    live: LiveMarks,
    /// Marks rose since [`Engine::local`] last took them.
    marks_dirty: bool,
    /// When the marks last went into [`Engine::local`] (clock milliseconds).
    marks_stored_at: Option<u64>,
    /// Teammate devices set aside for a different history; kept across connections until the
    /// verified log changes.
    divergent: BTreeMap<Holder, Divergent>,
    cursor: ClientCursor,
    // Per connection.
    me: Option<Holder>,
    ops: HashMap<String, Op>,
    resyncing: bool,
    verifier: LogVerifier,
    /// A log fetch is running; another one is wanted after it.
    syncing: bool,
    sync_again: bool,
    /// The log was fetched and checked at least once on this connection.
    synced: bool,
    creating: bool,
    rotating: bool,
    key_get: Option<Epoch>,
    devices_get: bool,
    /// Wrapped keys from `key.rotated` for epochs above the verified current one, by epoch; at
    /// most [`MAX_INLINE`], dropped with every `hello`.
    inline: BTreeMap<Epoch, Ciphertext>,
    /// Devices this one confirmed, to wrap the current key to once the log lists them.
    wrap_queue: Vec<Holder>,
    pending: Vec<Pending>,
    status: TeamStatus,
    // Sharing (part 2b).
    share: LocalShare,
    /// A random session key per shared session, for this engine's lifetime.
    session_keys: BTreeMap<SessionId, SessionKey>,
    /// What went out per session in the last `claim.set` (hashes and detail), on this connection.
    sent_claims: HashMap<SessionId, Vec<u8>>,
    sent_presence: Option<Vec<u8>>,
    /// The epoch the shared state was sent under; a new one sends everything again.
    shared_epoch: Option<Epoch>,
    peers: BTreeMap<Holder, Peer>,
    overlaps: Overlaps,
    stashed: Vec<ServerMessage>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("project", &self.project)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

fn op_id() -> OpId {
    // A UUID v4 in simple form: 32 hex digits, within the op id alphabet and length.
    let text = uuid::Uuid::new_v4().simple().to_string();
    OpId::new(text).unwrap_or_else(|_| unreachable_op_id())
}

/// Documented invariant: a 32-digit hex string is a valid op id ([A-Za-z0-9_-], at most 64).
#[allow(clippy::expect_used)]
fn unreachable_op_id() -> OpId {
    OpId::new("op").expect("a fixed ASCII op id is valid")
}

/// A new random session key: a UUID v4 in simple form (32 hex digits, a valid key).
fn new_session_key() -> SessionKey {
    let text = uuid::Uuid::new_v4().simple().to_string();
    SessionKey::new(text).unwrap_or_else(|_| unreachable_session_key())
}

/// Documented invariant: a fixed ASCII key is valid ([A-Za-z0-9_-], at most 64).
#[allow(clippy::expect_used)]
fn unreachable_session_key() -> SessionKey {
    SessionKey::new("session").expect("a fixed ASCII session key is valid")
}

fn role_rank(role: Role) -> u8 {
    match role {
        Role::Owner => 0,
        Role::Admin => 1,
        Role::Member => 2,
    }
}

impl Engine {
    pub fn new(setup: EngineSetup) -> Self {
        let mut cursor = ClientCursor::default();
        if let Some(stored) = &setup.local.cursor {
            cursor.welcome(&stored.stream, stored.seq, true);
        }
        let marks = setup
            .local
            .marks
            .iter()
            .map(|m| ((m.holder.clone(), m.sign), m.counter))
            .collect();
        // A lost or restored state file must not make this device repeat a counter.
        let floor = crate::team::counter_floor((setup.now)());
        Self {
            verifier: LogVerifier::new(setup.team.clone()),
            counter: SendCounter::resume(setup.local.counter.max(floor)),
            guard: ReplayGuard::from_marks(marks),
            live: LiveMarks::from_local(&setup.local),
            marks_dirty: false,
            marks_stored_at: None,
            divergent: BTreeMap::new(),
            project: setup.project,
            team: setup.team,
            device: setup.device,
            identity: setup.identity,
            member: setup.member,
            website_role: setup.website_role,
            keys: setup.keys,
            local: setup.local,
            now: setup.now,
            entropy: setup.entropy,
            cursor,
            me: None,
            ops: HashMap::new(),
            resyncing: false,
            syncing: false,
            sync_again: false,
            synced: false,
            creating: false,
            rotating: false,
            key_get: None,
            devices_get: false,
            inline: BTreeMap::new(),
            wrap_queue: Vec::new(),
            pending: Vec::new(),
            status: TeamStatus::Connecting { retrying: false },
            share: LocalShare::default(),
            session_keys: BTreeMap::new(),
            sent_claims: HashMap::new(),
            sent_presence: None,
            shared_epoch: None,
            peers: BTreeMap::new(),
            overlaps: BTreeMap::new(),
            stashed: Vec::new(),
        }
    }

    /// The account this device acts for, once bound.
    pub fn member(&self) -> Option<&MemberId> {
        self.member.as_ref()
    }

    /// The keychain entry of this device, with its bound account.
    pub fn stored_device(&self) -> StoredDevice {
        StoredDevice {
            device: self.device.as_str().to_owned(),
            member: self.member.as_ref().map(|m| m.as_str().to_owned()),
            secret: self.identity.export(),
        }
    }

    /// The person's role on the website, read before every connection and after
    /// [`Effect::CheckWebsite`]; the relay's events never set it.
    pub fn set_website_role(&mut self, role: Role) -> Vec<Effect> {
        self.website_role = role;
        self.reconcile()
    }

    pub fn keys(&self) -> &StoredKeys {
        &self.keys
    }

    pub fn local(&self) -> &LocalState {
        &self.local
    }

    pub fn state(&self) -> Option<&TeamState> {
        self.verifier.state()
    }

    pub fn me(&self) -> Option<&Holder> {
        self.me.as_ref()
    }

    /// The window's status; `retrying` while not yet welcomed after a failure.
    pub fn status(&self, retrying: bool) -> TeamStatus {
        match &self.status {
            TeamStatus::Connecting { .. } => TeamStatus::Connecting { retrying },
            other => other.clone(),
        }
    }

    pub fn pending(&self) -> Vec<PendingConfirmation> {
        self.pending.iter().map(|p| p.view.clone()).collect()
    }

    fn frame(&mut self, op: Op, message: ClientMessage) -> Effect {
        let id = op_id();
        self.ops.insert(id.as_str().to_owned(), op);
        let team = (!matches!(message, ClientMessage::Ping)).then(|| self.team.clone());
        Effect::Send(ClientFrame { id, team, message })
    }

    fn now(&self) -> UnixMillis {
        UnixMillis((self.now)())
    }

    fn stop(reason: AttentionReason) -> Effect {
        Effect::End(SessionEnd::Stop(StopReason::Attention(reason)))
    }

    // -----------------------------------------------------------------------------------------
    // Connection
    // -----------------------------------------------------------------------------------------

    /// A new connection: forget the last one's ops and send `hello` with the stored cursor.
    pub fn hello(&mut self, ticket: Ticket) -> Vec<Effect> {
        self.me = None;
        self.ops.clear();
        self.resyncing = false;
        self.syncing = false;
        self.sync_again = false;
        self.synced = false;
        self.creating = false;
        self.rotating = false;
        self.key_get = None;
        self.devices_get = false;
        self.inline.clear();
        self.verifier = LogVerifier::new(self.team.clone());
        self.status = TeamStatus::Connecting { retrying: false };
        // The relay keeps no claims or presence of a closed connection: everything goes again,
        // and the others' presence arrives anew.
        self.sent_claims.clear();
        self.sent_presence = None;
        self.shared_epoch = None;
        self.peers.clear();
        self.overlaps.clear();
        self.stashed.clear();
        let hello = ClientMessage::Hello(Hello {
            ticket,
            device: self.device.clone(),
            proto: ProtoRange::CURRENT,
            last_seq: self.cursor.cursor().cloned(),
        });
        let mut effects = self.flush_marks();
        effects.push(self.frame(Op::Hello, hello));
        effects.push(Effect::Changed);
        effects
    }

    /// The live marks that rose since they were last stored; the worker calls it when a
    /// connection ends, so a restart does not forget them.
    pub fn flush_marks(&mut self) -> Vec<Effect> {
        if !self.marks_dirty {
            return Vec::new();
        }
        self.store_marks();
        vec![Effect::StoreLocal]
    }

    fn store_marks(&mut self) {
        self.live.store(&mut self.local);
        self.marks_dirty = false;
        self.marks_stored_at = Some((self.now)());
    }

    /// A mark rose: stored at once when the last store is [`MARKS_STORE_EVERY_MS`] ago (or the
    /// clock went back), otherwise left for the next ping or the end of the connection.
    fn marks_raised(&mut self) -> Vec<Effect> {
        let now = (self.now)();
        let due = self
            .marks_stored_at
            .is_none_or(|at| now < at || now - at >= MARKS_STORE_EVERY_MS);
        if due {
            self.store_marks();
            vec![Effect::StoreLocal]
        } else {
            self.marks_dirty = true;
            Vec::new()
        }
    }

    /// `ticket.renew` with a fresh website ticket, once welcomed.
    pub fn renew(&mut self, ticket: Ticket) -> Vec<Effect> {
        if self.me.is_none() {
            return Vec::new();
        }
        vec![self.frame(
            Op::Quiet,
            ClientMessage::TicketRenew(TicketRenew { ticket }),
        )]
    }

    /// A `ping`, and the live marks left from the last few seconds.
    pub fn ping(&mut self) -> Vec<Effect> {
        let mut effects = self.flush_marks();
        effects.push(self.frame(Op::Quiet, ClientMessage::Ping));
        effects
    }

    /// Asks the relay for devices that may wait for this device's confirmation.
    pub fn poll_devices(&mut self) -> Vec<Effect> {
        if !self.synced || self.devices_get || !self.in_log() {
            return Vec::new();
        }
        self.devices_get = true;
        vec![self.frame(Op::DevicesGet, ClientMessage::DevicesGet)]
    }

    fn in_log(&self) -> bool {
        match (self.verifier.state(), &self.me) {
            (Some(state), Some(me)) => state.device(me).is_some(),
            _ => false,
        }
    }

    /// One text frame from the relay.
    pub fn on_text(&mut self, text: &str) -> Vec<Effect> {
        match decode_server(text) {
            Ok(frame) => self.on_frame(frame),
            Err(error) if error.needs_update() => vec![Self::stop(AttentionReason::NeedsUpdate)],
            Err(error) => {
                eprintln!("hangar: team relay frame refused: {error}");
                vec![Effect::End(SessionEnd::Lost)]
            }
        }
    }

    fn on_frame(&mut self, frame: ServerFrame) -> Vec<Effect> {
        if frame.team.as_ref().is_some_and(|team| *team != self.team) {
            return Vec::new();
        }
        let op = frame
            .id
            .as_ref()
            .and_then(|id| self.ops.get(id.as_str()).cloned());
        let mut effects = Vec::new();
        if let Some(seq) = frame.seq {
            match self.cursor.observe(seq) {
                Observed::Duplicate => return effects,
                Observed::Gap { .. } => return vec![Effect::End(SessionEnd::Gap)],
                Observed::Next => {
                    self.local.cursor = self.cursor.cursor().cloned();
                    effects.push(Effect::StoreLocal);
                }
            }
        }
        if let Some(id) = &frame.id {
            if !matches!(op, Some(Op::Hello)) {
                self.ops.remove(id.as_str());
            }
        }
        match frame.message {
            ServerMessage::Welcome(welcome) => {
                if check_welcome(ProtoRange::CURRENT, &welcome).is_err() {
                    return vec![Self::stop(AttentionReason::NeedsUpdate)];
                }
                if let Some(id) = &frame.id {
                    self.ops.remove(id.as_str());
                }
                match &self.member {
                    // Another person signed in on this keychain: their account must not act as
                    // this device, and the device's keys stay as they are.
                    Some(bound) if *bound != welcome.member => {
                        eprintln!("hangar: the team device belongs to another account");
                        return vec![Self::stop(AttentionReason::OtherAccount)];
                    }
                    Some(_) => {}
                    None => {
                        self.member = Some(welcome.member.clone());
                        effects.push(Effect::StoreDevice);
                    }
                }
                self.cursor
                    .welcome(&welcome.stream, welcome.head, self.resyncing);
                self.resyncing = false;
                self.local.cursor = self.cursor.cursor().cloned();
                let me = Holder {
                    member: welcome.member,
                    device: self.device.clone(),
                };
                let keys = self.identity.device_keys(&self.team, &me);
                self.me = Some(me);
                effects.push(Effect::StoreLocal);
                effects.push(self.frame(Op::DeviceKeys, ClientMessage::DeviceKeys(keys)));
                effects.extend(self.sync_log());
            }
            ServerMessage::ResyncRequired(_) => {
                // The state events without `seq` that follow are not needed: the log is fetched
                // anew after the welcome that ends the snapshot.
                self.cursor.resync();
                self.resyncing = true;
            }
            ServerMessage::Err(err) => effects.extend(self.on_err(op, err.code)),
            ServerMessage::Ack(ack) => effects.extend(self.on_ack(op, ack.keys)),
            ServerMessage::MemberLogEntries(page) => effects.extend(self.on_entries(page)),
            ServerMessage::Devices(list) => {
                self.devices_get = false;
                self.update_pending(list.devices);
                effects.push(Effect::Changed);
            }
            ServerMessage::MemberLogAppended(_) => effects.extend(self.sync_log()),
            ServerMessage::KeyRotated(rotated) => {
                if let Some(data) = rotated.wrapped {
                    self.keep_inline(rotated.epoch, data);
                }
                effects.extend(self.sync_log());
            }
            ServerMessage::KeyWrapped(wrapped) => {
                if self.me.as_ref() == Some(&wrapped.holder) {
                    effects.extend(self.reconcile());
                }
            }
            // The relay's events are not signed: a removal only counts once the verified log
            // shows it (`after_sync`) or the website confirms it (`Effect::CheckWebsite`).
            ServerMessage::DeviceRemoved(_) => effects.extend(self.sync_log()),
            ServerMessage::MemberRemoved(removed) => {
                // The website first: the relay usually closes this connection right after, so
                // the log fetch may never be answered.
                if self
                    .me
                    .as_ref()
                    .is_some_and(|me| me.member == removed.member)
                {
                    effects.push(Effect::CheckWebsite);
                }
                effects.extend(self.sync_log());
            }
            ServerMessage::MemberRoleChanged(changed)
                if self
                    .me
                    .as_ref()
                    .is_some_and(|me| me.member == changed.member) =>
            {
                effects.push(Effect::CheckWebsite);
            }
            ServerMessage::PresenceUpdate(update) => effects.extend(self.on_presence(update, true)),
            ServerMessage::ClaimOverlap(overlap) => effects.extend(self.on_overlap(overlap, true)),
            // Reservations and notifications are not built yet.
            _ => {}
        }
        effects
    }

    fn on_err(&mut self, op: Option<Op>, code: ErrorCode) -> Vec<Effect> {
        match op {
            Some(Op::Hello) => match code {
                ErrorCode::Forbidden => vec![Effect::End(SessionEnd::HelloForbidden)],
                ErrorCode::OutdatedClient => vec![Self::stop(AttentionReason::NeedsUpdate)],
                _ => vec![Effect::End(SessionEnd::Lost)],
            },
            Some(Op::LogGet) => vec![Effect::End(SessionEnd::Lost)],
            Some(Op::DevicesGet) => {
                self.devices_get = false;
                Vec::new()
            }
            Some(Op::KeyGet(_)) => {
                // `notFound`: no key wrapped for this device yet; `key.wrapped` will say when.
                self.key_get = None;
                Vec::new()
            }
            Some(Op::Append(_)) => {
                // Another device appended first (`conflict`) or the relay refused: fetch the log
                // and decide again; a confirmation shows up again with the next device poll.
                eprintln!("hangar: team log entry refused ({code:?})");
                self.creating = false;
                self.sync_log()
            }
            Some(Op::Rotate(epoch)) => {
                // Another rotation came first: the unused secret goes, the log decides again.
                eprintln!("hangar: team key rotation refused ({code:?})");
                self.rotating = false;
                self.keys.epochs.remove(&epoch);
                let mut effects = vec![Effect::StoreKeys];
                effects.extend(self.sync_log());
                effects
            }
            Some(Op::Wrap) | Some(Op::Remove) => {
                eprintln!("hangar: team key or device change refused ({code:?})");
                self.sync_log()
            }
            Some(Op::DeviceKeys) | Some(Op::Quiet) | None => {
                eprintln!("hangar: team relay refused a request ({code:?})");
                Vec::new()
            }
        }
    }

    fn on_ack(
        &mut self,
        op: Option<Op>,
        keys: Option<Vec<hangar_team_protocol::ids::Sealed>>,
    ) -> Vec<Effect> {
        match op {
            Some(Op::KeyGet(epoch)) => {
                self.key_get = None;
                let packages: Vec<Ciphertext> = keys
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|sealed| sealed.epoch == epoch)
                    .map(|sealed| sealed.data)
                    .collect();
                self.accept_packages(epoch, &packages)
            }
            Some(Op::Append(confirmed)) => {
                self.creating = false;
                if let Some(holder) = confirmed {
                    self.wrap_queue.push(holder);
                }
                self.sync_log()
            }
            Some(Op::Rotate(_)) => {
                self.rotating = false;
                self.sync_log()
            }
            _ => Vec::new(),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Membership log
    // -----------------------------------------------------------------------------------------

    fn sync_log(&mut self) -> Vec<Effect> {
        if self.me.is_none() {
            return Vec::new();
        }
        if self.syncing {
            self.sync_again = true;
            return Vec::new();
        }
        self.syncing = true;
        let after = self.verifier.state().map_or(0, |s| s.head().index);
        vec![self.frame(
            Op::LogGet,
            ClientMessage::MemberLogGet(MemberLogGet { after }),
        )]
    }

    fn on_entries(&mut self, page: MemberLogEntries) -> Vec<Effect> {
        if page.more && page.entries.is_empty() {
            // "More" without a single entry would have this device ask forever.
            eprintln!("hangar: team membership log page without progress");
            return vec![Effect::End(SessionEnd::Lost)];
        }
        if let Err(error) = self.verifier.apply_all(&page.entries) {
            eprintln!("hangar: team membership log refused: {error}");
            return vec![Self::stop(AttentionReason::HistoryMismatch)];
        }
        if page.more {
            let after = self.verifier.state().map_or(0, |s| s.head().index);
            return vec![self.frame(
                Op::LogGet,
                ClientMessage::MemberLogGet(MemberLogGet { after }),
            )];
        }
        self.syncing = false;
        let mut effects = self.after_sync();
        if self.sync_again {
            self.sync_again = false;
            effects.extend(self.sync_log());
        }
        effects
    }

    /// The log is complete as far as the relay knows it.
    fn after_sync(&mut self) -> Vec<Effect> {
        let head = self.verifier.state().map(|s| *s.head());
        // What this device verified before must still be there, unchanged.
        if let Some(stored) = self.keys.head {
            let same = self
                .verifier
                .state()
                .and_then(|s| s.hash_at(stored.index))
                .is_some_and(|hash| hash == stored.hash);
            if !same {
                eprintln!("hangar: team history differs from the one verified before");
                return vec![Self::stop(AttentionReason::HistoryMismatch)];
            }
        }
        let mut effects = Vec::new();
        if !self.synced {
            effects.push(Effect::Settled);
        }
        self.synced = true;
        if let Some(head) = head {
            if self.keys.head != Some(head) {
                self.keys.head = Some(head);
                effects.push(Effect::StoreKeys);
            }
        }
        // A device set aside for a different history is heard again once the log moved on (it
        // may have caught up), or goes with its entry.
        let index = head.map(|h| h.index);
        let before = self.divergent.len();
        let state = self.verifier.state();
        self.divergent.retain(|holder, divergent| {
            Some(divergent.at) == index && state.is_some_and(|s| s.device(holder).is_some())
        });
        if self.divergent.len() != before {
            effects.push(Effect::Changed);
        }
        if let (Some(state), Some(me)) = (self.verifier.state(), &self.me) {
            let removed = state
                .key_periods(me)
                .last()
                .is_some_and(|p| p.removed_at.is_some());
            if removed && state.device(me).is_none() {
                return vec![
                    Effect::WipeDevice,
                    Effect::End(SessionEnd::Stop(StopReason::Removed)),
                ];
            }
        }
        effects.extend(self.reconcile());
        effects.extend(self.poll_devices());
        effects.extend(self.retry_stashed());
        effects
    }

    /// Decides the next step from the verified log and the secrets this device holds.
    fn reconcile(&mut self) -> Vec<Effect> {
        let Some(me) = self.me.clone() else {
            return Vec::new();
        };
        if !self.synced {
            return Vec::new();
        }
        let mut effects = vec![Effect::Changed];
        let waiting = TeamStatus::WaitingForConfirmation {
            safety_number: safety_groups(&safety_number(&me, &self.identity.public_keys())),
        };
        let Some(state) = self.verifier.state() else {
            if self.website_role == Role::Owner {
                if !self.creating {
                    effects.extend(self.create_team(&me));
                }
                self.status = TeamStatus::Connecting { retrying: false };
            } else {
                self.status = waiting;
            }
            return effects;
        };
        if state.device(&me).is_none() {
            self.status = waiting;
            return effects;
        }
        let role = state.effective_role(&me.member, self.website_role);
        let manager = role.is_some_and(|r| role_rank(r) <= role_rank(Role::Admin));
        let current = state.current_epoch().map(|e| e.epoch);
        if manager && !self.rotating && (current.is_none() || state.rotation_due()) {
            effects.extend(self.rotate(&me));
            self.status = TeamStatus::Connecting { retrying: false };
            return effects;
        }
        let Some(epoch) = current else {
            // The owner's device creates the first epoch.
            self.status = waiting;
            return effects;
        };
        // A secret that does not match the log's commitment (this device's own plan of a
        // rotation another device won) is not the epoch's: it goes.
        let stale = match (state.epoch(epoch), self.keys.epochs.get(&epoch)) {
            (Some(record), Some(secret)) => !secret
                .commitment(&self.team, epoch)
                .matches(&record.commitment),
            _ => false,
        };
        if stale {
            self.keys.epochs.remove(&epoch);
            effects.push(Effect::StoreKeys);
        }
        if !self.keys.epochs.contains_key(&epoch) {
            if let Some(data) = self.inline.remove(&epoch) {
                effects.extend(self.accept_packages(epoch, &[data]));
                if self.keys.epochs.contains_key(&epoch) {
                    return effects;
                }
            }
            if self.key_get.is_none() {
                self.key_get = Some(epoch);
                effects
                    .push(self.frame(Op::KeyGet(epoch), ClientMessage::KeyGet(KeyGet { epoch })));
            }
            self.status = waiting;
            return effects;
        }
        effects.extend(self.wrap_queued());
        let Some(state) = self.verifier.state() else {
            return effects;
        };
        self.status = TeamStatus::Connected {
            epoch: epoch.0,
            members: state.members().len(),
            devices: state.device_count(),
        };
        if self.shared_epoch != Some(epoch) {
            effects.extend(self.share_now());
        }
        effects
    }

    fn create_team(&mut self, me: &Holder) -> Vec<Effect> {
        let body = EntryBody::create_team(
            self.team.clone(),
            me.clone(),
            self.now(),
            vec![DeviceEntry::new(&self.team, me, &self.identity, false)],
        );
        match body.sign(&self.identity) {
            Ok(append) => {
                self.creating = true;
                vec![self.frame(Op::Append(None), ClientMessage::MemberLogAppend(append))]
            }
            Err(error) => {
                eprintln!("hangar: team not created: {error}");
                vec![Self::stop(AttentionReason::Refused)]
            }
        }
    }

    /// A new epoch for every device; the secret is stored before `key.rotate` leaves.
    fn rotate(&mut self, me: &Holder) -> Vec<Effect> {
        let Some(state) = self.verifier.state() else {
            return Vec::new();
        };
        let now = self.now();
        let planned = plan_rotation(
            state,
            &self.identity,
            me,
            None,
            now,
            &mut Lend(self.entropy.as_mut()),
        );
        let rotation = match planned {
            Ok(rotation) => rotation,
            Err(error) => {
                eprintln!("hangar: team key rotation not planned: {error}");
                return Vec::new();
            }
        };
        for (holder, error) in &rotation.failed {
            // Handed the key later with `key.wrap` (rotation.rs, WrapOutcome::failed).
            eprintln!(
                "hangar: team key not wrapped for a device ({}): {error}",
                holder.device.as_str()
            );
        }
        self.wrap_queue
            .extend(rotation.failed.iter().map(|(holder, _)| holder.clone()));
        self.rotating = true;
        self.keys.epochs.insert(rotation.epoch, rotation.secret);
        self.keys.trim();
        vec![
            Effect::StoreKeys,
            self.frame(
                Op::Rotate(rotation.epoch),
                ClientMessage::KeyRotate(rotation.rotate),
            ),
        ]
    }

    fn accept_packages(&mut self, epoch: Epoch, packages: &[Ciphertext]) -> Vec<Effect> {
        let (Some(state), Some(me)) = (self.verifier.state(), self.me.as_ref()) else {
            return Vec::new();
        };
        if packages.is_empty() {
            return Vec::new();
        }
        match state.unwrap_any(&self.identity, me, epoch, packages) {
            Ok(secret) => {
                self.keys.epochs.insert(epoch, secret);
                self.keys.trim();
                let mut effects = vec![Effect::StoreKeys];
                effects.extend(self.reconcile());
                effects
            }
            Err(error) => {
                eprintln!("hangar: team key not accepted: {error}");
                Vec::new()
            }
        }
    }

    /// Keeps a wrapped key from `key.rotated` for an epoch above the verified current one.
    fn keep_inline(&mut self, epoch: Epoch, data: Ciphertext) {
        let current = self
            .verifier
            .state()
            .and_then(|s| s.current_epoch())
            .map_or(0, |e| e.epoch.0);
        if epoch.0 <= current {
            return;
        }
        self.inline.insert(epoch, data);
        while self.inline.len() > MAX_INLINE {
            if self.inline.pop_first().is_none() {
                break;
            }
        }
    }

    fn wrap_queued(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        let queue = std::mem::take(&mut self.wrap_queue);
        for holder in queue {
            let current = self.verifier.state().and_then(|state| {
                let epoch = state.current_epoch()?.epoch;
                Some((state, self.keys.epochs.get(&epoch)?))
            });
            let Some((state, secret)) = current else {
                self.wrap_queue.push(holder);
                continue;
            };
            if state.device(&holder).is_none() {
                // Not in the log (yet, or any more): nothing to wrap to.
                continue;
            }
            match wrap_for_device(state, secret, &holder, &mut Lend(self.entropy.as_mut())) {
                Ok(wrap) => effects.push(self.frame(Op::Wrap, ClientMessage::KeyWrap(wrap))),
                Err(error) => eprintln!("hangar: team key not wrapped for a device: {error}"),
            }
        }
        effects
    }

    // -----------------------------------------------------------------------------------------
    // Confirmations
    // -----------------------------------------------------------------------------------------

    fn pending_id(&self, holder: &Holder, keys: &DevicePublicKeys) -> String {
        let mut sha = Sha256::new();
        for part in [
            b"hangar-team/app/confirm".as_slice(),
            self.team.as_str().as_bytes(),
            holder.member.as_str().as_bytes(),
            holder.device.as_str().as_bytes(),
            keys.sign.as_bytes(),
            keys.kem.as_bytes(),
        ] {
            sha.update(u32::try_from(part.len()).unwrap_or(u32::MAX).to_be_bytes());
            sha.update(part);
        }
        hex(&sha.finalize()[..16])
    }

    /// The entry confirming `info`, if this device may and the log would take it.
    fn confirmation_body(&self, info: &DeviceInfo) -> Option<(PendingKind, EntryBody)> {
        let state = self.verifier.state()?;
        let me = self.me.as_ref()?;
        state.device(me)?;
        if state.device(&info.holder).is_some() || info.holder == *me {
            return None;
        }
        let keys = DevicePublicKeys {
            sign: info.sign,
            kem: info.kem,
        };
        let entry = DeviceEntry::from_published(info.holder.device.clone(), keys, info.proof);
        let (kind, entry_kind) = if info.holder.member == me.member {
            let endorsement = Endorsement {
                device: me.device.clone(),
                signature: self
                    .identity
                    .endorse(&self.team, &info.holder, &keys, false),
            };
            (
                PendingKind::OwnDevice,
                EntryKind::AddDevice {
                    member: me.member.clone(),
                    device: entry,
                    endorsement,
                },
            )
        } else {
            let role = state.effective_role(&me.member, self.website_role)?;
            let new_member = state.member(&info.holder.member).is_none();
            if !new_member || role_rank(role) > role_rank(Role::Admin) {
                return None;
            }
            (
                PendingKind::NewMember,
                EntryKind::AddMember {
                    member: info.holder.member.clone(),
                    role: Role::Member,
                    devices: vec![entry],
                },
            )
        };
        let body = state.next_body(me.clone(), self.now(), entry_kind);
        state.check(&body).ok()?;
        Some((kind, body))
    }

    fn update_pending(&mut self, devices: Vec<DeviceInfo>) {
        let mut pending = Vec::new();
        for info in devices {
            let Some((kind, _)) = self.confirmation_body(&info) else {
                continue;
            };
            let keys = DevicePublicKeys {
                sign: info.sign,
                kem: info.kem,
            };
            let id = self.pending_id(&info.holder, &keys);
            if self.local.rejected.contains(&id) {
                continue;
            }
            let readmitted = kind == PendingKind::NewMember
                && self
                    .verifier
                    .state()
                    .is_some_and(|s| s.departed_at(&info.holder.member).is_some());
            pending.push(Pending {
                view: PendingConfirmation {
                    id,
                    project_id: self.project.clone(),
                    member: info.holder.member.as_str().to_owned(),
                    device: info.holder.device.as_str().to_owned(),
                    kind,
                    safety_number: safety_groups(&safety_number(&info.holder, &keys)),
                    readmitted,
                },
                info,
            });
        }
        self.pending = pending;
    }

    pub fn has_request(&self, id: &str) -> bool {
        self.pending.iter().any(|p| p.view.id == id)
    }

    /// Confirms a waiting device: the add device or add member entry, then (after the relay
    /// took it) the current key wrapped to the device.
    pub fn confirm(&mut self, id: &str) -> Result<Vec<Effect>, TeamError> {
        let at = self
            .pending
            .iter()
            .position(|p| p.view.id == id)
            .ok_or(TeamError::UnknownRequest)?;
        if self.syncing || self.creating || self.rotating {
            return Err(TeamError::Busy);
        }
        let info = self.pending[at].info.clone();
        let (_, body) = self
            .confirmation_body(&info)
            .ok_or(TeamError::UnknownRequest)?;
        let append = body
            .sign(&self.identity)
            .map_err(|_| TeamError::UnknownRequest)?;
        self.pending.remove(at);
        Ok(vec![
            self.frame(
                Op::Append(Some(info.holder)),
                ClientMessage::MemberLogAppend(append),
            ),
            Effect::Changed,
        ])
    }

    /// Hides a waiting device for good (remembered in the state file).
    pub fn reject(&mut self, id: &str) -> Result<Vec<Effect>, TeamError> {
        let at = self
            .pending
            .iter()
            .position(|p| p.view.id == id)
            .ok_or(TeamError::UnknownRequest)?;
        self.pending.remove(at);
        self.local.reject(id);
        Ok(vec![Effect::StoreLocal, Effect::Changed])
    }

    /// Removes another device from the team (`device.remove` with its signed entry). Not a
    /// command yet (step F part 2b); a rotation follows on an owner's or admin's device.
    pub fn remove_device(&mut self, holder: &Holder) -> Result<Vec<Effect>, TeamError> {
        let body = self.removal_body(EntryKind::RemoveDevice {
            holder: holder.clone(),
        })?;
        let log = body
            .sign(&self.identity)
            .map_err(|_| TeamError::UnknownRequest)?;
        let remove = DeviceRemove {
            holder: holder.clone(),
            log,
        };
        Ok(vec![
            self.frame(Op::Remove, ClientMessage::DeviceRemove(remove))
        ])
    }

    /// Removes a member with all its devices (owner or admin). Not a command yet.
    pub fn remove_member(&mut self, member: &MemberId) -> Result<Vec<Effect>, TeamError> {
        let body = self.removal_body(EntryKind::RemoveMember {
            member: member.clone(),
        })?;
        let append = body
            .sign(&self.identity)
            .map_err(|_| TeamError::UnknownRequest)?;
        Ok(vec![self.frame(
            Op::Append(None),
            ClientMessage::MemberLogAppend(append),
        )])
    }

    fn removal_body(&self, kind: EntryKind) -> Result<EntryBody, TeamError> {
        let (Some(state), Some(me)) = (self.verifier.state(), self.me.as_ref()) else {
            return Err(TeamError::Busy);
        };
        let body = state.next_body(me.clone(), self.now(), kind);
        state.check(&body).map_err(|_| TeamError::UnknownRequest)?;
        Ok(body)
    }

    // -----------------------------------------------------------------------------------------
    // Sharing (part 2b)
    // -----------------------------------------------------------------------------------------

    /// What this device shares with the project now; sent at once while connected.
    pub fn set_share(&mut self, share: LocalShare) -> Vec<Effect> {
        if share == self.share {
            return Vec::new();
        }
        self.share = share;
        let mut effects = self.share_now();
        effects.push(Effect::Changed);
        effects
    }

    /// The current epoch's cipher and this device's verified head, while connected.
    fn current_cipher(&self) -> Option<(EpochCipher, ChainHead, Epoch)> {
        let TeamStatus::Connected { epoch, .. } = self.status else {
            return None;
        };
        let epoch = Epoch(epoch);
        let state = self.verifier.state()?;
        let secret = self.keys.epochs.get(&epoch)?;
        match EpochCipher::for_epoch(state, secret, epoch) {
            Ok(cipher) => Some((cipher, *state.head(), epoch)),
            Err(error) => {
                eprintln!("hangar: team cipher not set up: {error}");
                None
            }
        }
    }

    /// Seals `plaintext` with the next record counter; the counter is stored before the frame
    /// that carries the record leaves (the caller puts [`Effect::StoreLocal`] first).
    fn seal(
        &mut self,
        cipher: &EpochCipher,
        head: &ChainHead,
        kind: RecordKind,
        record: &[u8],
        plaintext: &[u8],
    ) -> Option<Sealed> {
        let me = self.me.clone()?;
        let counter = self.counter.take()?;
        self.local.counter = self.counter.last();
        let context = RecordContext {
            kind,
            record,
            sender: &me,
        };
        match cipher.seal(
            &context,
            &self.identity,
            head,
            counter,
            plaintext,
            &mut Lend(self.entropy.as_mut()),
        ) {
            Ok(sealed) => Some(sealed),
            Err(error) => {
                eprintln!("hangar: team record not sealed: {error}");
                None
            }
        }
    }

    /// Sends what changed since the last send on this connection: the presence status, a
    /// `claim.set` per session whose claims changed, a `claim.clear` per session that ended or
    /// changes nothing any more. The counters are stored first.
    fn share_now(&mut self) -> Vec<Effect> {
        let Some((cipher, head, epoch)) = self.current_cipher() else {
            return Vec::new();
        };
        if self.shared_epoch != Some(epoch) {
            self.sent_claims.clear();
            self.sent_presence = None;
            self.shared_epoch = Some(epoch);
        }
        for session in &self.share.sessions {
            self.session_keys
                .entry(session.id)
                .or_insert_with(new_session_key);
        }
        let mut sends = Vec::new();
        let mut sealed_any = false;

        let keys: BTreeMap<SessionId, String> = self
            .session_keys
            .iter()
            .map(|(id, key)| (*id, key.as_str().to_owned()))
            .collect();
        let status = team_share::status_plaintext(
            &self.share,
            &keys,
            RecordKind::PresenceStatus.max_plaintext(),
        );
        if self.sent_presence.as_ref() != Some(&status) {
            if let Some(sealed) =
                self.seal(&cipher, &head, RecordKind::PresenceStatus, b"", &status)
            {
                sealed_any = true;
                let busy = self
                    .share
                    .sessions
                    .iter()
                    .any(|s| s.state != SessionState::Idle);
                let state = if busy {
                    PresenceState::Active
                } else {
                    PresenceState::Idle
                };
                sends.push(ClientMessage::PresenceSet(PresenceSet {
                    state,
                    status: Some(sealed),
                }));
                self.sent_presence = Some(status);
            }
        }

        let sessions = self.share.sessions.clone();
        for session in &sessions {
            let Some(key) = self.session_keys.get(&session.id).cloned() else {
                continue;
            };
            if session.claims.is_empty() {
                if self.sent_claims.remove(&session.id).is_some() {
                    sends.push(ClientMessage::ClaimClear(ClaimClear { session: key }));
                }
                continue;
            }
            let Ok(repo) = RepoId::new(&session.repo) else {
                continue;
            };
            let (repo_hash, files, left_out) =
                team_share::claim_hashes(cipher.match_key(), &repo, &session.claims);
            let detail = team_share::detail_plaintext(
                &self.share.person,
                session,
                RecordKind::ClaimDetail.max_plaintext(),
            );
            let mut digest = Sha256::new();
            digest.update(repo_hash.as_bytes());
            for file in &files {
                digest.update(file.area.map_or([0; 32], |a| *a.as_bytes()));
                digest.update(file.file.as_bytes());
            }
            digest.update(&detail);
            let digest = digest.finalize().to_vec();
            if self.sent_claims.get(&session.id) == Some(&digest) {
                continue;
            }
            let Some(sealed) = self.seal(
                &cipher,
                &head,
                RecordKind::ClaimDetail,
                key.as_str().as_bytes(),
                &detail,
            ) else {
                continue;
            };
            sealed_any = true;
            sends.push(ClientMessage::ClaimSet(ClaimSet {
                session: key,
                repo: repo_hash,
                epoch,
                files,
                truncated: left_out || session.truncated,
                detail: Some(sealed),
            }));
            self.sent_claims.insert(session.id, digest);
        }

        let ended: Vec<SessionId> = self
            .session_keys
            .keys()
            .filter(|id| !sessions.iter().any(|s| s.id == **id))
            .copied()
            .collect();
        for id in ended {
            if let Some(key) = self.session_keys.remove(&id) {
                if self.sent_claims.remove(&id).is_some() {
                    sends.push(ClientMessage::ClaimClear(ClaimClear { session: key }));
                }
            }
            self.overlaps.remove(&id);
        }

        let mut effects = Vec::new();
        if sealed_any {
            effects.push(Effect::StoreLocal);
        }
        for message in sends {
            effects.push(self.frame(Op::Quiet, message));
        }
        effects
    }

    /// Opens a teammate's sealed record against the verified log.
    fn open_sealed(
        &self,
        kind: RecordKind,
        record: &[u8],
        sender: &Holder,
        sealed: &Sealed,
    ) -> Open {
        let Some(state) = self.verifier.state() else {
            return Open::Later;
        };
        let current = state.current_epoch().map_or(0, |e| e.epoch.0);
        if sealed.epoch.0 < current {
            // Live state is sealed under the current epoch; an older one is from before a
            // rotation (perhaps by a device removed since) and is not shown.
            return Open::Failed;
        }
        let Some(secret) = self.keys.epochs.get(&sealed.epoch) else {
            // An epoch this device has no key for yet (a rotation under way) comes later.
            return if sealed.epoch.0 > current {
                Open::Later
            } else {
                Open::Failed
            };
        };
        let cipher = match EpochCipher::for_epoch(state, secret, sealed.epoch) {
            Ok(cipher) => cipher,
            Err(_) => return Open::Failed,
        };
        let context = RecordContext {
            kind,
            record,
            sender,
        };
        match cipher.open_verified(state, &context, sealed) {
            Ok(opened) => Open::Opened(opened),
            Err(OpenError::AheadOfLog { .. }) => Open::Later,
            Err(OpenError::Fork { .. }) => Open::Fork,
            Err(error) => {
                eprintln!("hangar: team record not opened: {error}");
                Open::Failed
            }
        }
    }

    /// Keeps a live record for one retry after the next log fetch.
    fn stash(&mut self, message: ServerMessage) -> Vec<Effect> {
        if self.stashed.len() < MAX_STASHED {
            self.stashed.push(message);
        }
        self.sync_log()
    }

    /// Whether the verified log vouches for `holder` now. The relay names the holder of every
    /// presence and overlap; a device it made up, or one removed since, never shows as a teammate.
    fn listed(&self, holder: &Holder) -> bool {
        self.verifier
            .state()
            .is_some_and(|state| state.device(holder).is_some())
    }

    /// Keeps the overlaps `keep` accepts; `true` when any went.
    fn retain_overlaps(&mut self, mut keep: impl FnMut(&SessionRef) -> bool) -> bool {
        let mut changed = false;
        self.overlaps.retain(|_, others| {
            let before = others.len();
            others.retain(|other, _| keep(other));
            changed |= others.len() != before;
            !others.is_empty()
        });
        changed
    }

    fn remove_overlap(&mut self, session: SessionId, other: &SessionRef) -> bool {
        let Some(others) = self.overlaps.get_mut(&session) else {
            return false;
        };
        let removed = others.remove(other).is_some();
        if others.is_empty() {
            self.overlaps.remove(&session);
        }
        removed
    }

    fn retry_stashed(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        // The log changed: devices it no longer vouches for leave the view.
        let unlisted: BTreeSet<Holder> = self
            .peers
            .keys()
            .chain(
                self.overlaps
                    .values()
                    .flat_map(|others| others.keys().map(|other| &other.holder)),
            )
            .filter(|holder| !self.listed(holder))
            .cloned()
            .collect();
        let peers = self.peers.len();
        self.peers.retain(|holder, _| !unlisted.contains(holder));
        let overlaps = self.retain_overlaps(|other| !unlisted.contains(&other.holder));
        if peers != self.peers.len() || overlaps {
            effects.push(Effect::Changed);
        }
        // Marks of devices the log never listed go too: they bound the state file. A device the
        // log removed keeps its marks, so the same device id admitted again before the next
        // rotation cannot have its old records shown as new.
        if let Some(state) = self.verifier.state() {
            if self
                .live
                .retain(|holder| !state.key_periods(holder).is_empty())
            {
                self.store_marks();
                effects.push(Effect::StoreLocal);
            }
        }
        for message in std::mem::take(&mut self.stashed) {
            match message {
                ServerMessage::PresenceUpdate(update) => {
                    effects.extend(self.on_presence(update, false));
                }
                ServerMessage::ClaimOverlap(overlap) => {
                    effects.extend(self.on_overlap(overlap, false));
                }
                _ => {}
            }
        }
        effects
    }

    /// A teammate device's presence. `first`: not a retry of a stashed one.
    fn on_presence(&mut self, update: PresenceUpdate, first: bool) -> Vec<Effect> {
        if self.me.as_ref() == Some(&update.holder) || self.divergent.contains_key(&update.holder) {
            return Vec::new();
        }
        if update.state == PresenceState::Offline {
            let had = self.peers.remove(&update.holder).is_some();
            let overlaps = self.retain_overlaps(|other| other.holder != update.holder);
            return if had || overlaps {
                vec![Effect::Changed]
            } else {
                Vec::new()
            };
        }
        if !self.listed(&update.holder) {
            // Perhaps a device added after this device's last log fetch: fetch, try once more.
            return if first {
                self.stash(ServerMessage::PresenceUpdate(update))
            } else {
                Vec::new()
            };
        }
        let previous = self.peers.get(&update.holder);
        let mut status = previous.map(|p| p.status.clone()).unwrap_or_default();
        let mut signed = previous.is_some_and(|p| p.signed);
        let mut effects = Vec::new();
        // No status: the relay's word on the state alone (active, idle, away); the sessions stay
        // as the newest signed status listed them. Only `offline` ends them.
        if let Some(sealed) = &update.status {
            match self.open_sealed(RecordKind::PresenceStatus, b"", &update.holder, sealed) {
                Open::Opened(opened) => {
                    match self.live.presence(&update.holder, opened.counter) {
                        // An old status shown again (after `offline`, a reconnect or a
                        // restart): the newer one stays.
                        Mark::Stale => return Vec::new(),
                        Mark::Seen => {}
                        Mark::Raised => effects.extend(self.marks_raised()),
                    }
                    match team_share::parse_status(&opened.plaintext) {
                        Some(parsed) => {
                            // Overlaps of sessions this status no longer lists have ended.
                            let ended = {
                                let keys: BTreeSet<&str> =
                                    parsed.sessions.iter().map(|s| s.key.as_str()).collect();
                                self.retain_overlaps(|other| {
                                    other.holder != update.holder
                                        || keys.contains(other.session.as_str())
                                })
                            };
                            if ended {
                                effects.push(Effect::Changed);
                            }
                            status = parsed;
                            signed = true;
                        }
                        None => eprintln!("hangar: a teammate's status was not readable"),
                    }
                }
                Open::Later if first => {
                    return self.stash(ServerMessage::PresenceUpdate(update));
                }
                // Only a record the listed sender signed reports a fork (`open_verified`).
                Open::Fork => return self.diverged(&update.holder),
                Open::Later | Open::Failed => {}
            }
        }
        // At most one entry per device the log lists (`listed` above keeps out the others).
        if !self.peers.contains_key(&update.holder) && self.peers.len() >= self.device_count() {
            return effects;
        }
        self.peers.insert(
            update.holder,
            Peer {
                presence: update.state,
                status,
                signed,
            },
        );
        effects.push(Effect::Changed);
        effects
    }

    /// The devices the verified log lists now.
    fn device_count(&self) -> usize {
        self.verifier.state().map_or(0, TeamState::device_count)
    }

    /// The relay's word that one of this device's sessions shares files (or an area) with a
    /// teammate's session, with the teammate's sealed detail at the file level.
    fn on_overlap(&mut self, overlap: ClaimOverlap, first: bool) -> Vec<Effect> {
        let Some(session) = self
            .session_keys
            .iter()
            .find(|(_, key)| **key == overlap.session)
            .map(|(id, _)| *id)
        else {
            return Vec::new();
        };
        if self.divergent.contains_key(&overlap.other.holder) {
            return Vec::new();
        }
        if overlap.level == MatchLevel::None {
            return if self.remove_overlap(session, &overlap.other) {
                vec![Effect::Changed]
            } else {
                Vec::new()
            };
        }
        if !self.listed(&overlap.other.holder) {
            return if first {
                self.stash(ServerMessage::ClaimOverlap(overlap))
            } else {
                Vec::new()
            };
        }
        // The teammate's newest signed status names its sessions: an overlap of any other session
        // key (ended, or made up by the relay) does not count.
        let signed = self
            .peers
            .get(&overlap.other.holder)
            .filter(|peer| peer.signed);
        let unlisted = signed.is_some_and(|peer| {
            !peer
                .status
                .sessions
                .iter()
                .any(|s| s.key == overlap.other.session.as_str())
        });
        if unlisted {
            return if self.remove_overlap(session, &overlap.other) {
                vec![Effect::Changed]
            } else {
                Vec::new()
            };
        }
        let known = self
            .overlaps
            .get(&session)
            .and_then(|others| others.get(&overlap.other));
        if known.is_none() {
            let cap = self.device_count().saturating_mul(MAX_OVERLAPS_PER_DEVICE);
            let held = self.overlaps.get(&session).map_or(0, BTreeMap::len);
            if held >= cap {
                return Vec::new();
            }
        }
        let mut effects = Vec::new();
        let mut detail = known.and_then(|e| e.detail.clone());
        if let Some(sealed) = &overlap.detail {
            let record = overlap.other.session.as_str().as_bytes();
            match self.open_sealed(
                RecordKind::ClaimDetail,
                record,
                &overlap.other.holder,
                sealed,
            ) {
                Open::Opened(opened) => {
                    let mark = self.live.detail(
                        &overlap.other.holder,
                        overlap.other.session.as_str(),
                        opened.counter,
                    );
                    if mark == Mark::Raised {
                        effects.extend(self.marks_raised());
                    }
                    // An older detail shown again keeps the newer one.
                    if mark != Mark::Stale {
                        if let Some(parsed) = team_share::parse_detail(&opened.plaintext) {
                            detail = Some(parsed);
                        }
                    }
                }
                Open::Later if first => {
                    return self.stash(ServerMessage::ClaimOverlap(overlap));
                }
                Open::Fork => return self.diverged(&overlap.other.holder),
                Open::Later | Open::Failed => {}
            }
        }
        let hint = overlap
            .file
            .and_then(|file| self.path_of_hash(session, &file));
        self.overlaps.entry(session).or_default().insert(
            overlap.other,
            OverlapEntry {
                level: overlap.level,
                files: overlap.files,
                hint,
                detail,
            },
        );
        effects.push(Effect::Changed);
        effects
    }

    /// `holder` signed a live record naming another log at an index this device verified: it
    /// (or the relay towards it) sees a different team history. Only that device is set aside,
    /// with a notice in the Team view, until this device's verified log changes; its marks stay,
    /// since they only refuse records older than ones verified against this device's log.
    fn diverged(&mut self, holder: &Holder) -> Vec<Effect> {
        eprintln!("hangar: a team device reports a different membership history");
        let at = self.verifier.state().map_or(0, |s| s.head().index);
        let name = self
            .peers
            .remove(holder)
            .map(|peer| peer.status.name)
            .unwrap_or_default();
        self.retain_overlaps(|other| other.holder != *holder);
        self.divergent
            .insert(holder.clone(), Divergent { at, name });
        vec![Effect::Changed]
    }

    /// This device's spelling of a file the relay names by its keyed hash.
    fn path_of_hash(&self, session: SessionId, file: &KeyedHash) -> Option<String> {
        let mine = self.share.sessions.iter().find(|s| s.id == session)?;
        let repo = RepoId::new(&mine.repo).ok()?;
        let (cipher, _, _) = self.current_cipher()?;
        team_share::path_for_hash(cipher.match_key(), &repo, &mine.claims, file)
    }

    /// The teammates as the Team view shows them (the linked repositories are the service's).
    pub fn activity(&self) -> TeamActivity {
        let me = self.me.as_ref().map(|m| &m.member);
        let peers = self
            .peers
            .iter()
            .map(|(holder, peer)| PeerView {
                member: holder.member.as_str().to_owned(),
                device: holder.device.as_str().to_owned(),
                name: peer.status.name.clone(),
                own: Some(&holder.member) == me,
                presence: match peer.presence {
                    PresenceState::Active => "active",
                    PresenceState::Away => "away",
                    PresenceState::Idle | PresenceState::Offline => "idle",
                }
                .to_owned(),
                sessions: peer.status.sessions.clone(),
            })
            .collect();
        let divergent = self
            .divergent
            .iter()
            .map(|(holder, divergent)| DivergentDevice {
                member: holder.member.as_str().to_owned(),
                device: holder.device.as_str().to_owned(),
                name: divergent.name.clone(),
            })
            .collect();
        TeamActivity {
            linked: Vec::new(),
            peers,
            overlaps: self.teammate_overlaps(),
            divergent,
        }
    }

    fn teammate_overlaps(&self) -> Vec<TeammateOverlap> {
        let mut list: Vec<TeammateOverlap> = self
            .overlaps
            .iter()
            .flat_map(|(session, others)| others.iter().map(move |entry| (*session, entry)))
            .filter_map(|(session, (other, entry))| {
                let mine = self.share.sessions.iter().find(|s| s.id == session)?;
                let peer = self.peers.get(&other.holder);
                let signed = peer.filter(|p| p.signed);
                let peer_session = signed.and_then(|p| {
                    p.status
                        .sessions
                        .iter()
                        .find(|s| s.key == other.session.as_str())
                });
                // A teammate whose newest signed status does not list the session has ended it
                // (or the relay made the key up).
                if signed.is_some() && peer_session.is_none() {
                    return None;
                }
                let (name, agent, title) = match &entry.detail {
                    Some(d) => (d.name.clone(), d.agent.clone(), d.title.clone()),
                    None => (
                        peer.map(|p| p.status.name.clone()).unwrap_or_default(),
                        peer_session.map(|s| s.agent.clone()).unwrap_or_default(),
                        peer_session.map(|s| s.title.clone()).unwrap_or_default(),
                    ),
                };
                let refined = entry
                    .detail
                    .as_ref()
                    .and_then(|detail| team_share::refine(mine, detail));
                let (level, path, lines, files) = match (refined, &entry.detail) {
                    (Some(r), _) => (r.level, Some(r.path), r.lines, r.files),
                    // The teammate's own signed detail names no file of ours: the relay's word
                    // of a shared file does not outweigh it, at most the same area remains.
                    (None, Some(_)) => match entry.level {
                        MatchLevel::None => return None,
                        MatchLevel::SameArea | MatchLevel::SameFile => {
                            (OverlapLevel::SameArea, None, Vec::new(), 0)
                        }
                    },
                    (None, None) => match entry.level {
                        MatchLevel::SameFile => (
                            OverlapLevel::SameFile,
                            entry.hint.clone(),
                            Vec::new(),
                            entry.files,
                        ),
                        MatchLevel::SameArea => (OverlapLevel::SameArea, None, Vec::new(), 0),
                        MatchLevel::None => return None,
                    },
                };
                // Without a status from that device nothing vouches that the session still runs
                // (a replayed detail, or a key the relay made up): the same area at most, which
                // raises no notice.
                let (level, path, lines, files) = if peer_session.is_none() {
                    (level.min(OverlapLevel::SameArea), None, Vec::new(), 0)
                } else {
                    (level, path, lines, files)
                };
                if level == OverlapLevel::None {
                    return None;
                }
                Some(TeammateOverlap {
                    session_id: session,
                    member: other.holder.member.as_str().to_owned(),
                    device: other.holder.device.as_str().to_owned(),
                    other_session: other.session.as_str().to_owned(),
                    name,
                    agent,
                    title,
                    level,
                    path,
                    lines,
                    files,
                })
            })
            .collect();
        list.sort_by(|a, b| {
            (
                a.session_id,
                std::cmp::Reverse(a.level),
                &a.path,
                &a.member,
                &a.device,
                &a.other_session,
            )
                .cmp(&(
                    b.session_id,
                    std::cmp::Reverse(b.level),
                    &b.path,
                    &b.member,
                    &b.device,
                    &b.other_session,
                ))
        });
        list
    }

    /// Every file a teammate's session is changing, as the presence lists it, with the lines
    /// where a claim detail reached this device: what the agent gate and the agent's context
    /// read.
    pub fn teammate_files(&self) -> Vec<TeammateFile> {
        // Each teammate session's detail once, its paths normalized once (the first detail per
        // session, as several of this device's sessions may overlap with it).
        let mut details: HashMap<(&Holder, &str), HashMap<String, &[LineRange]>> = HashMap::new();
        for (other, entry) in self.overlaps.values().flatten() {
            let Some(detail) = &entry.detail else {
                continue;
            };
            details
                .entry((&other.holder, other.session.as_str()))
                .or_insert_with(|| {
                    let mut by_path = HashMap::new();
                    for claim in &detail.claims {
                        if let Some(key) = team_share::team_path(&claim.path) {
                            by_path.entry(key).or_insert(claim.ranges.as_slice());
                        }
                    }
                    by_path
                });
        }
        let mut files = Vec::new();
        for (holder, peer) in &self.peers {
            for session in &peer.status.sessions {
                let detail = details.get(&(holder, session.key.as_str()));
                let person = team_share::person_tag(&peer.status.name, holder.member.as_str());
                let who = team_share::who(&person, &session.agent);
                for path in &session.files {
                    let Some(key) = team_share::team_path(path) else {
                        continue;
                    };
                    let lines = detail
                        .and_then(|by_path| by_path.get(&key))
                        .map(|ranges| ranges.to_vec())
                        .unwrap_or_default();
                    files.push(TeammateFile {
                        repo: session.repo.clone(),
                        path: path.clone(),
                        key,
                        who: who.clone(),
                        lines,
                    });
                }
            }
        }
        files
    }

    // -----------------------------------------------------------------------------------------
    // Record counters
    // -----------------------------------------------------------------------------------------

    /// The counter of the next sealed record; the caller stores [`Engine::local`]
    /// ([`Effect::StoreLocal`]) before it sends the record.
    pub fn reserve_counter(&mut self) -> Option<(u64, Effect)> {
        let counter = self.counter.take()?;
        self.local.counter = self.counter.last();
        Some((counter, Effect::StoreLocal))
    }

    /// Accepts an opened record once (replay marks); store [`Engine::local`] afterwards.
    pub fn accept_record(&mut self, opened: &Opened) -> bool {
        if self.guard.check(opened).is_err() {
            return false;
        }
        self.local.marks = self
            .guard
            .marks()
            .iter()
            .map(
                |((holder, sign), counter): (&(Holder, PublicKey), &u64)| ReplayMark {
                    holder: holder.clone(),
                    sign: *sign,
                    counter: *counter,
                },
            )
            .collect();
        true
    }

    /// The verified head, for tests and the wiki's safety number comparison.
    pub fn head(&self) -> Option<ChainHead> {
        self.verifier.state().map(|s| *s.head())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hangar_team_crypto::OsEntropy;
    use hangar_team_protocol::ids::{Seq, StreamId};
    use std::sync::{Arc, Mutex};

    /// The operating system's randomness, except that the draw the test arms fails once.
    struct Scripted(Arc<Mutex<Option<usize>>>);

    impl Entropy for Scripted {
        fn fill(&mut self, dest: &mut [u8]) -> Result<(), EntropyError> {
            let mut armed = self.0.lock().unwrap();
            if let Some(left) = armed.as_mut() {
                if *left == 0 {
                    *armed = None;
                    return Err(EntropyError);
                }
                *left -= 1;
            }
            drop(armed);
            OsEntropy.fill(dest)
        }
    }
    use hangar_team_protocol::overlap::{ClaimBoard, SessionClaims as BoardClaims};
    use hangar_team_protocol::wire::{
        encode_server, Ack, ErrorBody, KeyRotated, MemberLogAppended, MemberLogEntry, Welcome,
    };

    /// A relay in memory, enough to drive engines through the bootstrap: it applies every
    /// `memberlog.append`, `key.rotate` and `key.wrap` with the crypto crate's relay checks.
    struct Relay {
        team: TeamId,
        verifier: LogVerifier,
        entries: Vec<MemberLogEntry>,
        devices: Vec<DeviceInfo>,
        wrapped: Vec<(Holder, Epoch, Holder, Ciphertext)>,
        /// Accepted `key.wrap` frames.
        wraps: usize,
        seq: u64,
        board: ClaimBoard,
        /// `claim.set` frames received.
        claim_sets: usize,
    }

    fn team() -> TeamId {
        TeamId::new("p1").unwrap()
    }

    fn holder(member: &str, device: &str) -> Holder {
        Holder {
            member: MemberId::new(member).unwrap(),
            device: DeviceId::new(device).unwrap(),
        }
    }

    impl Relay {
        fn new() -> Self {
            Self {
                team: team(),
                verifier: LogVerifier::new(team()),
                entries: Vec::new(),
                devices: Vec::new(),
                wrapped: Vec::new(),
                wraps: 0,
                seq: 0,
                board: ClaimBoard::new(),
                claim_sets: 0,
            }
        }

        fn text(&self, id: Option<OpId>, seq: Option<u64>, message: ServerMessage) -> String {
            encode_server(&ServerFrame {
                id,
                seq: seq.map(Seq),
                team: Some(self.team.clone()),
                message,
            })
            .unwrap()
        }

        fn ack(&self, id: OpId, keys: Option<Vec<hangar_team_protocol::ids::Sealed>>) -> String {
            self.text(
                Some(id),
                None,
                ServerMessage::Ack(Ack {
                    blob: None,
                    content: None,
                    keys,
                }),
            )
        }

        fn err(&self, id: OpId, code: ErrorCode) -> String {
            self.text(
                Some(id),
                None,
                ServerMessage::Err(ErrorBody {
                    code,
                    retry_after_ms: None,
                    holder: None,
                }),
            )
        }

        /// Answers one client frame of `sender` (role from its ticket); returns frames for
        /// the sender, frames for everyone and frames for one holder.
        fn handle(
            &mut self,
            sender: &Holder,
            role: Role,
            frame: ClientFrame,
        ) -> (Vec<String>, Vec<String>, Vec<(Holder, String)>) {
            let id = frame.id;
            let mut own = Vec::new();
            let mut all = Vec::new();
            let mut targeted = Vec::new();
            match frame.message {
                ClientMessage::Hello(_) => own.push(self.text(
                    Some(id),
                    None,
                    ServerMessage::Welcome(Welcome {
                        proto: 1,
                        stream: StreamId::new("s").unwrap(),
                        head: Seq(self.seq),
                        member: sender.member.clone(),
                        epoch: Epoch(0),
                    }),
                )),
                ClientMessage::DeviceKeys(keys) => {
                    self.devices.retain(|d| d.holder != *sender);
                    self.devices.push(DeviceInfo {
                        holder: sender.clone(),
                        sign: keys.sign,
                        kem: keys.kem,
                        proof: keys.proof,
                        at: UnixMillis(1),
                    });
                    own.push(self.ack(id, None));
                }
                ClientMessage::DevicesGet => own.push(self.text(
                    Some(id),
                    None,
                    ServerMessage::Devices(hangar_team_protocol::wire::DeviceList {
                        devices: self.devices.clone(),
                    }),
                )),
                ClientMessage::MemberLogGet(get) => {
                    let entries = self
                        .entries
                        .iter()
                        .filter(|e| e.index > get.after)
                        .cloned()
                        .collect::<Vec<_>>();
                    own.push(self.text(
                        Some(id),
                        None,
                        ServerMessage::MemberLogEntries(MemberLogEntries::page(entries)),
                    ));
                }
                ClientMessage::MemberLogAppend(append) => {
                    let devices = self.devices.clone();
                    let registered = |h: &Holder| {
                        devices
                            .iter()
                            .find(|d| d.holder == *h)
                            .map(|d| DevicePublicKeys {
                                sign: d.sign,
                                kem: d.kem,
                            })
                    };
                    let checked =
                        hangar_team_crypto::relay::check_registered_keys(&append, registered)
                            .and_then(|()| self.verifier.admit(&append, sender, role));
                    match checked {
                        Ok(admitted) => {
                            self.store(&append, admitted.head, sender);
                            own.push(self.ack(id, None));
                            all.push(self.appended(admitted.head, sender));
                        }
                        Err(error) => own.push(self.err(id, error.code())),
                    }
                }
                ClientMessage::KeyRotate(rotate) => {
                    match self.verifier.admit_rotation(&rotate, sender, role) {
                        Ok(admitted) => {
                            self.store(&rotate.log, admitted.head, sender);
                            for wrapped in &rotate.wrapped {
                                self.wrapped.push((
                                    wrapped.holder.clone(),
                                    rotate.epoch,
                                    sender.clone(),
                                    wrapped.data.clone(),
                                ));
                            }
                            own.push(self.ack(id, None));
                            all.push(self.appended(admitted.head, sender));
                            self.seq += 1;
                            all.push(self.text(
                                None,
                                Some(self.seq),
                                ServerMessage::KeyRotated(KeyRotated {
                                    epoch: rotate.epoch,
                                    wrapped: None,
                                }),
                            ));
                        }
                        Err(error) => own.push(self.err(id, error.code())),
                    }
                }
                ClientMessage::KeyWrap(wrap) => {
                    let state = self.verifier.state().unwrap();
                    let from: Vec<Holder> = self
                        .wrapped
                        .iter()
                        .filter(|w| w.0 == wrap.wrapped.holder && w.1 == wrap.epoch)
                        .map(|w| w.2.clone())
                        .collect();
                    match hangar_team_crypto::relay::check_wrap(state, role, sender, &wrap, &from) {
                        Ok(()) => {
                            self.wraps += 1;
                            let target = wrap.wrapped.holder.clone();
                            self.wrapped.push((
                                target.clone(),
                                wrap.epoch,
                                sender.clone(),
                                wrap.wrapped.data,
                            ));
                            own.push(self.ack(id, None));
                            let note = self.text(
                                None,
                                None,
                                ServerMessage::KeyWrapped(hangar_team_protocol::wire::KeyWrapped {
                                    epoch: wrap.epoch,
                                    holder: target.clone(),
                                }),
                            );
                            targeted.push((target, note));
                        }
                        Err(error) => own.push(self.err(id, error.code())),
                    }
                }
                ClientMessage::KeyGet(get) => {
                    let keys: Vec<_> = self
                        .wrapped
                        .iter()
                        .filter(|w| w.0 == *sender && w.1 == get.epoch)
                        .map(|w| hangar_team_protocol::ids::Sealed {
                            epoch: get.epoch,
                            data: w.3.clone(),
                        })
                        .collect();
                    if keys.is_empty() {
                        own.push(self.err(id, ErrorCode::NotFound));
                    } else {
                        own.push(self.ack(id, Some(keys)));
                    }
                }
                ClientMessage::PresenceSet(set) => {
                    own.push(self.ack(id, None));
                    all.push(self.text(
                        None,
                        None,
                        ServerMessage::PresenceUpdate(PresenceUpdate {
                            holder: sender.clone(),
                            state: set.state,
                            status: set.status,
                            at: UnixMillis(1),
                        }),
                    ));
                }
                ClientMessage::ClaimSet(set) => {
                    self.claim_sets += 1;
                    own.push(self.ack(id, None));
                    let notices = self.board.set(
                        SessionRef {
                            holder: sender.clone(),
                            session: set.session,
                        },
                        BoardClaims {
                            repo: set.repo,
                            epoch: set.epoch,
                            files: set.files,
                            detail: set.detail,
                        },
                    );
                    for notice in notices {
                        let text =
                            self.text(None, None, ServerMessage::ClaimOverlap(notice.overlap));
                        targeted.push((notice.to.holder, text));
                    }
                }
                ClientMessage::ClaimClear(clear) => {
                    own.push(self.ack(id, None));
                    let notices = self.board.clear(&SessionRef {
                        holder: sender.clone(),
                        session: clear.session,
                    });
                    for notice in notices {
                        let text =
                            self.text(None, None, ServerMessage::ClaimOverlap(notice.overlap));
                        targeted.push((notice.to.holder, text));
                    }
                }
                _ => own.push(self.ack(id, None)),
            }
            (own, all, targeted)
        }

        fn store(
            &mut self,
            append: &hangar_team_protocol::wire::MemberLogAppend,
            head: ChainHead,
            by: &Holder,
        ) {
            self.entries.push(MemberLogEntry {
                index: append.index,
                hash: head.hash,
                entry: append.entry.clone(),
                by: by.clone(),
                at: UnixMillis(1),
            });
        }

        fn appended(&mut self, head: ChainHead, by: &Holder) -> String {
            self.seq += 1;
            self.text(
                None,
                Some(self.seq),
                ServerMessage::MemberLogAppended(MemberLogAppended {
                    index: head.index,
                    hash: head.hash,
                    by: by.clone(),
                }),
            )
        }
    }

    struct Client {
        engine: Engine,
        holder: Holder,
        role: Role,
        inbox: Vec<String>,
        stored_keys: usize,
        ended: Option<SessionEnd>,
        /// Fails the engine's draw after this many more, once.
        fail_draw: Arc<Mutex<Option<usize>>>,
    }

    fn client(member: &str, device: &str, role: Role) -> Client {
        let identity = DeviceIdentity::generate(&mut OsEntropy).unwrap();
        let fail_draw = Arc::new(Mutex::new(None));
        Client {
            engine: Engine::new(EngineSetup {
                project: "p1".to_owned(),
                team: team(),
                device: DeviceId::new(device).unwrap(),
                identity,
                member: None,
                website_role: role,
                keys: StoredKeys::default(),
                local: LocalState::default(),
                now: Box::new(|| 1_000),
                entropy: Box::new(Scripted(Arc::clone(&fail_draw))),
            }),
            holder: holder(member, device),
            role,
            inbox: Vec::new(),
            stored_keys: 0,
            ended: None,
            fail_draw,
        }
    }

    /// Runs frames between the clients and the relay until nothing moves.
    fn run(relay: &mut Relay, clients: &mut [&mut Client], mut start: Vec<(usize, Vec<Effect>)>) {
        for _ in 0..200 {
            let mut sends = Vec::new();
            for (at, effects) in start.drain(..) {
                for effect in effects {
                    match effect {
                        Effect::Send(frame) => sends.push((at, frame)),
                        Effect::StoreKeys => clients[at].stored_keys += 1,
                        Effect::End(end) => clients[at].ended = Some(end),
                        _ => {}
                    }
                }
            }
            for (at, frame) in sends {
                let (own, all, targeted) =
                    relay.handle(&clients[at].holder.clone(), clients[at].role, frame);
                clients[at].inbox.extend(own);
                for client in clients.iter_mut() {
                    client.inbox.extend(all.iter().cloned());
                    for (holder, text) in &targeted {
                        if *holder == client.holder {
                            client.inbox.push(text.clone());
                        }
                    }
                }
            }
            let mut any = false;
            for (at, client) in clients.iter_mut().enumerate() {
                for text in std::mem::take(&mut client.inbox) {
                    any = true;
                    start.push((at, client.engine.on_text(&text)));
                }
            }
            if !any && start.is_empty() {
                return;
            }
        }
        panic!("the clients did not settle");
    }

    fn ticket() -> Ticket {
        Ticket::new("t.t").unwrap()
    }

    fn connected(client: &Client) -> (u32, usize, usize) {
        match client.engine.status(false) {
            TeamStatus::Connected {
                epoch,
                members,
                devices,
            } => (epoch, members, devices),
            other => panic!("not connected: {other:?}"),
        }
    }

    #[test]
    fn the_owner_creates_the_team_and_its_first_epoch_and_stores_the_secret_first() {
        let mut relay = Relay::new();
        let mut owner = client("alice", "a1", Role::Owner);
        let hello = owner.engine.hello(ticket());
        run(&mut relay, &mut [&mut owner], vec![(0, hello)]);
        assert_eq!(connected(&owner), (1, 1, 1));
        assert!(owner.engine.keys().epochs.contains_key(&Epoch(1)));
        assert_eq!(owner.engine.keys().head.map(|h| h.index), Some(2));
        assert!(owner.stored_keys >= 2);
        assert!(owner.ended.is_none());
    }

    #[test]
    fn rotation_effects_store_the_secret_before_sending_key_rotate() {
        let mut relay = Relay::new();
        let mut owner = client("alice", "a1", Role::Owner);
        // Up to the created team, by hand: hello, welcome, the create entry accepted.
        let mut effects = owner.engine.hello(ticket());
        let mut order = Vec::new();
        for _ in 0..20 {
            let mut next = Vec::new();
            for effect in effects {
                match effect {
                    Effect::Send(frame) => {
                        order.push(frame.message.kind());
                        let (own, all, _) = relay.handle(&owner.holder.clone(), Role::Owner, frame);
                        for text in own.into_iter().chain(all) {
                            next.extend(owner.engine.on_text(&text));
                        }
                    }
                    Effect::StoreKeys => order.push("store"),
                    _ => {}
                }
            }
            effects = next;
        }
        let rotate = order.iter().position(|k| *k == "key.rotate").unwrap();
        assert_eq!(order[rotate - 1], "store", "{order:?}");
    }

    #[test]
    fn a_member_waits_while_the_log_is_empty() {
        let mut relay = Relay::new();
        let mut bob = client("bob", "b1", Role::Member);
        let hello = bob.engine.hello(ticket());
        run(&mut relay, &mut [&mut bob], vec![(0, hello)]);
        assert!(matches!(
            bob.engine.status(false),
            TeamStatus::WaitingForConfirmation { ref safety_number } if safety_number.len() == 6
        ));
        assert!(relay.entries.is_empty());
    }

    #[test]
    fn a_second_device_is_endorsed_by_the_first_and_gets_the_key() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let mut a2 = client("alice", "a2", Role::Owner);
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut a2], vec![(0, hello)]);
        let hello = a2.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut a2], vec![(1, hello)]);
        let shown = match a2.engine.status(false) {
            TeamStatus::WaitingForConfirmation { safety_number } => safety_number,
            other => panic!("{other:?}"),
        };
        let poll = a1.engine.poll_devices();
        run(&mut relay, &mut [&mut a1, &mut a2], vec![(0, poll)]);
        let pending = a1.engine.pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, PendingKind::OwnDevice);
        assert_eq!(
            pending[0].safety_number, shown,
            "both sides show the same number"
        );
        let confirm = a1.engine.confirm(&pending[0].id).unwrap();
        run(&mut relay, &mut [&mut a1, &mut a2], vec![(0, confirm)]);
        assert_eq!(connected(&a2), (1, 1, 2));
        assert_eq!(
            a2.engine.keys().epochs.get(&Epoch(1)),
            a1.engine.keys().epochs.get(&Epoch(1))
        );
        assert!(a1.engine.pending().is_empty());
    }

    #[test]
    fn an_owner_adds_a_member_rejects_another_and_rotates_after_a_removal() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let mut b1 = client("bob", "b1", Role::Member);
        let mut c1 = client("carol", "c1", Role::Member);
        let hello = a1.engine.hello(ticket());
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(0, hello)],
        );
        let b = b1.engine.hello(ticket());
        let c = c1.engine.hello(ticket());
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(1, b), (2, c)],
        );
        let poll = a1.engine.poll_devices();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(0, poll)],
        );
        let pending = a1.engine.pending();
        assert_eq!(pending.len(), 2);
        assert!(pending.iter().all(|p| p.kind == PendingKind::NewMember));
        let bob = pending
            .iter()
            .find(|p| p.member == "bob")
            .unwrap()
            .id
            .clone();
        let carol = pending
            .iter()
            .find(|p| p.member == "carol")
            .unwrap()
            .id
            .clone();
        // A member cannot confirm another member's device.
        let poll = b1.engine.poll_devices();
        assert!(poll.is_empty(), "bob is not in the log yet");
        let rejected = a1.engine.reject(&carol).unwrap();
        assert!(matches!(rejected[0], Effect::StoreLocal));
        assert_eq!(a1.engine.local().rejected, vec![carol.clone()]);
        let confirm = a1.engine.confirm(&bob).unwrap();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(0, confirm)],
        );
        assert_eq!(connected(&b1), (1, 2, 2));
        // The rejected request does not come back.
        let poll = a1.engine.poll_devices();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(0, poll)],
        );
        assert!(a1.engine.pending().is_empty());
        assert!(matches!(
            a1.engine.confirm(&carol),
            Err(TeamError::UnknownRequest)
        ));
        // Bob leaves: the owner's device rotates, bob is told and wipes his keys.
        let remove = a1
            .engine
            .remove_member(&MemberId::new("bob").unwrap())
            .unwrap();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(0, remove)],
        );
        assert_eq!(connected(&a1), (2, 1, 1));
        assert_eq!(b1.ended, Some(SessionEnd::Stop(StopReason::Removed)));
    }

    #[test]
    fn a_rewritten_history_is_refused() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1], vec![(0, hello)]);
        let verified = a1.engine.keys().head.unwrap();
        // The relay forgets everything: a new team under the same id, by another owner.
        let mut fake = Relay::new();
        let mut mallory = client("alice", "m1", Role::Owner);
        let hello = mallory.engine.hello(ticket());
        run(&mut fake, &mut [&mut mallory], vec![(0, hello)]);
        a1.ended = None;
        let hello = a1.engine.hello(ticket());
        run(&mut fake, &mut [&mut a1], vec![(0, hello)]);
        assert_eq!(
            a1.ended,
            Some(SessionEnd::Stop(StopReason::Attention(
                AttentionReason::HistoryMismatch
            )))
        );
        assert_eq!(
            a1.engine.keys().head,
            Some(verified),
            "the old head stays stored"
        );
    }

    #[test]
    fn hello_errors_and_close_relevant_answers() {
        let relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let effects = a1.engine.hello(ticket());
        let Effect::Send(frame) = &effects[0] else {
            panic!("hello not sent");
        };
        let forbidden = relay.err(frame.id.clone(), ErrorCode::Forbidden);
        assert!(matches!(
            a1.engine.on_text(&forbidden)[..],
            [Effect::End(SessionEnd::HelloForbidden)]
        ));
        let effects = a1.engine.hello(ticket());
        let Effect::Send(frame) = &effects[0] else {
            panic!("hello not sent");
        };
        let outdated = relay.err(frame.id.clone(), ErrorCode::OutdatedClient);
        assert!(matches!(
            a1.engine.on_text(&outdated)[..],
            [Effect::End(SessionEnd::Stop(StopReason::Attention(
                AttentionReason::NeedsUpdate
            )))]
        ));
        assert!(matches!(
            a1.engine.on_text(r#"{"v":2,"type":"ack"}"#)[..],
            [Effect::End(SessionEnd::Stop(StopReason::Attention(
                AttentionReason::NeedsUpdate
            )))]
        ));
        assert!(matches!(
            a1.engine.on_text("nonsense")[..],
            [Effect::End(SessionEnd::Lost)]
        ));
    }

    #[test]
    fn the_cursor_resumes_and_a_gap_reconnects() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1], vec![(0, hello)]);
        let cursor = a1.engine.local().cursor.clone().unwrap();
        assert_eq!(cursor.seq, Seq(relay.seq));
        // The next hello carries it.
        let effects = a1.engine.hello(ticket());
        let Effect::Send(ClientFrame {
            message: ClientMessage::Hello(hello),
            ..
        }) = &effects[0]
        else {
            panic!("hello not sent");
        };
        assert_eq!(hello.last_seq.as_ref(), Some(&cursor));
        // An event two ahead is a gap.
        let gap = relay.text(
            None,
            Some(cursor.seq.0 + 2),
            ServerMessage::MemberLogAppended(MemberLogAppended {
                index: 9,
                hash: hangar_team_protocol::e2e::LogHash::from_bytes([0; 32]),
                by: a1.holder.clone(),
            }),
        );
        assert!(matches!(
            a1.engine.on_text(&gap)[..],
            [Effect::End(SessionEnd::Gap)]
        ));
    }

    #[test]
    fn unsigned_removal_events_wipe_nothing_without_a_log_entry() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1], vec![(0, hello)]);
        assert_eq!(connected(&a1), (1, 1, 1));
        // The relay claims this device and then its member were removed; the log says nothing.
        relay.seq += 1;
        let device = relay.text(
            None,
            Some(relay.seq),
            ServerMessage::DeviceRemoved(hangar_team_protocol::wire::DeviceRemoved {
                holder: a1.holder.clone(),
            }),
        );
        relay.seq += 1;
        let member = relay.text(
            None,
            Some(relay.seq),
            ServerMessage::MemberRemoved(hangar_team_protocol::wire::MemberRemoved {
                member: a1.holder.member.clone(),
            }),
        );
        a1.inbox.push(device);
        a1.inbox.push(member);
        let mut wiped = false;
        let mut effects = Vec::new();
        for text in std::mem::take(&mut a1.inbox) {
            let next = a1.engine.on_text(&text);
            wiped |= next.iter().any(|e| matches!(e, Effect::WipeDevice));
            effects.push((0, next));
        }
        assert!(!wiped, "an unsigned relay event wiped the keys");
        run(&mut relay, &mut [&mut a1], effects);
        assert!(a1.ended.is_none(), "{:?}", a1.ended);
        assert_eq!(connected(&a1), (1, 1, 1));
        assert!(a1.engine.keys().epochs.contains_key(&Epoch(1)));
    }

    /// The op id of the first `memberlog.get` among `effects`.
    fn log_get(effects: &[Effect]) -> Option<OpId> {
        effects.iter().find_map(|effect| match effect {
            Effect::Send(ClientFrame {
                id,
                message: ClientMessage::MemberLogGet(_),
                ..
            }) => Some(id.clone()),
            _ => None,
        })
    }

    #[test]
    fn a_log_page_without_progress_ends_the_connection() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1], vec![(0, hello)]);
        // A new entry is announced; the relay then answers the fetch with "more" but nothing.
        relay.seq += 1;
        let appended = relay.text(
            None,
            Some(relay.seq),
            ServerMessage::MemberLogAppended(MemberLogAppended {
                index: 3,
                hash: hangar_team_protocol::e2e::LogHash::from_bytes([0; 32]),
                by: a1.holder.clone(),
            }),
        );
        let effects = a1.engine.on_text(&appended);
        let id = log_get(&effects).unwrap();
        let empty = relay.text(
            Some(id),
            None,
            ServerMessage::MemberLogEntries(MemberLogEntries {
                entries: Vec::new(),
                more: true,
            }),
        );
        let effects = a1.engine.on_text(&empty);
        assert!(log_get(&effects).is_none(), "asked again without progress");
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, Effect::End(SessionEnd::Lost))),
            "{effects:?}"
        );
    }

    #[test]
    fn counters_are_reserved_for_storing_first() {
        // The clock reads 1000 ms: a fresh state resumes above 1000 * 1024.
        let mut a1 = client("alice", "a1", Role::Owner);
        let (first, effect) = a1.engine.reserve_counter().unwrap();
        assert_eq!(first, 1_024_001);
        assert!(matches!(effect, Effect::StoreLocal));
        assert_eq!(a1.engine.local().counter, 1_024_001);
        let (second, _) = a1.engine.reserve_counter().unwrap();
        assert_eq!(second, 1_024_002);
    }

    #[test]
    fn a_stored_counter_above_the_clock_floor_goes_on_from_there() {
        let engine = |stored: u64| {
            Engine::new(EngineSetup {
                project: "p1".to_owned(),
                team: team(),
                device: DeviceId::new("a1").unwrap(),
                identity: DeviceIdentity::generate(&mut OsEntropy).unwrap(),
                member: None,
                website_role: Role::Owner,
                keys: StoredKeys::default(),
                local: LocalState {
                    counter: stored,
                    ..LocalState::default()
                },
                now: Box::new(|| 1_000),
                entropy: Box::new(OsEntropy),
            })
        };
        assert_eq!(
            engine(5_000_000).reserve_counter().map(|(c, _)| c),
            Some(5_000_001)
        );
        // A state file put back from an old backup: the clock floor wins.
        assert_eq!(engine(3).reserve_counter().map(|(c, _)| c), Some(1_024_001));
    }

    #[test]
    fn a_partial_rotation_hands_the_missing_device_its_key_with_key_wrap() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let mut a2 = client("alice", "a2", Role::Owner);
        let mut b1 = client("bob", "b1", Role::Member);
        let hello = a1.engine.hello(ticket());
        run(
            &mut relay,
            &mut [&mut a1, &mut a2, &mut b1],
            vec![(0, hello)],
        );
        let a2_hello = a2.engine.hello(ticket());
        let b1_hello = b1.engine.hello(ticket());
        run(
            &mut relay,
            &mut [&mut a1, &mut a2, &mut b1],
            vec![(1, a2_hello), (2, b1_hello)],
        );
        let poll = a1.engine.poll_devices();
        run(
            &mut relay,
            &mut [&mut a1, &mut a2, &mut b1],
            vec![(0, poll)],
        );
        let a2_request = a1
            .engine
            .pending()
            .into_iter()
            .find(|p| p.device == "a2")
            .unwrap();
        let confirm = a1.engine.confirm(&a2_request.id).unwrap();
        run(
            &mut relay,
            &mut [&mut a1, &mut a2, &mut b1],
            vec![(0, confirm)],
        );
        assert_eq!(connected(&a2), (1, 1, 2));
        // Bob joins and leaves again; in the rotation that follows, the wrap for a2 fails (the
        // draws are the secret, then the wraps in log order: a1, a2).
        let poll = a1.engine.poll_devices();
        run(
            &mut relay,
            &mut [&mut a1, &mut a2, &mut b1],
            vec![(0, poll)],
        );
        let bob = a1.engine.pending().into_iter().next().unwrap();
        let confirm = a1.engine.confirm(&bob.id).unwrap();
        run(
            &mut relay,
            &mut [&mut a1, &mut a2, &mut b1],
            vec![(0, confirm)],
        );
        assert_eq!(connected(&b1), (1, 2, 3));
        let wraps = relay.wraps;
        *a1.fail_draw.lock().unwrap() = Some(2);
        let remove = a1
            .engine
            .remove_member(&MemberId::new("bob").unwrap())
            .unwrap();
        run(
            &mut relay,
            &mut [&mut a1, &mut a2, &mut b1],
            vec![(0, remove)],
        );
        assert!(a1.fail_draw.lock().unwrap().is_none(), "the draw failed");
        assert_eq!(connected(&a1), (2, 1, 2));
        assert_eq!(relay.wraps, wraps + 1, "a2 got epoch 2 by key.wrap");
        assert_eq!(connected(&a2), (2, 1, 2));
        assert_eq!(
            a2.engine.keys().epochs.get(&Epoch(2)),
            a1.engine.keys().epochs.get(&Epoch(2))
        );
    }

    #[test]
    fn a_member_removed_before_is_marked_as_readmitted() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let mut b1 = client("bob", "b1", Role::Member);
        let mut c1 = client("carol", "c1", Role::Member);
        let hello = a1.engine.hello(ticket());
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(0, hello)],
        );
        let hello = b1.engine.hello(ticket());
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(1, hello)],
        );
        let poll = a1.engine.poll_devices();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(0, poll)],
        );
        let bob = a1.engine.pending().into_iter().next().unwrap();
        assert!(!bob.readmitted);
        let confirm = a1.engine.confirm(&bob.id).unwrap();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(0, confirm)],
        );
        let remove = a1
            .engine
            .remove_member(&MemberId::new("bob").unwrap())
            .unwrap();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(0, remove)],
        );
        assert_eq!(connected(&a1), (2, 1, 1));
        // Bob comes back with a new device; carol is new.
        let mut b2 = client("bob", "b2", Role::Member);
        let b2_hello = b2.engine.hello(ticket());
        let c1_hello = c1.engine.hello(ticket());
        run(
            &mut relay,
            &mut [&mut a1, &mut b2, &mut c1],
            vec![(1, b2_hello), (2, c1_hello)],
        );
        let poll = a1.engine.poll_devices();
        run(
            &mut relay,
            &mut [&mut a1, &mut b2, &mut c1],
            vec![(0, poll)],
        );
        let pending = a1.engine.pending();
        let readmitted = |member: &str| {
            pending
                .iter()
                .find(|p| p.member == member)
                .map(|p| p.readmitted)
        };
        assert_eq!(readmitted("bob"), Some(true));
        assert_eq!(readmitted("carol"), Some(false));
    }

    /// Alice (owner, a1) and Bob (member, b1), both connected with epoch 1.
    fn team_of_two() -> (Relay, Client, Client) {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let mut b1 = client("bob", "b1", Role::Member);
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, hello)]);
        let hello = b1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, hello)]);
        let poll = a1.engine.poll_devices();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, poll)]);
        let bob = a1.engine.pending().into_iter().next().unwrap();
        let confirm = a1.engine.confirm(&bob.id).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, confirm)]);
        assert_eq!(connected(&b1), (1, 2, 2));
        (relay, a1, b1)
    }

    fn range(start: u32, end: u32) -> crate::claims::LineRange {
        crate::claims::LineRange { start, end }
    }

    fn share(person: &str, id: SessionId, agent: &str, files: &[(&str, u32, u32)]) -> LocalShare {
        LocalShare {
            person: person.to_owned(),
            sessions: vec![team_share::LocalSession {
                id,
                agent: agent.to_owned(),
                title: format!("{person}'s task"),
                state: SessionState::Working,
                repo: "a".repeat(40),
                folder: "c:/src/hangar/.git".to_owned(),
                repo_name: "hangar".to_owned(),
                base: Some("b".repeat(40)),
                claims: files
                    .iter()
                    .map(|&(path, start, end)| crate::claims::Claim {
                        path: path.to_owned(),
                        kind: crate::claims::ChangeKind::Modified,
                        whole_file: false,
                        ranges: vec![range(start, end)],
                        area: crate::claims::area_of(path),
                    })
                    .collect(),
                truncated: false,
            }],
        }
    }

    #[test]
    fn teammates_see_each_others_presence_and_a_refined_overlap() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let a = a1
            .engine
            .set_share(share("Alice", 1, "Claude Code", &[("src/auth.rs", 10, 12)]));
        let b = b1.engine.set_share(share(
            "Bob",
            7,
            "Codex",
            &[("src/Auth.rs", 14, 15), ("docs/x.md", 1, 1)],
        ));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, a), (1, b)]);

        // Presence: each sees the other's sessions, opened from the sealed status.
        let seen = b1.engine.activity();
        assert_eq!(seen.peers.len(), 1);
        assert_eq!(seen.peers[0].name, "Alice");
        assert!(!seen.peers[0].own);
        assert_eq!(seen.peers[0].sessions[0].agent, "Claude Code");
        assert_eq!(seen.peers[0].sessions[0].files, ["src/auth.rs"]);
        assert_eq!(seen.peers[0].presence, "active");

        // The relay matched the file by its keyed hash; each side refined it to the lines.
        let overlaps = a1.engine.activity().overlaps;
        assert_eq!(overlaps.len(), 1, "{overlaps:?}");
        let overlap = &overlaps[0];
        assert_eq!(overlap.session_id, 1);
        assert_eq!(overlap.name, "Bob");
        assert_eq!(overlap.agent, "Codex");
        assert_eq!(overlap.level, OverlapLevel::OverlappingLines);
        assert_eq!(overlap.path.as_deref(), Some("src/auth.rs"));
        assert_eq!(overlap.lines, [range(10, 15)]);
        let theirs = &b1.engine.activity().overlaps[0];
        assert_eq!(theirs.session_id, 7);
        assert_eq!(
            theirs.path.as_deref(),
            Some("src/Auth.rs"),
            "bob's spelling"
        );

        // The gate's and the context's view of Bob's files on Alice's device.
        let files = a1.engine.teammate_files();
        let auth = files.iter().find(|f| f.key == "src/auth.rs").unwrap();
        assert_eq!(auth.who, "Bob #bob (Codex)", "the name with the member id");
        assert_eq!(auth.lines, [range(14, 15)]);
        assert!(files.iter().any(|f| f.path == "docs/x.md"));

        // Nothing changed: nothing is sent again.
        let sets = relay.claim_sets;
        let again =
            a1.engine
                .set_share(share("Alice", 1, "Claude Code", &[("src/auth.rs", 10, 12)]));
        assert!(again.is_empty());
        assert_eq!(relay.claim_sets, sets);

        // Bob's session ends: its claims are cleared and the overlap ends on Alice's device.
        let ended = b1.engine.set_share(LocalShare {
            person: "Bob".to_owned(),
            sessions: Vec::new(),
        });
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, ended)]);
        assert!(a1.engine.activity().overlaps.is_empty());
        assert!(a1.engine.activity().peers[0].sessions.is_empty());
    }

    #[test]
    fn the_relay_never_sees_paths_names_or_lines() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let effects = a1.engine.set_share(share(
            "Alice",
            1,
            "Claude Code",
            &[("src/secret-plan.rs", 3, 4)],
        ));
        let mut texts = Vec::new();
        for effect in &effects {
            if let Effect::Send(frame) = effect {
                texts.push(hangar_team_protocol::wire::encode_client(frame).unwrap());
            }
        }
        assert!(texts.iter().any(|t| t.contains("claim.set")));
        assert!(texts.iter().any(|t| t.contains("presence.set")));
        for text in &texts {
            for word in ["secret-plan", "Alice", "Claude", "hangar", &"a".repeat(40)] {
                assert!(!text.contains(word), "{word} in {text}");
            }
        }
        // The counter was stored before the records left.
        assert!(matches!(effects[0], Effect::StoreLocal), "{effects:?}");
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
    }

    #[test]
    fn a_device_the_log_does_not_list_never_shows_as_a_teammate() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let stranger = Holder {
            member: MemberId::new("mallory").unwrap(),
            device: DeviceId::new("m1").unwrap(),
        };
        let made_up = relay.text(
            None,
            None,
            ServerMessage::PresenceUpdate(PresenceUpdate {
                holder: stranger.clone(),
                state: PresenceState::Active,
                status: None,
                at: UnixMillis(1),
            }),
        );
        let effects = a1.engine.on_text(&made_up);
        // The device fetches the log once more, then drops the stranger.
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        assert!(a1.engine.stashed.is_empty());
        // Bob, whom the log lists, shows; the stranger does not.
        let b = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("x.rs", 1, 1)]));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, b)]);
        let peers = a1.engine.activity().peers;
        assert_eq!(peers.len(), 1, "{peers:?}");
        assert_eq!(peers[0].member, "bob");
        assert_eq!(peers[0].name, "Bob");
    }

    /// The sealed status of the `presence.set` among `effects`.
    fn presence_status(effects: &[Effect]) -> Sealed {
        effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Send(ClientFrame {
                    message: ClientMessage::PresenceSet(set),
                    ..
                }) => set.status.clone(),
                _ => None,
            })
            .expect("a presence.set with a status")
    }

    /// The `claim.set` among `effects`.
    fn claim_set(effects: &[Effect]) -> ClaimSet {
        effects
            .iter()
            .find_map(|effect| match effect {
                Effect::Send(ClientFrame {
                    message: ClientMessage::ClaimSet(set),
                    ..
                }) => Some(set.clone()),
                _ => None,
            })
            .expect("a claim.set")
    }

    fn presence_text(relay: &Relay, holder: &Holder, status: Option<Sealed>) -> String {
        let state = if status.is_some() {
            PresenceState::Active
        } else {
            PresenceState::Offline
        };
        relay.text(
            None,
            None,
            ServerMessage::PresenceUpdate(PresenceUpdate {
                holder: holder.clone(),
                state,
                status,
                at: UnixMillis(1),
            }),
        )
    }

    fn peer_files(client: &Client) -> Option<Vec<String>> {
        let peers = client.engine.activity().peers;
        peers.first().map(|p| {
            p.sessions
                .iter()
                .flat_map(|s| s.files.iter().cloned())
                .collect()
        })
    }

    #[test]
    fn an_unsigned_record_with_a_wrong_log_head_does_not_stop_the_team() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let b = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("x.rs", 1, 1)]));
        let sealed = presence_status(&b);
        // One character inside the head hash (record bytes 9 to 40, base64url characters 12 to
        // 54): a known head index with another hash, and a signature that no longer verifies.
        let mut text = sealed.data.as_str().to_owned();
        let other = if text.as_bytes()[20] == b'A' {
            "B"
        } else {
            "A"
        };
        text.replace_range(20..21, other);
        let forged = Sealed {
            epoch: sealed.epoch,
            data: Ciphertext::new(text).unwrap(),
        };
        let effects = a1
            .engine
            .on_text(&presence_text(&relay, &b1.holder, Some(forged)));
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::End(_))),
            "{effects:?}"
        );
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        assert!(a1.ended.is_none(), "{:?}", a1.ended);
        assert_eq!(connected(&a1), (1, 2, 2));
        // Bob's real status still opens afterwards.
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, b)]);
        assert_eq!(peer_files(&a1), Some(vec!["x.rs".to_owned()]));
    }

    #[test]
    fn an_old_status_shown_again_after_offline_or_a_reconnect_is_ignored() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let first = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("old.rs", 1, 1)]));
        let old = presence_status(&first);
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, first)]);
        let second = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("new.rs", 1, 1)]));
        let new = presence_status(&second);
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, second)]);
        assert_eq!(peer_files(&a1), Some(vec!["new.rs".to_owned()]));

        // The relay says Bob went offline (unsigned), then shows his old status again.
        let _ = a1.engine.on_text(&presence_text(&relay, &b1.holder, None));
        assert_eq!(peer_files(&a1), None);
        let _ = a1
            .engine
            .on_text(&presence_text(&relay, &b1.holder, Some(old.clone())));
        assert_eq!(peer_files(&a1), None, "the old status came back");
        // The newest one sent again (same counter) is fine.
        let _ = a1
            .engine
            .on_text(&presence_text(&relay, &b1.holder, Some(new.clone())));
        assert_eq!(peer_files(&a1), Some(vec!["new.rs".to_owned()]));

        // A reconnect forgets the peers, not the marks; the state file keeps them.
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, hello)]);
        let _ = a1
            .engine
            .on_text(&presence_text(&relay, &b1.holder, Some(old)));
        assert_eq!(peer_files(&a1), None, "the old status came back");
        assert!(!a1.engine.local().presence_marks.is_empty());
        assert_eq!(LiveMarks::from_local(a1.engine.local()), a1.engine.live);
    }

    #[test]
    fn an_old_claim_detail_shown_again_keeps_the_newer_lines() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let a = a1
            .engine
            .set_share(share("Alice", 1, "Claude Code", &[("src/auth.rs", 10, 12)]));
        let b = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("src/auth.rs", 14, 15)]));
        let old = claim_set(&b);
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, a), (1, b)]);
        assert_eq!(
            a1.engine.activity().overlaps[0].level,
            OverlapLevel::OverlappingLines
        );
        let b = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("src/auth.rs", 40, 41)]));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, b)]);
        let now = a1.engine.activity().overlaps[0].clone();
        assert_eq!(now.level, OverlapLevel::SameFile);
        assert_eq!(now.lines, [range(40, 41)]);

        // The relay shows the old detail again.
        let mine = a1.engine.session_keys.get(&1).unwrap().clone();
        let replay = relay.text(
            None,
            None,
            ServerMessage::ClaimOverlap(ClaimOverlap {
                repo: old.repo,
                session: mine,
                other: SessionRef {
                    holder: b1.holder.clone(),
                    session: old.session,
                },
                level: MatchLevel::SameFile,
                file: None,
                files: 1,
                detail: old.detail,
            }),
        );
        let _ = a1.engine.on_text(&replay);
        assert_eq!(a1.engine.activity().overlaps[0], now);
        // Stored with the next ping or the end of the connection at the latest.
        let _ = a1.engine.ping();
        assert!(!a1.engine.local().detail_marks.is_empty());
    }

    #[test]
    fn a_detail_without_a_shared_file_outweighs_the_relays_same_file() {
        let (_relay, mut a1, b1) = team_of_two();
        let _ = a1
            .engine
            .set_share(share("Alice", 1, "Claude Code", &[("src/auth.rs", 10, 12)]));
        let other = SessionRef {
            holder: b1.holder.clone(),
            session: SessionKey::new("k").unwrap(),
        };
        let detail = |path: &str| OtherDetail {
            name: "Bob".to_owned(),
            agent: "Codex".to_owned(),
            title: String::new(),
            base: None,
            claims: vec![crate::claims::Claim {
                path: path.to_owned(),
                kind: crate::claims::ChangeKind::Modified,
                whole_file: true,
                ranges: Vec::new(),
                area: crate::claims::area_of(path),
            }],
        };
        let entry = |detail: Option<OtherDetail>| OverlapEntry {
            level: MatchLevel::SameFile,
            files: 1,
            hint: Some("src/auth.rs".to_owned()),
            detail,
        };
        // Bob's newest signed status lists the session.
        a1.engine
            .peers
            .insert(b1.holder.clone(), peer_with_sessions(&["k"]));
        let put = |engine: &mut Engine, entry: OverlapEntry| {
            engine
                .overlaps
                .entry(1)
                .or_default()
                .insert(other.clone(), entry);
        };
        put(&mut a1.engine, entry(Some(detail("docs/x.md"))));
        let shown = &a1.engine.activity().overlaps[0];
        assert_eq!(shown.level, OverlapLevel::SameArea);
        assert_eq!(shown.path, None);
        // Without a detail the relay's word stands; with a shared file the detail decides.
        put(&mut a1.engine, entry(None));
        assert_eq!(
            a1.engine.activity().overlaps[0].level,
            OverlapLevel::SameFile
        );
        put(&mut a1.engine, entry(Some(detail("SRC/Auth.rs"))));
        assert_eq!(
            a1.engine.activity().overlaps[0].path.as_deref(),
            Some("src/auth.rs")
        );
    }

    /// A teammate's presence whose signed status lists sessions with these keys.
    fn peer_with_sessions(keys: &[&str]) -> Peer {
        Peer {
            presence: PresenceState::Active,
            status: PeerStatus {
                name: "Bob".to_owned(),
                sessions: keys
                    .iter()
                    .map(|key| team_share::PeerSession {
                        key: (*key).to_owned(),
                        agent: "Codex".to_owned(),
                        title: String::new(),
                        state: SessionState::Working,
                        repo: "a".repeat(40),
                        files: Vec::new(),
                        more_files: 0,
                    })
                    .collect(),
            },
            signed: true,
        }
    }

    #[test]
    fn overlaps_per_session_are_bounded_by_the_devices_in_the_log() {
        let (relay, mut a1, b1) = team_of_two();
        let a = a1
            .engine
            .set_share(share("Alice", 1, "Claude Code", &[("src/auth.rs", 10, 12)]));
        let repo = claim_set(&a).repo;
        let mine = a1.engine.session_keys.get(&1).unwrap().clone();
        // Without a status from bob's device the overlaps are kept (at most as the same area).
        a1.engine.peers.clear();
        for n in 0..500 {
            let text = relay.text(
                None,
                None,
                ServerMessage::ClaimOverlap(ClaimOverlap {
                    repo,
                    session: mine.clone(),
                    other: SessionRef {
                        holder: b1.holder.clone(),
                        session: SessionKey::new(format!("s{n}")).unwrap(),
                    },
                    level: MatchLevel::SameArea,
                    file: None,
                    files: 0,
                    detail: None,
                }),
            );
            let _ = a1.engine.on_text(&text);
        }
        assert_eq!(a1.engine.overlaps.len(), 1);
        assert_eq!(a1.engine.overlaps[&1].len(), 2 * MAX_OVERLAPS_PER_DEVICE);
        assert!(a1
            .engine
            .activity()
            .overlaps
            .iter()
            .all(|o| o.level == OverlapLevel::SameArea && o.path.is_none()));
    }

    #[test]
    fn detail_marks_stay_bounded_per_device_and_refuse_dropped_sessions() {
        let mut marks = LiveMarks::default();
        let bob = holder("bob", "b1");
        for n in 0..MAX_DETAIL_SESSIONS as u64 {
            assert_eq!(marks.detail(&bob, &format!("s{n}"), 100 + n), Mark::Raised);
        }
        // One more: the oldest session's mark goes, and the floor rises to it.
        assert_eq!(marks.detail(&bob, "late", 500), Mark::Raised);
        assert_eq!(marks.detail.len(), MAX_DETAIL_SESSIONS);
        assert_eq!(marks.detail(&bob, "s0", 100), Mark::Stale);
        // A newer session (or record) of the device still passes; an older unknown one does not.
        assert_eq!(marks.detail(&bob, "later", 600), Mark::Raised);
        assert_eq!(marks.detail(&bob, "ancient", 50), Mark::Stale);
        assert_eq!(marks.detail(&bob, "late", 500), Mark::Seen);
        assert_eq!(marks.detail(&bob, "late", 499), Mark::Stale);
        // Devices the log no longer lists lose their marks.
        assert!(marks.retain(|_| false));
        assert!(marks.detail.is_empty() && marks.floors.is_empty());
    }

    #[test]
    fn a_status_of_an_older_epoch_is_not_shown() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let b = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("x.rs", 1, 1)]));
        let old = presence_status(&b);
        assert_eq!(old.epoch, Epoch(1));
        // Carol joins and leaves: the owner's device rotates to epoch 2.
        let mut c1 = client("carol", "c1", Role::Member);
        let hello = c1.engine.hello(ticket());
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(2, hello)],
        );
        let poll = a1.engine.poll_devices();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(0, poll)],
        );
        let carol = a1.engine.pending().into_iter().next().unwrap();
        let confirm = a1.engine.confirm(&carol.id).unwrap();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(0, confirm)],
        );
        let remove = a1
            .engine
            .remove_member(&MemberId::new("carol").unwrap())
            .unwrap();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(0, remove)],
        );
        assert_eq!(connected(&a1).0, 2);
        assert!(a1.engine.keys().epochs.contains_key(&Epoch(1)));
        // Without any mark, a status of epoch 1 does not show any more.
        a1.engine.live = LiveMarks::default();
        a1.engine.peers.clear();
        let _ = a1
            .engine
            .on_text(&presence_text(&relay, &b1.holder, Some(old)));
        assert!(
            peer_files(&a1).unwrap_or_default().is_empty(),
            "{:?}",
            peer_files(&a1)
        );
    }

    #[test]
    fn a_device_bound_to_another_account_stops_without_touching_keys() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let hello = a1.engine.hello(ticket());
        assert!(
            !hello.iter().any(|e| matches!(e, Effect::StoreDevice)),
            "nothing to bind before the welcome"
        );
        run(&mut relay, &mut [&mut a1], vec![(0, hello)]);
        assert_eq!(a1.engine.member().map(MemberId::as_str), Some("alice"));
        assert_eq!(a1.engine.stored_device().member.as_deref(), Some("alice"));
        // Bob signs in on the same keychain: the relay welcomes bob for this device.
        a1.holder.member = MemberId::new("bob").unwrap();
        a1.ended = None;
        let keys_before = a1.engine.keys().encode();
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1], vec![(0, hello)]);
        assert_eq!(
            a1.ended,
            Some(SessionEnd::Stop(StopReason::Attention(
                AttentionReason::OtherAccount
            )))
        );
        assert_eq!(a1.engine.member().map(MemberId::as_str), Some("alice"));
        assert_eq!(*a1.engine.keys().encode(), *keys_before);
    }

    #[test]
    fn wrapped_keys_of_rotations_are_kept_only_for_newer_epochs_and_few() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1], vec![(0, hello)]);
        for epoch in 0..20u32 {
            relay.seq += 1;
            let rotated = relay.text(
                None,
                Some(relay.seq),
                ServerMessage::KeyRotated(KeyRotated {
                    epoch: Epoch(epoch),
                    wrapped: Some(Ciphertext::new("AAAA").unwrap()),
                }),
            );
            let _ = a1.engine.on_text(&rotated);
        }
        assert_eq!(a1.engine.inline.len(), MAX_INLINE);
        assert!(a1.engine.inline.keys().all(|e| e.0 > 1));
        assert_eq!(a1.engine.inline.keys().next(), Some(&Epoch(16)));
        let _ = a1.engine.hello(ticket());
        assert!(a1.engine.inline.is_empty());
    }

    /// Carol joins the team of `a1` (owner) and `b1`: the verified log moves on.
    fn carol_joins(relay: &mut Relay, a1: &mut Client, b1: &mut Client) {
        let mut c1 = client("carol", "c1", Role::Member);
        let hello = c1.engine.hello(ticket());
        run(relay, &mut [&mut *a1, &mut *b1, &mut c1], vec![(2, hello)]);
        let poll = a1.engine.poll_devices();
        run(relay, &mut [&mut *a1, &mut *b1, &mut c1], vec![(0, poll)]);
        let carol = a1.engine.pending().into_iter().next().unwrap();
        let confirm = a1.engine.confirm(&carol.id).unwrap();
        run(
            relay,
            &mut [&mut *a1, &mut *b1, &mut c1],
            vec![(0, confirm)],
        );
    }

    #[test]
    fn a_device_signing_another_history_is_set_aside_without_stopping_the_team() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let b = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("x.rs", 1, 1)]));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, b)]);
        assert_eq!(peer_files(&a1), Some(vec!["x.rs".to_owned()]));

        // Bob's device signs a status naming the verified head index with another hash.
        let (cipher, head, _) = b1.engine.current_cipher().unwrap();
        let forked = ChainHead {
            index: head.index,
            hash: hangar_team_protocol::e2e::LogHash::from_bytes([7; 32]),
        };
        let status = br#"{"v":1,"name":"Bob","sessions":[]}"#;
        let sealed = b1
            .engine
            .seal(&cipher, &forked, RecordKind::PresenceStatus, b"", status)
            .unwrap();
        let effects = a1
            .engine
            .on_text(&presence_text(&relay, &b1.holder, Some(sealed)));
        assert!(
            !effects.iter().any(|e| matches!(e, Effect::End(_))),
            "{effects:?}"
        );
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        assert!(a1.ended.is_none(), "{:?}", a1.ended);
        assert_eq!(connected(&a1), (1, 2, 2), "the team goes on");
        let activity = a1.engine.activity();
        assert!(activity.peers.is_empty());
        assert_eq!(
            activity.divergent,
            [DivergentDevice {
                member: "bob".to_owned(),
                device: "b1".to_owned(),
                name: "Bob".to_owned(),
            }]
        );
        assert!(a1.engine.teammate_files().is_empty());

        // Bob's records are ignored meanwhile, also over a new connection with the same log.
        let again = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("y.rs", 1, 1)]));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, again)]);
        assert_eq!(peer_files(&a1), None);
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, hello)]);
        assert_eq!(a1.engine.activity().divergent.len(), 1);

        // The log moves on: bob's device is heard again.
        carol_joins(&mut relay, &mut a1, &mut b1);
        assert!(a1.engine.activity().divergent.is_empty());
        let later = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("z.rs", 1, 1)]));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, later)]);
        assert_eq!(peer_files(&a1), Some(vec!["z.rs".to_owned()]));
        assert!(a1.ended.is_none());
    }

    #[test]
    fn presence_marks_are_kept_per_device_and_read_from_per_key_marks() {
        let bob = holder("bob", "b1");
        // A state file of the first format: one mark per signing key of the same device.
        let local = LocalState {
            presence_marks: vec![
                HolderMark {
                    holder: bob.clone(),
                    counter: 9,
                },
                HolderMark {
                    holder: bob.clone(),
                    counter: 7,
                },
            ],
            ..LocalState::default()
        };
        let mut marks = LiveMarks::from_local(&local);
        assert_eq!(marks.presence.len(), 1);
        assert_eq!(marks.presence[&bob], 9);
        // Whatever key signed it: an older status stays refused.
        assert_eq!(marks.presence(&bob, 8), Mark::Stale);
        assert_eq!(marks.presence(&bob, 9), Mark::Seen);
        assert_eq!(marks.presence(&bob, 10), Mark::Raised);
    }

    #[test]
    fn a_removed_device_keeps_its_presence_mark() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let b = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("x.rs", 1, 1)]));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, b)]);
        let shown = a1.engine.live.presence[&b1.holder];
        let remove = a1
            .engine
            .remove_member(&MemberId::new("bob").unwrap())
            .unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, remove)]);
        assert_eq!(connected(&a1), (2, 1, 1));
        assert!(a1.engine.activity().peers.is_empty());
        // Admitted again with the same device id before a rotation, its old status stays old.
        assert_eq!(a1.engine.live.presence.get(&b1.holder), Some(&shown));
    }

    #[test]
    fn overlaps_count_only_for_sessions_the_teammates_status_lists() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let a = a1
            .engine
            .set_share(share("Alice", 1, "Claude Code", &[("src/auth.rs", 10, 12)]));
        let b = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("src/auth.rs", 14, 15)]));
        let old = claim_set(&b);
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, a), (1, b)]);
        assert_eq!(a1.engine.activity().overlaps.len(), 1);

        // Bob's session ends: claim.clear and a status without it.
        let ended = b1.engine.set_share(LocalShare {
            person: "Bob".to_owned(),
            sessions: Vec::new(),
        });
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, ended)]);
        assert!(a1.engine.activity().overlaps.is_empty());

        // The relay sends the ended session's last detail again, and a session key it made up.
        let mine = a1.engine.session_keys.get(&1).unwrap().clone();
        let overlap = |session: SessionKey, detail: Option<Sealed>| {
            relay.text(
                None,
                None,
                ServerMessage::ClaimOverlap(ClaimOverlap {
                    repo: old.repo,
                    session: mine.clone(),
                    other: SessionRef {
                        holder: b1.holder.clone(),
                        session,
                    },
                    level: MatchLevel::SameFile,
                    file: Some(old.files[0].file),
                    files: 1,
                    detail,
                }),
            )
        };
        let replay = overlap(old.session.clone(), old.detail.clone());
        let invented = overlap(SessionKey::new("made-up").unwrap(), None);
        let _ = a1.engine.on_text(&replay);
        let _ = a1.engine.on_text(&invented);
        assert!(a1.engine.activity().overlaps.is_empty());
        assert!(a1.engine.overlaps.is_empty());

        // Without any status from bob's device: kept, but the same area at most (no notice).
        a1.engine.peers.clear();
        let _ = a1.engine.on_text(&invented);
        let shown = a1.engine.activity().overlaps;
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].level, OverlapLevel::SameArea);
        assert_eq!(shown[0].path, None);
        // A presence the relay reports without a status vouches for no session either: still
        // the same area at most, not dropped as if a signed status had ended the session.
        let away = relay.text(
            None,
            None,
            ServerMessage::PresenceUpdate(PresenceUpdate {
                holder: b1.holder.clone(),
                state: PresenceState::Away,
                status: None,
                at: UnixMillis(3),
            }),
        );
        let _ = a1.engine.on_text(&away);
        let _ = a1.engine.on_text(&invented);
        let shown = a1.engine.activity().overlaps;
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].level, OverlapLevel::SameArea);
        // Bob's next status, without that key, ends it.
        let status = b1
            .engine
            .set_share(share("Bob", 8, "Codex", &[("docs/x.md", 1, 1)]));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, status)]);
        assert!(a1
            .engine
            .activity()
            .overlaps
            .iter()
            .all(|o| o.other_session != "made-up"));
    }

    #[test]
    fn a_presence_without_a_status_keeps_the_sessions() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let b = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("x.rs", 1, 1)]));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, b)]);
        let away = relay.text(
            None,
            None,
            ServerMessage::PresenceUpdate(PresenceUpdate {
                holder: b1.holder.clone(),
                state: PresenceState::Away,
                status: None,
                at: UnixMillis(2),
            }),
        );
        let _ = a1.engine.on_text(&away);
        assert_eq!(peer_files(&a1), Some(vec!["x.rs".to_owned()]));
        assert_eq!(a1.engine.activity().peers[0].presence, "away");
        assert_eq!(a1.engine.teammate_files().len(), 1);
    }

    #[test]
    fn rising_marks_are_stored_at_most_every_few_seconds() {
        let (relay, mut a1, mut b1) = team_of_two();
        let clock = Arc::new(std::sync::atomic::AtomicU64::new(10_000_000));
        let read = Arc::clone(&clock);
        a1.engine.now = Box::new(move || read.load(std::sync::atomic::Ordering::SeqCst));
        let advance = |ms: u64| {
            clock.fetch_add(ms, std::sync::atomic::Ordering::SeqCst);
        };
        let stores = |effects: &[Effect]| {
            effects
                .iter()
                .filter(|e| matches!(e, Effect::StoreLocal))
                .count()
        };
        let mut status = |file: &str| {
            let effects = b1
                .engine
                .set_share(share("Bob", 7, "Codex", &[(file, 1, 1)]));
            presence_text(&relay, &b1.holder, Some(presence_status(&effects)))
        };
        let one = status("one.rs");
        let two = status("two.rs");
        let three = status("three.rs");

        assert_eq!(
            stores(&a1.engine.on_text(&one)),
            1,
            "the first rise is stored"
        );
        advance(1_000);
        assert_eq!(
            stores(&a1.engine.on_text(&two)),
            0,
            "a second later: not yet"
        );
        let stored = a1.engine.local().presence_marks.clone();
        assert_ne!(LiveMarks::from_local(a1.engine.local()), a1.engine.live);
        // The next ping writes what is left, all marks together.
        assert_eq!(stores(&a1.engine.ping()), 1);
        assert_ne!(a1.engine.local().presence_marks, stored);
        assert_eq!(LiveMarks::from_local(a1.engine.local()), a1.engine.live);
        assert_eq!(stores(&a1.engine.ping()), 0, "nothing left");
        advance(MARKS_STORE_EVERY_MS);
        assert_eq!(stores(&a1.engine.on_text(&three)), 1);
        assert!(a1.engine.flush_marks().is_empty());
    }

    #[test]
    fn teammate_files_read_each_sessions_lines_from_its_first_detail() {
        let (_relay, mut a1, b1) = team_of_two();
        let mut peer = peer_with_sessions(&["k"]);
        peer.status.sessions[0].files = vec!["src/a.rs".to_owned(), "SRC/B.rs".to_owned()];
        a1.engine.peers.clear();
        a1.engine.peers.insert(b1.holder.clone(), peer);
        let other = SessionRef {
            holder: b1.holder.clone(),
            session: SessionKey::new("k").unwrap(),
        };
        let detail = |claims: &[(&str, u32, u32)]| OverlapEntry {
            level: MatchLevel::SameFile,
            files: 1,
            hint: None,
            detail: Some(OtherDetail {
                name: "Bob".to_owned(),
                agent: "Codex".to_owned(),
                title: String::new(),
                base: None,
                claims: claims
                    .iter()
                    .map(|&(path, start, end)| crate::claims::Claim {
                        path: path.to_owned(),
                        kind: crate::claims::ChangeKind::Modified,
                        whole_file: false,
                        ranges: vec![range(start, end)],
                        area: crate::claims::area_of(path),
                    })
                    .collect(),
            }),
        };
        // Two of this device's sessions overlap with the same session of bob's.
        a1.engine.overlaps.entry(1).or_default().insert(
            other.clone(),
            detail(&[("src/a.rs", 1, 2), ("src/b.rs", 5, 6)]),
        );
        a1.engine
            .overlaps
            .entry(2)
            .or_default()
            .insert(other, detail(&[("src/a.rs", 9, 9)]));
        let files = a1.engine.teammate_files();
        let lines = |key: &str| {
            files
                .iter()
                .find(|f| f.key == key)
                .map(|f| f.lines.clone())
                .unwrap()
        };
        assert_eq!(files.len(), 2);
        assert_eq!(lines("src/a.rs"), [range(1, 2)]);
        assert_eq!(lines("src/b.rs"), [range(5, 6)]);
    }
}
