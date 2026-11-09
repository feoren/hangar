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

use std::collections::{BTreeMap, HashMap};

use hangar_team_crypto::identity::{safety_number, DeviceIdentity, DevicePublicKeys};
use hangar_team_crypto::memberlog::{
    DeviceEntry, Endorsement, EntryBody, EntryKind, LogVerifier, TeamState,
};
use hangar_team_crypto::record::{Opened, ReplayGuard, SendCounter};
use hangar_team_crypto::rotation::{plan_rotation, wrap_for_device};
use hangar_team_crypto::{Entropy, EntropyError};
use hangar_team_protocol::e2e::{ChainHead, PublicKey};
use hangar_team_protocol::ids::{
    Ciphertext, DeviceId, Epoch, MemberId, OpId, TeamId, Ticket, UnixMillis,
};
use hangar_team_protocol::log::{ClientCursor, Observed};
use hangar_team_protocol::wire::{
    check_welcome, decode_server, ClientFrame, ClientMessage, DeviceInfo, DeviceRemove, ErrorCode,
    Hello, Holder, KeyGet, MemberLogEntries, MemberLogGet, ProtoRange, Role, ServerFrame,
    ServerMessage, TicketRenew,
};
use sha2::{Digest as _, Sha256};

use crate::team::{
    hex, safety_groups, AttentionReason, LocalState, PendingConfirmation, PendingKind, ReplayMark,
    SessionEnd, StopReason, StoredKeys, TeamError, TeamStatus,
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
    website_role: Role,
    keys: StoredKeys,
    local: LocalState,
    now: Box<dyn Fn() -> u64 + Send + Sync>,
    entropy: EntropySource,
    counter: SendCounter,
    guard: ReplayGuard,
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
            project: setup.project,
            team: setup.team,
            device: setup.device,
            identity: setup.identity,
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
        let hello = ClientMessage::Hello(Hello {
            ticket,
            device: self.device.clone(),
            proto: ProtoRange::CURRENT,
            last_seq: self.cursor.cursor().cloned(),
        });
        vec![self.frame(Op::Hello, hello), Effect::Changed]
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

    pub fn ping(&mut self) -> Vec<Effect> {
        vec![self.frame(Op::Quiet, ClientMessage::Ping)]
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
            // Presence, reservations, claims and notifications are step F part 2b.
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
    // Record counters (sealed records come with step F part 2b)
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
}
