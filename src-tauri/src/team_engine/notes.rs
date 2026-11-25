//! The team's notes key ring on a connection (ADR 0006, phase 2: "Phase 2 design").
//!
//! The relay keeps one `notes-keys` record per team: the key ring sealed under the current epoch
//! by an owner's or admin's device. After every `welcome` of protocol version 2, once this device
//! holds the current epoch's secret, the engine fetches it (`notes.keys.get`) and keeps the union
//! of every ring it accepted ([`NotesKeys::ring`]); a hint (`notes.keys.updated`) fetches again.
//! An owner's or admin's device writes a new record (`notes.keys.set`, compare and set on the
//! relay's version) when the record is missing while it holds a complete ring, when the ring's
//! newest key was put in place under an older epoch than the current one (a rotation: one new key
//! first, also when the relay shows no record or one that does not count), or when the record
//! lacks keys this device holds or comes from a member who is no longer owner or admin or a
//! device the log no longer lists.
//!
//! A new key is used only once the relay stored it: until the `ack` it waits in
//! [`NotesKeys::pending`], so no note is ever written with a key the team cannot fetch.
//! Nothing here touches files or the keychain; the app's notes read [`NotesKeysView`].

use hangar_team_crypto::notes::{self as notes_crypto, KeyRing, NotesCryptoError, NotesKeysError};
use hangar_team_crypto::record::{EpochCipher, OpenError};
use hangar_team_protocol::ids::Epoch;
use hangar_team_protocol::wire::{
    ClientMessage, ErrorCode, NotesKeys as NotesKeysAnswer, NotesKeysRecord, NotesKeysSet,
};

use super::{Effect, Engine, Lend, Op};
use crate::team::{TeamError, TeamStatus};

/// Refused or conflicting `notes.keys.set` per connection before this device waits for the next
/// connection; a relay that refuses every write cannot make it loop.
const MAX_SET_FAILURES: u32 = 3;

/// What the app's shared notes see of the team's key ring.
#[derive(Debug, Clone, Default)]
pub struct NotesKeysView {
    /// Every key this run accepted from the team or stored there; `None` before the first.
    pub ring: Option<KeyRing>,
    /// Whether the relay speaks protocol version 2 on the current connection; `None` while not
    /// connected or not known yet.
    pub speaks: Option<bool>,
    /// Whether the team has a key ring on the relay; `None` until asked on this connection.
    pub exists: Option<bool>,
    /// This device may turn shared notes on (an admitted owner's or admin's device).
    pub manager: bool,
    /// The ring's newest key is not closed to everyone who left: the verified log names a
    /// removal whose rotation has not landed yet, or the newest key was last confirmed under an
    /// older epoch than the log's current one. An owner or admin still has to add the key that
    /// shuts out whoever left; notes wait with saving until then. Taken from the verified log,
    /// not from the connection, so neither a relay that withholds the record or speaks version 1
    /// nor a status that waits for the key change turns it off.
    pub renewing: bool,
    /// The newest epoch under which the ring's newest key was confirmed (`None`: not in this
    /// run). The app keeps it with its keychain cache, so the save rule holds across restarts.
    pub fresh: Option<Epoch>,
    /// The verified log's current epoch, once this run verified the log (any protocol version).
    pub log_epoch: Option<Epoch>,
    /// The verified log names a removal whose rotation has not landed yet.
    pub urgent: bool,
}

/// What became of the record the relay showed on this connection.
#[derive(Debug, Clone)]
enum Seen {
    /// The team has no ring yet.
    Absent,
    /// Opened and merged; `holds_all`: the record holds every key this device has.
    Opened { version: u64, holds_all: bool },
    /// Signed and sealed, but by a member who is no longer owner or admin, or by a device the
    /// log no longer lists.
    NotManager { version: u64 },
    /// Names a log entry this device has not verified yet; opened again once a log fetch
    /// brought this device past `at` (its log head when the record came), else unreadable.
    Ahead { record: NotesKeysRecord, at: u64 },
    /// Does not open here (an epoch this device does not keep, a forged key, a broken record).
    Unreadable,
}

/// The turn-on or solo-to-team request the window waits for.
#[derive(Debug)]
struct Enable {
    /// The solo vault's ring to bring into the team's, with a new key on top.
    adopt: Option<KeyRing>,
}

/// The notes key ring's state in an engine.
#[derive(Debug, Default)]
pub(super) struct NotesKeys {
    /// The union of every ring accepted or stored, for this engine's lifetime.
    ring: Option<KeyRing>,
    /// The newest epoch under which a record that put `ring`'s newest key in place was accepted
    /// or stored. That key is closed only to whoever left before this epoch; below the current
    /// epoch the next write adds a key, whatever the relay shows (ADR 0006, phase 2 design,
    /// section 4).
    fresh: Option<Epoch>,
    // Per connection.
    proto: u16,
    fetching: bool,
    fetch_again: bool,
    seen: Option<Seen>,
    /// A `notes.keys.set` waits for its answer, with the ring it carries and its epoch.
    pending: Option<(KeyRing, Epoch)>,
    failures: u32,
    enable: Option<Enable>,
    /// The answer to the last turn-on request, for the worker to hand to the window.
    outcome: Option<Result<(), TeamError>>,
    /// This computer's keychain keeps notes keys of the team: the team had a ring before, so a
    /// ring the relay does not show is missing, not new, and none is made for decisions.
    cached: bool,
}

impl NotesKeys {
    /// A new connection: what the relay said goes; the ring stays.
    pub(super) fn new_connection(&mut self) {
        self.proto = 0;
        self.fetching = false;
        self.fetch_again = false;
        self.seen = None;
        self.pending = None;
        self.failures = 0;
        if self.enable.take().is_some() {
            self.outcome = Some(Err(TeamError::NotesNotReady));
        }
    }

    pub(super) fn welcome(&mut self, proto: u16) {
        self.proto = proto;
    }

    /// Every key this run accepted or stored; the team's decisions open with it.
    pub(super) fn ring(&self) -> Option<&KeyRing> {
        self.ring.as_ref()
    }
}

impl Engine {
    fn notes_epoch(&self) -> Option<Epoch> {
        match self.status {
            TeamStatus::Connected { epoch, .. } => Some(Epoch(epoch)),
            _ => None,
        }
    }

    fn notes_speaks(&self) -> bool {
        self.notes.proto >= 2
    }

    /// What the app's shared notes see.
    pub fn notes_view(&self) -> NotesKeysView {
        let connected = self.notes_epoch();
        let renewing = self.notes_renewing();
        NotesKeysView {
            ring: self.notes.ring.clone(),
            speaks: connected.map(|_| self.notes_speaks()),
            exists: match &self.notes.seen {
                None => None,
                Some(Seen::Absent) => Some(false),
                Some(_) => Some(true),
            },
            manager: self.manages(),
            renewing,
            fresh: self.notes.fresh,
            log_epoch: self
                .verifier
                .state()
                .and_then(|state| state.current_epoch().map(|e| e.epoch)),
            urgent: self
                .verifier
                .state()
                .is_some_and(|state| state.rotation_urgent()),
        }
    }

    /// The ring's newest key is not closed to everyone who left yet ([`NotesKeysView::renewing`]).
    pub(super) fn notes_renewing(&self) -> bool {
        self.notes.ring.is_some()
            && self.verifier.state().is_some_and(|state| {
                state.rotation_urgent()
                    || state.current_epoch().is_some_and(|current| {
                        self.notes.fresh.is_none_or(|fresh| fresh < current.epoch)
                    })
            })
    }

    /// Whether this computer's keychain keeps notes keys of the team (the worker reads it before
    /// every connection).
    pub fn set_notes_cached(&mut self, cached: bool) {
        self.notes.cached = cached;
    }

    /// The answer to the last [`Engine::notes_enable`], once there is one.
    pub fn take_notes_outcome(&mut self) -> Option<Result<(), TeamError>> {
        self.notes.outcome.take()
    }

    /// Called at the end of every reconcile: fetch the record once per connection, open one that
    /// waited for the log, and write a new one where due.
    pub(super) fn notes_step(&mut self) -> Vec<Effect> {
        if self.notes_epoch().is_none() || !self.notes_speaks() {
            return Vec::new();
        }
        let mut effects = Vec::new();
        if let Some(Seen::Ahead { record, at }) = self.notes.seen.clone() {
            // Once per log fetch: a record naming an entry the relay never hands over must not
            // keep this device fetching the log.
            if !self.syncing {
                let head = self.verifier.state().map_or(0, |s| s.head().index);
                if head > at {
                    effects.extend(self.notes_open(record));
                } else {
                    eprintln!(
                        "hangar: team notes keys name a log entry the relay did not hand over"
                    );
                    self.notes.seen = Some(Seen::Unreadable);
                }
            }
        }
        if self.notes.seen.is_none() {
            effects.extend(self.notes_fetch());
            return effects;
        }
        effects.extend(self.notes_write_due());
        effects
    }

    /// `notes.keys.get`, or once more after the running one.
    fn notes_fetch(&mut self) -> Vec<Effect> {
        if self.notes.fetching {
            self.notes.fetch_again = true;
            return Vec::new();
        }
        self.notes.fetching = true;
        vec![self.frame(Op::NotesGet, ClientMessage::NotesKeysGet)]
    }

    /// `notes.keys.updated`: a hint only; the record decides.
    pub(super) fn on_notes_updated(&mut self) -> Vec<Effect> {
        if self.notes_epoch().is_none() || !self.notes_speaks() {
            return Vec::new();
        }
        self.notes_fetch()
    }

    pub(super) fn on_notes_keys(&mut self, answer: NotesKeysAnswer) -> Vec<Effect> {
        self.notes.fetching = false;
        let mut effects = vec![Effect::Changed];
        match answer.record {
            None => self.notes.seen = Some(Seen::Absent),
            Some(record) => effects.extend(self.notes_open(record)),
        }
        if std::mem::take(&mut self.notes.fetch_again) {
            effects.extend(self.notes_fetch());
            return effects;
        }
        effects.extend(self.notes_write_due());
        // Decisions that waited for a notes key open now.
        effects.extend(self.decisions_step());
        effects
    }

    /// Opens `record` against the verified log and merges its keys.
    fn notes_open(&mut self, record: NotesKeysRecord) -> Vec<Effect> {
        let version = record.version;
        let epoch = record.content.epoch;
        let Some(state) = self.verifier.state() else {
            self.notes.seen = Some(Seen::Unreadable);
            return Vec::new();
        };
        let Some(secret) = self.keys.epochs.get(&epoch) else {
            self.notes.seen = Some(Seen::Unreadable);
            return Vec::new();
        };
        let cipher = match EpochCipher::for_epoch(state, secret, epoch) {
            Ok(cipher) => cipher,
            Err(error) => {
                eprintln!("hangar: team notes keys not opened: {error}");
                self.notes.seen = Some(Seen::Unreadable);
                return Vec::new();
            }
        };
        let head = state.head().index;
        match notes_crypto::open_notes_keys(&cipher, state, &record.by, &record.content) {
            Ok(ring) => {
                match self.notes_accept(&ring, epoch) {
                    Ok(()) => {
                        let holds_all = self
                            .notes
                            .ring
                            .as_ref()
                            .is_some_and(|mine| ring.contains_all(mine));
                        self.notes.seen = Some(Seen::Opened { version, holds_all });
                    }
                    Err(error) => {
                        // A key id with other bytes than one already accepted: one is forged.
                        eprintln!("hangar: team notes keys refused: {error}");
                        self.notes.seen = Some(Seen::Unreadable);
                    }
                }
                Vec::new()
            }
            Err(NotesKeysError::Open(OpenError::AheadOfLog { .. })) => {
                self.notes.seen = Some(Seen::Ahead { record, at: head });
                self.sync_log()
            }
            Err(NotesKeysError::Open(OpenError::Fork { .. })) => {
                // Signed under the current epoch's key over another log history: the relay shows
                // the two devices different logs, handled as for every other record.
                self.notes.seen = Some(Seen::Unreadable);
                self.diverged(&record.by)
            }
            Err(NotesKeysError::NotManager | NotesKeysError::SenderRemoved) => {
                self.notes.seen = Some(Seen::NotManager { version });
                Vec::new()
            }
            Err(error) => {
                eprintln!("hangar: team notes keys not opened: {error}");
                self.notes.seen = Some(Seen::Unreadable);
                Vec::new()
            }
        }
    }

    /// Merges `ring`, accepted or stored under `epoch`, and keeps [`NotesKeys::fresh`]: keys it
    /// adds become the newest, so the newest key is then as fresh as `epoch`; a ring that adds
    /// nothing but ends in the same newest key confirms that key under `epoch`.
    fn notes_accept(&mut self, ring: &KeyRing, epoch: Epoch) -> Result<(), NotesCryptoError> {
        let added = match &mut self.notes.ring {
            None => {
                self.notes.ring = Some(ring.clone());
                true
            }
            Some(mine) => mine.merge(ring)?,
        };
        if added {
            self.notes.fresh = Some(epoch);
        } else if self
            .notes
            .ring
            .as_ref()
            .is_some_and(|mine| mine.newest().id() == ring.newest().id())
        {
            self.notes.fresh = self.notes.fresh.max(Some(epoch));
        }
        Ok(())
    }

    /// Writes a new record where one is due (module docs), or the turn-on request's.
    fn notes_write_due(&mut self) -> Vec<Effect> {
        if self.notes.pending.is_some()
            || self.notes.fetching
            || self.notes.failures >= MAX_SET_FAILURES
            || !self.manages()
        {
            if self.notes.failures >= MAX_SET_FAILURES && self.notes.enable.take().is_some() {
                self.notes.outcome = Some(Err(TeamError::Busy));
            }
            return Vec::new();
        }
        let Some(current) = self.notes_epoch() else {
            return Vec::new();
        };
        let Some(seen) = self.notes.seen.clone() else {
            return Vec::new();
        };
        if let Some(enable) = self.notes.enable.take() {
            return self.notes_enable_now(enable, &seen);
        }
        let Some(mut ring) = self.notes.ring.clone() else {
            // Protocol version 4: the team's decisions are sealed with the notes key ring, so an
            // owner's or admin's device makes the team's first ring when there is none
            // (ADR 0021). Only for a team that shows no trace of an earlier ring: no decision on
            // the relay and no cached key here. Otherwise the relay may be hiding the ring, and
            // a new one would split the team's keys; the Team view then waits for the keys.
            // Shared notes still turn on per folder; this only puts the keys in place.
            if matches!(seen, Seen::Absent)
                && self.decisions_spoken()
                && self.decisions_found_none()
                && !self.notes.cached
            {
                return match KeyRing::generate(&mut Lend(self.entropy.as_mut())) {
                    Ok(ring) => self.notes_send(ring, 0),
                    Err(error) => {
                        eprintln!("hangar: no team notes key: {error}");
                        Vec::new()
                    }
                };
            }
            return Vec::new();
        };
        // Whatever the relay shows (no record, an old one, one that no longer counts): a newest
        // key from before the current epoch may be in the hands of whoever left.
        let stale = self.notes.fresh.is_none_or(|fresh| fresh < current);
        let base = match seen {
            Seen::Absent => 0,
            Seen::Opened {
                version, holds_all, ..
            } => {
                if !stale && holds_all {
                    return Vec::new();
                }
                version
            }
            Seen::NotManager { version } => version,
            Seen::Ahead { .. } | Seen::Unreadable => return Vec::new(),
        };
        if stale {
            // A rotation since: one new key, closed to whoever left. A full ring keeps its newest
            // key (ADR 0006, phase 2 design, section 4).
            if let Err(error) = ring.add_generated(&mut Lend(self.entropy.as_mut())) {
                eprintln!("hangar: no new team notes key: {error}");
            }
        }
        self.notes_send(ring, base)
    }

    /// Turns shared notes on for the team, or brings a solo vault's ring in (`adopt`). Answered
    /// through [`Engine::take_notes_outcome`] once the relay stored the ring (or at once when
    /// the team already has one and nothing is adopted).
    pub fn notes_enable(&mut self, adopt: Option<KeyRing>) -> Result<Vec<Effect>, TeamError> {
        if self.notes_epoch().is_none() {
            return Err(TeamError::NotesNotReady);
        }
        if !self.notes_speaks() {
            return Err(TeamError::NotesUnsupported);
        }
        if !self.manages() {
            return Err(TeamError::NotOwnerOrAdmin);
        }
        if self.notes.enable.is_some() || self.notes.pending.is_some() {
            return Err(TeamError::Busy);
        }
        self.notes.outcome = None;
        self.notes.failures = 0;
        self.notes.enable = Some(Enable { adopt });
        let Some(seen) = self.notes.seen.clone() else {
            // The fetch is on its way; the answer runs the request.
            return Ok(self.notes_fetch());
        };
        if self.notes.fetching {
            return Ok(Vec::new());
        }
        let Some(enable) = self.notes.enable.take() else {
            return Ok(Vec::new());
        };
        Ok(self.notes_enable_now(enable, &seen))
    }

    fn notes_enable_now(&mut self, enable: Enable, seen: &Seen) -> Vec<Effect> {
        let entropy = &mut Lend(self.entropy.as_mut());
        let (mut ring, base) = match seen {
            Seen::Absent => match &enable.adopt {
                Some(solo) => (solo.clone(), 0),
                None => match KeyRing::generate(entropy) {
                    Ok(ring) => (ring, 0),
                    Err(error) => {
                        eprintln!("hangar: no team notes key: {error}");
                        self.notes.outcome = Some(Err(TeamError::Busy));
                        return vec![Effect::Changed];
                    }
                },
            },
            Seen::Opened { version, .. } => {
                let Some(mut ring) = self.notes.ring.clone() else {
                    self.notes.outcome = Some(Err(TeamError::NotesNotReady));
                    return vec![Effect::Changed];
                };
                match &enable.adopt {
                    // The team has a ring already: nothing to write.
                    None => {
                        self.notes.outcome = Some(Ok(()));
                        return vec![Effect::Changed];
                    }
                    Some(solo) => {
                        if ring.merge(solo).is_err() {
                            self.notes.outcome = Some(Err(TeamError::NotesNotReady));
                            return vec![Effect::Changed];
                        }
                    }
                }
                (ring, *version)
            }
            // A ring this device cannot read: merging into it could drop keys.
            Seen::NotManager { .. } | Seen::Ahead { .. } | Seen::Unreadable => {
                self.notes.outcome = Some(Err(TeamError::NotesNotReady));
                return vec![Effect::Changed];
            }
        };
        if enable.adopt.is_some() {
            // The solo vault's code still opens what was written before; what follows is the
            // team's alone.
            if let Err(error) = ring.add_generated(entropy) {
                eprintln!("hangar: no new team notes key: {error}");
            }
        }
        self.notes.enable = Some(enable);
        self.notes_send(ring, base)
    }

    /// Seals `ring` under the current epoch and sends it with `base`; the counter is stored
    /// first.
    fn notes_send(&mut self, ring: KeyRing, base: u64) -> Vec<Effect> {
        let Some((cipher, head, _)) = self.current_cipher() else {
            return Vec::new();
        };
        let epoch = cipher.epoch();
        let Some(me) = self.me.clone() else {
            return Vec::new();
        };
        let Some(counter) = self.counter.take() else {
            return Vec::new();
        };
        self.local.counter = self.counter.last();
        let sealed = notes_crypto::seal_notes_keys(
            &cipher,
            &me,
            &self.identity,
            &head,
            counter,
            &ring,
            &mut Lend(self.entropy.as_mut()),
        );
        let content = match sealed {
            Ok(content) => content,
            Err(error) => {
                eprintln!("hangar: team notes keys not sealed: {error}");
                if self.notes.enable.take().is_some() {
                    self.notes.outcome = Some(Err(TeamError::Busy));
                }
                return vec![Effect::Changed];
            }
        };
        self.notes.pending = Some((ring, epoch));
        vec![
            Effect::StoreLocal,
            self.frame(
                Op::NotesSet,
                ClientMessage::NotesKeysSet(NotesKeysSet { base, content }),
            ),
        ]
    }

    /// The relay stored the ring: its keys are the team's now.
    pub(super) fn on_notes_stored(&mut self) -> Vec<Effect> {
        if let Some((stored, epoch)) = self.notes.pending.take() {
            if let Err(error) = self.notes_accept(&stored, epoch) {
                eprintln!("hangar: team notes keys not merged: {error}");
            }
        }
        if self.notes.enable.take().is_some() {
            self.notes.outcome = Some(Ok(()));
        }
        // The stored version is the base of the next write.
        let mut effects = vec![Effect::Changed];
        self.notes.seen = None;
        effects.extend(self.notes_fetch());
        effects.extend(self.decisions_step());
        effects
    }

    pub(super) fn on_notes_refused(&mut self, set: bool, code: ErrorCode) -> Vec<Effect> {
        if !set {
            self.notes.fetching = false;
            eprintln!("hangar: team notes keys not fetched ({code:?})");
            return Vec::new();
        }
        self.notes.pending = None;
        self.notes.failures = self.notes.failures.saturating_add(1);
        eprintln!("hangar: team notes keys not stored ({code:?})");
        if code == ErrorCode::Conflict && self.notes.failures < MAX_SET_FAILURES {
            // Another device wrote first: fetch, merge, decide again.
            self.notes.seen = None;
            return self.notes_fetch();
        }
        if self.notes.enable.take().is_some() {
            self.notes.outcome = Some(Err(match code {
                ErrorCode::Forbidden => TeamError::NotOwnerOrAdmin,
                _ => TeamError::Busy,
            }));
        }
        vec![Effect::Changed]
    }
}
