//! The team's decisions on a connection (ADR 0021, protocol version 4).
//!
//! After every `welcome` of version 4 the engine lists the team's decision blobs from the first
//! (`blob.list`, paged, at most 100 per page, and no further than a little above what a team may
//! keep); the full listing drops what the team no longer has, so a withdrawal missed while
//! offline goes too. The live hints `blob.stored` and `blob.deleted` are not logged: a stored
//! hint lists again from the newest blob known, a deleted hint forgets the id. A connection
//! starts at most one listing per [`BLOB_LIST_EVERY_MS`]; a hint within that time waits for the
//! next ping.
//!
//! Each listed blob is fetched once (`blob.get`, a few at a time; larger than a decision can be:
//! unreadable) and opened with the team's notes key ring against the verified log
//! (`hangar_team_crypto::decision`). A decision that cannot open yet keeps only what it waits for
//! (the notes key it names, or the log head) and is fetched again once that arrived. Opened
//! decisions stay in memory across connections; content never touches the disk.
//!
//! Decisions are named by writer and id together (`team_decisions::Key`), commands by the
//! relay's blob id, so a member who reuses another's id hides nothing and an owner's Withdraw
//! deletes exactly the blob the window showed.
//!
//! Recording seals the document with the ring's newest key under the current epoch and sends
//! `blob.put`; its answer settles the window's request. Withdrawing stores a signed withdrawal
//! first and then deletes the decision, so a relay that serves the deleted blob again does not
//! bring it back; whatever a replacement or withdrawal hid stays hidden for this engine's life.
//! Old withdrawals of decisions the relay no longer lists are cleaned up after
//! [`WITHDRAWAL_KEPT_MS`]. Decisions of writers the verified log no longer lists show as a former
//! member's and never reach an agent's context. On a relay older than version 4 nothing here
//! sends anything.

use std::collections::{BTreeMap, BTreeSet};

use hangar_team_crypto::decision::{self as decision_crypto, DecisionError};
use hangar_team_crypto::notes::KeyId;
use hangar_team_crypto::record::OpenError;
use hangar_team_protocol::ids::{BlobId, Sealed};
use hangar_team_protocol::limits::{
    BLOB_LIST_EVERY_MS, MAX_DECISIONS, MAX_DECISIONS_PER_MEMBER, MAX_DECISION_CIPHERTEXT_LEN,
};
use hangar_team_protocol::wire::{
    BlobDelete, BlobEntry, BlobGet, BlobKind, BlobList, BlobListing, BlobPut, ClientMessage,
    ErrorBody, ErrorCode, Holder, Role,
};
use hangar_team_protocol::DECISIONS_PROTOCOL_VERSION;

use super::{Effect, Engine, Lend, Op};
use crate::team::{TeamError, TeamStatus};
use crate::team_decisions::{
    self, Decision, DecisionView, DecisionsState, DraftError, Key, Listed, TeamDecision,
};

/// `blob.get` requests in flight at once: a team's 200 decisions after a restart stay well
/// within the relay's frame rate.
const MAX_FETCHING: usize = 8;
/// A listing stops this far above what a team may keep: a relay that lists without end cannot
/// make this device fetch without end.
const LIST_MARGIN: usize = 50;
/// How long a withdrawal stays after the decision it withdrew is gone (30 days): long enough
/// that a relay restored from a recent backup does not bring the decision back.
pub const WITHDRAWAL_KEPT_MS: u64 = 30 * 24 * 60 * 60 * 1000;

/// What became of one listed blob.
#[derive(Debug, Clone)]
enum State {
    /// Listed, not fetched (again) yet.
    Listed,
    /// `blob.get` is on its way.
    Fetching,
    /// Fetched, waits for the notes key `key` (or for the ring and the log at all, `None`), or
    /// for a log fetch to pass the index `ahead`; then it is fetched again.
    Waiting {
        key: Option<KeyId>,
        ahead: Option<u64>,
    },
    Open(Decision),
    /// Does not open here, or is not a decision document of this version.
    Unreadable,
}

#[derive(Debug, Clone)]
struct Entry {
    blob: BlobId,
    /// The device that stored it (the relay's word; the seal's signature must verify for it).
    by: Holder,
    /// When the relay stored it (its clock; this device's for its own until listed).
    created_at: u64,
    state: State,
}

/// A listing on its way.
#[derive(Debug)]
struct Listing {
    /// From the first blob: at its end, blobs it did not name are gone.
    full: bool,
    seen: BTreeSet<String>,
    order: Vec<String>,
}

/// The window's request the relay's answers settle.
#[derive(Debug)]
enum Request {
    Record {
        decision: Decision,
        /// The blob of the decision it replaces, deleted once this one is stored.
        replaces: Option<(BlobId, Key)>,
        /// The sealed record, sent once more after this device's old withdrawals made room
        /// (`None` once it was).
        retry: Option<Sealed>,
    },
    /// The withdrawal record is on its way (`record`), then the delete of `target`.
    Withdraw {
        target: BlobId,
        key: Key,
        record: Option<Decision>,
    },
}

/// The decisions' state in an engine.
#[derive(Debug, Default)]
pub(super) struct Decisions {
    /// By blob id; kept across connections.
    entries: BTreeMap<String, Entry>,
    /// Blob ids in the relay's order, oldest first.
    order: Vec<String>,
    /// Decisions a replacement or a withdrawal hid; kept for this engine's life.
    hidden: BTreeSet<Key>,
    // Per connection.
    proto: u16,
    listing: Option<Listing>,
    /// A full listing ran to its end on this connection.
    listed: bool,
    /// Another listing is wanted (`Some(true)`: a full one).
    wanted: Option<bool>,
    /// When this connection last started a listing (engine clock).
    started_at: Option<u64>,
    request: Option<Request>,
    outcome: Option<Result<(), TeamError>>,
}

impl Decisions {
    pub(super) fn new_connection(&mut self) {
        self.proto = 0;
        self.listing = None;
        self.listed = false;
        self.wanted = None;
        self.started_at = None;
        for entry in self.entries.values_mut() {
            if matches!(entry.state, State::Fetching) {
                entry.state = State::Listed;
            }
        }
        if self.request.take().is_some() {
            self.outcome = Some(Err(TeamError::Busy));
        }
    }

    pub(super) fn welcome(&mut self, proto: u16) {
        self.proto = proto;
    }

    fn speaks(&self) -> bool {
        self.proto >= DECISIONS_PROTOCOL_VERSION
    }
}

fn blob_id(text: &str) -> Option<BlobId> {
    BlobId::new(text).ok()
}

impl Engine {
    /// The relay of this connection lists decisions (protocol version 4).
    pub(super) fn decisions_spoken(&self) -> bool {
        self.decisions.speaks()
    }

    /// A full listing on this connection found no decision at all: the team never had one, so
    /// it never had a notes key ring for them either.
    pub(super) fn decisions_found_none(&self) -> bool {
        self.decisions.listed && self.decisions.entries.is_empty()
    }

    fn decisions_ready(&self) -> bool {
        matches!(self.status, TeamStatus::Connected { .. }) && self.decisions.speaks()
    }

    /// The answer to the last [`Engine::record_decision`] or [`Engine::withdraw_decision`].
    pub fn take_decision_outcome(&mut self) -> Option<Result<(), TeamError>> {
        self.decisions.outcome.take()
    }

    /// Called at the end of every reconcile and whenever the notes key ring changed: the first
    /// listing of a connection, decisions that waited, and fetches.
    pub(super) fn decisions_step(&mut self) -> Vec<Effect> {
        if !self.decisions_ready() {
            return Vec::new();
        }
        let mut effects = Vec::new();
        if !self.decisions.listed && self.decisions.listing.is_none() {
            effects.extend(self.decisions_list(true));
        }
        if self.decisions_retry() {
            effects.push(Effect::Changed);
        }
        effects.extend(self.decisions_fetch());
        effects
    }

    /// From the ping: a listing that had to wait for the connection's rate.
    pub(super) fn decisions_due(&mut self) -> Vec<Effect> {
        if !self.decisions_ready() || self.decisions.listing.is_some() {
            return Vec::new();
        }
        match self.decisions.wanted {
            Some(full) => self.decisions_list(full),
            None => Vec::new(),
        }
    }

    /// Starts a listing, or remembers it for later while one runs or the rate holds it back.
    fn decisions_list(&mut self, full: bool) -> Vec<Effect> {
        let full = full || !self.decisions.listed || self.decisions.order.is_empty();
        if self.decisions.listing.is_some() {
            let wanted = self.decisions.wanted.unwrap_or(false);
            self.decisions.wanted = Some(wanted || full);
            return Vec::new();
        }
        let now = (self.now)();
        let free = self
            .decisions
            .started_at
            .is_none_or(|at| now < at || now - at >= BLOB_LIST_EVERY_MS);
        if !free {
            let wanted = self.decisions.wanted.unwrap_or(false);
            self.decisions.wanted = Some(wanted || full);
            return Vec::new();
        }
        self.decisions.wanted = None;
        self.decisions.started_at = Some(now);
        let after = if full {
            None
        } else {
            self.decisions.order.last().and_then(|id| blob_id(id))
        };
        self.decisions.listing = Some(Listing {
            full,
            seen: BTreeSet::new(),
            order: Vec::new(),
        });
        vec![self.frame(
            Op::DecisionList,
            ClientMessage::BlobList(BlobList {
                kind: BlobKind::Decision,
                after,
            }),
        )]
    }

    pub(super) fn on_blob_listing(&mut self, page: BlobListing) -> Vec<Effect> {
        if page.kind != BlobKind::Decision {
            return Vec::new();
        }
        let Some(listing) = self.decisions.listing.as_mut() else {
            return Vec::new();
        };
        if page.more && page.blobs.is_empty() {
            eprintln!("hangar: team decision listing without progress");
            self.decisions.listing = None;
            return Vec::new();
        }
        let cap = MAX_DECISIONS + LIST_MARGIN;
        let mut changed = false;
        let mut capped = false;
        for entry in &page.blobs {
            let id = entry.blob.as_str().to_owned();
            if !listing.seen.insert(id.clone()) {
                continue;
            }
            if listing.order.len() >= cap {
                capped = true;
                break;
            }
            listing.order.push(id.clone());
            if let std::collections::btree_map::Entry::Vacant(vacant) =
                self.decisions.entries.entry(id)
            {
                vacant.insert(new_entry(entry));
                changed = true;
            }
        }
        if capped {
            eprintln!("hangar: the team relay lists more decisions than a team keeps");
        }
        let mut effects = Vec::new();
        let next = page.blobs.last().filter(|_| page.more && !capped);
        if let Some(last) = next {
            effects.push(self.frame(
                Op::DecisionList,
                ClientMessage::BlobList(BlobList {
                    kind: BlobKind::Decision,
                    after: Some(last.blob.clone()),
                }),
            ));
        } else if let Some(done) = self.decisions.listing.take() {
            if done.full {
                let gone = self.decisions.entries.len();
                self.decisions
                    .entries
                    .retain(|id, _| done.seen.contains(id) && done.order.contains(id));
                changed |= gone != self.decisions.entries.len();
                self.decisions.order = done.order;
                self.decisions.listed = true;
                effects.extend(self.decisions_collect());
                // A team without any decision may get its notes key ring now (owner or admin).
                effects.extend(self.notes_step());
            } else {
                for id in done.order {
                    if !self.decisions.order.contains(&id) {
                        self.decisions.order.push(id);
                    }
                }
            }
            if let Some(full) = self.decisions.wanted {
                effects.extend(self.decisions_list(full));
            }
        }
        if changed {
            effects.push(Effect::Changed);
        }
        effects.extend(self.decisions_fetch());
        effects
    }

    /// `blob.stored`: a hint only; the listing decides.
    pub(super) fn on_blob_stored(&mut self, kind: BlobKind) -> Vec<Effect> {
        if kind != BlobKind::Decision || !self.decisions_ready() {
            return Vec::new();
        }
        self.decisions_list(false)
    }

    /// `blob.deleted`: the id is gone (the relay's word; it could withhold the decision anyway).
    pub(super) fn on_blob_deleted(&mut self, kind: BlobKind, blob: &BlobId) -> Vec<Effect> {
        if kind != BlobKind::Decision {
            return Vec::new();
        }
        if self.decisions.entries.remove(blob.as_str()).is_none() {
            return Vec::new();
        }
        self.decisions.order.retain(|id| id != blob.as_str());
        vec![Effect::Changed]
    }

    /// `blob.get` for listed blobs, a few at a time.
    fn decisions_fetch(&mut self) -> Vec<Effect> {
        if !self.decisions_ready() {
            return Vec::new();
        }
        let running = self
            .decisions
            .entries
            .values()
            .filter(|e| matches!(e.state, State::Fetching))
            .count();
        let next: Vec<BlobId> = self
            .decisions
            .order
            .iter()
            .rev()
            .filter_map(|id| self.decisions.entries.get(id))
            .filter(|e| matches!(e.state, State::Listed))
            .take(MAX_FETCHING.saturating_sub(running))
            .map(|e| e.blob.clone())
            .collect();
        let mut effects = Vec::new();
        for blob in next {
            if let Some(entry) = self.decisions.entries.get_mut(blob.as_str()) {
                entry.state = State::Fetching;
            }
            effects.push(self.frame(
                Op::DecisionGet(blob.as_str().to_owned()),
                ClientMessage::BlobGet(BlobGet { blob }),
            ));
        }
        effects
    }

    pub(super) fn on_decision_fetched(
        &mut self,
        blob: &str,
        content: Option<Sealed>,
    ) -> Vec<Effect> {
        let Some(by) = self.decisions.entries.get(blob).map(|e| e.by.clone()) else {
            return Vec::new();
        };
        let mut effects = vec![Effect::Changed];
        let state = match content {
            // Larger than any decision: not opened, not kept.
            Some(sealed) if sealed.data.as_str().len() > MAX_DECISION_CIPHERTEXT_LEN => {
                eprintln!("hangar: a team decision blob is larger than a decision");
                State::Unreadable
            }
            Some(sealed) => {
                let (state, more) = self.decision_open(&by, &sealed);
                effects.extend(more);
                state
            }
            None => State::Unreadable,
        };
        if let Some(entry) = self.decisions.entries.get_mut(blob) {
            entry.state = state;
        }
        self.remember_hidden();
        effects.extend(self.decisions_fetch());
        effects
    }

    /// Opens a fetched decision against the verified log; the state it is in afterwards, and
    /// what follows (a log fetch, a divergent sender).
    fn decision_open(&mut self, by: &Holder, sealed: &Sealed) -> (State, Vec<Effect>) {
        let (Some(state), Some(ring)) = (self.verifier.state(), self.notes.ring()) else {
            return (
                State::Waiting {
                    key: None,
                    ahead: None,
                },
                Vec::new(),
            );
        };
        let head = state.head().index;
        match decision_crypto::open_decision(ring, state, by, sealed) {
            Ok(opened) => match team_decisions::parse(&opened.plaintext, &opened.id) {
                Some(decision) => (State::Open(decision), Vec::new()),
                None => {
                    eprintln!("hangar: a team decision is not a document of this version");
                    (State::Unreadable, Vec::new())
                }
            },
            Err(DecisionError::UnknownKey(key)) => (
                State::Waiting {
                    key: Some(key),
                    ahead: None,
                },
                Vec::new(),
            ),
            Err(DecisionError::Record(OpenError::AheadOfLog { .. })) => (
                State::Waiting {
                    key: None,
                    ahead: Some(head),
                },
                self.sync_log(),
            ),
            Err(DecisionError::Record(OpenError::Fork { .. })) => {
                // A fork counts only from the sender's current keys and under the current
                // epoch, as for every other record; an old key period or epoch says nothing
                // about the sender's history now.
                let current_keys = state
                    .key_periods(by)
                    .last()
                    .is_some_and(|p| p.removed_at.is_none() && !p.recovery);
                let current_epoch = state
                    .current_epoch()
                    .is_some_and(|e| e.epoch == sealed.epoch);
                if current_keys && current_epoch {
                    (State::Unreadable, self.diverged(by))
                } else {
                    (State::Unreadable, Vec::new())
                }
            }
            Err(error) => {
                eprintln!("hangar: a team decision did not open: {error}");
                (State::Unreadable, Vec::new())
            }
        }
    }

    /// Fetches again what waited where it can open now: a notes key arrived, or a log fetch
    /// ended. `true` when something changed.
    fn decisions_retry(&mut self) -> bool {
        let head = self.verifier.state().map_or(0, |s| s.head().index);
        let syncing = self.syncing;
        let ready = self.verifier.state().is_some() && self.notes.ring().is_some();
        let ring = self.notes.ring();
        let mut changed = false;
        for entry in self.decisions.entries.values_mut() {
            let State::Waiting { key, ahead } = &entry.state else {
                continue;
            };
            let next = match (key, ahead) {
                // Once per log fetch: a head the relay never hands over makes it unreadable.
                (_, Some(_)) if syncing => continue,
                (_, Some(at)) if head <= *at => State::Unreadable,
                (_, Some(_)) => State::Listed,
                (Some(key), None) if ring.is_some_and(|r| r.get(*key).is_some()) => State::Listed,
                (None, None) if ready => State::Listed,
                _ => continue,
            };
            entry.state = next;
            changed = true;
        }
        changed
    }

    /// The listed decisions this device opened, oldest first, with their entries.
    fn open_entries(&self) -> Vec<(&Decision, &Entry)> {
        self.decisions
            .order
            .iter()
            .filter_map(|id| self.decisions.entries.get(id))
            .filter_map(|e| match &e.state {
                State::Open(decision) => Some((decision, e)),
                _ => None,
            })
            .collect()
    }

    fn is_manager(&self, holder: &Holder) -> bool {
        self.verifier
            .state()
            .and_then(|s| s.member(&holder.member))
            .is_some_and(|m| matches!(m.role, Role::Owner | Role::Admin))
    }

    fn listed_of<'a>(&self, open: &[(&'a Decision, &'a Entry)]) -> Vec<Listed<'a>> {
        open.iter()
            .map(|(decision, entry)| Listed {
                decision,
                member: entry.by.member.as_str(),
                manager: self.is_manager(&entry.by),
            })
            .collect()
    }

    /// The decisions that count, newest first, with their entries.
    fn visible_decisions(&self) -> Vec<(&Decision, &Entry)> {
        let open = self.open_entries();
        let listed = self.listed_of(&open);
        team_decisions::visible(&listed, &self.decisions.hidden)
            .shown
            .into_iter()
            .filter_map(|index| open.get(index).copied())
            .collect()
    }

    /// Keeps what replacements and withdrawals hide now, so the relay cannot bring it back.
    fn remember_hidden(&mut self) {
        let open = self.open_entries();
        let listed = self.listed_of(&open);
        let hidden = team_decisions::visible(&listed, &self.decisions.hidden).hidden;
        self.decisions.hidden.extend(hidden);
    }

    /// The writer of `entry` is no longer in the verified log.
    fn former(&self, holder: &Holder) -> bool {
        self.verifier
            .state()
            .is_some_and(|s| s.member(&holder.member).is_none())
    }

    /// Where the decisions stand, for the Team view.
    pub fn decisions_state(&self) -> DecisionsState {
        if !matches!(self.status, TeamStatus::Connected { .. })
            && !matches!(self.status, TeamStatus::WaitingForKeyChange { .. })
        {
            return DecisionsState::Unknown;
        }
        match self.decisions.proto {
            0 => DecisionsState::Unknown,
            proto if proto < DECISIONS_PROTOCOL_VERSION => DecisionsState::Unsupported,
            _ if self.notes.ring().is_none() => DecisionsState::WaitingForKeys,
            _ => DecisionsState::Ready,
        }
    }

    /// The decisions as the Team view lists them, newest first.
    pub fn decision_views(&self) -> Vec<DecisionView> {
        let mine = self.me.as_ref().map(|me| &me.member);
        let manages = self.manages();
        self.visible_decisions()
            .into_iter()
            .map(|(decision, entry)| {
                let own = Some(&entry.by.member) == mine;
                DecisionView {
                    blob: entry.blob.as_str().to_owned(),
                    id: decision.id.clone(),
                    decision: decision.decision.clone(),
                    reason: decision.reason.clone(),
                    member: entry.by.member.as_str().to_owned(),
                    name: decision.name.clone(),
                    // The relay's clock, as in the agents' context: the writer's own `at` is
                    // its word only and could be backdated.
                    at: entry.created_at,
                    mine: own,
                    former: self.former(&entry.by),
                    can_withdraw: own || manages,
                }
            })
            .collect()
    }

    /// The decisions for the agents' context, newest first: only of writers the verified log
    /// lists now. A former member's decision is the team's to look at, not an agent's to follow
    /// unasked (a removed device could still seal one under an older notes key).
    pub fn team_decisions(&self) -> Vec<TeamDecision> {
        self.visible_decisions()
            .into_iter()
            .filter(|(_, entry)| !self.former(&entry.by))
            .map(|(decision, entry)| TeamDecision {
                decision: decision.decision.clone(),
                reason: decision.reason.clone(),
                who: team_decisions::who(&decision.name, entry.by.member.as_str()),
                at: entry.created_at,
                repo: decision.repo.clone(),
            })
            .collect()
    }

    /// The opened, not withdrawn decision stored as `blob`.
    fn decision_at(&self, blob: &str) -> Option<(&Decision, &Entry)> {
        let entry = self.decisions.entries.get(blob)?;
        match &entry.state {
            State::Open(decision) if !decision.withdrawn => Some((decision, entry)),
            _ => None,
        }
    }

    /// Whether this person may withdraw the decision `entry` holds: its writer, an owner or an
    /// admin (the relay checks the same).
    fn may_withdraw(&self, entry: &Entry) -> bool {
        self.me
            .as_ref()
            .is_some_and(|me| me.member == entry.by.member)
            || self.manages()
    }

    /// Whether this device can write decisions now: connected on version 4, nothing on its way,
    /// the team's notes key ring here, and no key change pending that would leave the newest
    /// notes key in the hands of someone who left.
    fn decisions_usable(&self) -> Result<(), TeamError> {
        if self.decisions.proto != 0 && !self.decisions.speaks() {
            return Err(TeamError::DecisionsUnsupported);
        }
        if !self.decisions_ready() {
            return Err(TeamError::DecisionsNotReady);
        }
        if self.decisions.request.is_some() {
            return Err(TeamError::Busy);
        }
        let urgent = self
            .verifier
            .state()
            .is_some_and(|state| state.rotation_urgent());
        if self.notes.ring().is_none() || urgent || self.notes_renewing() {
            return Err(TeamError::DecisionsNotReady);
        }
        Ok(())
    }

    /// Seals `decision` with the ring's newest key under the current epoch; the counter is
    /// stored first (the caller puts the returned effects before the frame).
    fn seal_decision_doc(&mut self, decision: &Decision) -> Result<(Sealed, Effect), TeamError> {
        let Some(ring) = self.notes.ring().cloned() else {
            return Err(TeamError::DecisionsNotReady);
        };
        let (Some((_, head, epoch)), Some(me)) = (self.current_cipher(), self.me.clone()) else {
            return Err(TeamError::DecisionsNotReady);
        };
        let Some(counter) = self.counter.take() else {
            return Err(TeamError::Busy);
        };
        self.local.counter = self.counter.last();
        let sealed = decision_crypto::seal_decision(
            &ring,
            &self.team,
            epoch,
            &decision.id,
            &me,
            &self.identity,
            &head,
            counter,
            &team_decisions::plaintext(decision),
            &mut Lend(self.entropy.as_mut()),
        )
        .map_err(|error| {
            eprintln!("hangar: team decision not sealed: {error}");
            TeamError::InvalidDecision
        })?;
        Ok((sealed, Effect::StoreLocal))
    }

    fn put_frame(&mut self, sealed: Sealed) -> Effect {
        self.frame(
            Op::DecisionPut,
            ClientMessage::BlobPut(BlobPut {
                kind: BlobKind::Decision,
                content: sealed,
            }),
        )
    }

    /// Records a decision (`replaces`: the blob of one it supersedes, which goes once this one
    /// is stored). Answered through [`Engine::take_decision_outcome`] once the relay stored it.
    pub fn record_decision(
        &mut self,
        decision: &str,
        reason: &str,
        repo: Option<String>,
        replaces: Option<&str>,
    ) -> Result<Vec<Effect>, TeamError> {
        self.record_decision_as(team_decisions::new_id(), decision, reason, repo, replaces)
    }

    fn record_decision_as(
        &mut self,
        id: String,
        decision: &str,
        reason: &str,
        repo: Option<String>,
        replaces: Option<&str>,
    ) -> Result<Vec<Effect>, TeamError> {
        self.decisions_usable()?;
        let (decision, reason) =
            team_decisions::check_text(decision, reason).map_err(|error| match error {
                DraftError::Empty | DraftError::TooLong | DraftError::Characters => {
                    TeamError::InvalidDecision
                }
            })?;
        let mine = self.me.as_ref().map(|me| me.member.clone());
        if self.decisions.entries.len() >= MAX_DECISIONS {
            return Err(TeamError::DecisionLimit);
        }
        // At one person's limit, this person's own withdrawals of decisions that are gone make
        // room first (as many as needed); only real decisions keep it from recording.
        let own = self.own_count();
        let mut room = Vec::new();
        if own >= MAX_DECISIONS_PER_MEMBER {
            let needed = own + 1 - MAX_DECISIONS_PER_MEMBER;
            let spare = self.spare_withdrawals();
            if spare.len() < needed {
                return Err(TeamError::DecisionLimitOwn);
            }
            room = spare.into_iter().take(needed).collect();
        }
        let replaced = match replaces {
            Some(blob) => {
                let (old, entry) = self.decision_at(blob).ok_or(TeamError::UnknownDecision)?;
                if !self.may_withdraw(entry) {
                    return Err(TeamError::NotOwnerOrAdmin);
                }
                Some((
                    entry.blob.clone(),
                    (entry.by.member.as_str().to_owned(), old.id.clone()),
                ))
            }
            None => None,
        };
        let writer = mine
            .as_ref()
            .map(|m| m.as_str().to_owned())
            .unwrap_or_default();
        let record = Decision {
            id,
            decision,
            reason,
            name: self.share.person.clone(),
            at: (self.now)(),
            repo,
            supersedes: replaced.as_ref().map(|(_, key)| key.1.clone()),
            supersedes_member: replaced
                .as_ref()
                .map(|(_, key)| key.0.clone())
                .filter(|member| *member != writer),
            withdrawn: false,
        };
        let (sealed, store) = self.seal_decision_doc(&record)?;
        self.decisions.outcome = None;
        self.decisions.request = Some(Request::Record {
            decision: record,
            replaces: replaced,
            retry: Some(sealed.clone()),
        });
        // The relay takes one connection's frames in order: the deletes land before the put.
        let mut effects = vec![store];
        for blob in room {
            effects.push(self.delete_frame(blob));
        }
        effects.push(self.put_frame(sealed));
        Ok(effects)
    }

    /// Blobs this person stored, as far as this device knows them (decisions, withdrawals,
    /// unreadable ones): what the relay's per member limit counts.
    fn own_count(&self) -> usize {
        let mine = self.me.as_ref().map(|me| &me.member);
        self.decisions
            .entries
            .values()
            .filter(|e| Some(&e.by.member) == mine)
            .count()
    }

    /// This person's own withdrawals whose decision is gone, oldest first: they only keep a
    /// relay's old copy from coming back (`decisions_collect` deletes them after
    /// [`WITHDRAWAL_KEPT_MS`]), so they give way when this person needs the room.
    fn spare_withdrawals(&self) -> Vec<BlobId> {
        let mine = self.me.as_ref().map(|me| &me.member);
        let open = self.open_entries();
        let live: BTreeSet<Key> = open
            .iter()
            .filter(|(d, _)| !d.withdrawn)
            .map(|(d, e)| (e.by.member.as_str().to_owned(), d.id.clone()))
            .collect();
        open.iter()
            .filter(|(d, e)| d.withdrawn && Some(&e.by.member) == mine)
            .filter(|(d, e)| {
                d.hides(e.by.member.as_str())
                    .is_none_or(|target| !live.contains(&target))
            })
            .map(|(_, e)| e.blob.clone())
            .collect()
    }

    /// Withdraws the decision stored as `blob`: a signed withdrawal first, then `blob.delete`.
    /// Answered through [`Engine::take_decision_outcome`] once the relay deleted it.
    pub fn withdraw_decision(&mut self, blob: &str) -> Result<Vec<Effect>, TeamError> {
        self.decisions_usable()?;
        let (decision, entry) = self.decision_at(blob).ok_or(TeamError::UnknownDecision)?;
        if !self.may_withdraw(entry) {
            return Err(TeamError::NotOwnerOrAdmin);
        }
        let target = entry.blob.clone();
        let key = (entry.by.member.as_str().to_owned(), decision.id.clone());
        let writer = self
            .me
            .as_ref()
            .map(|me| me.member.as_str().to_owned())
            .unwrap_or_default();
        let record = Decision::withdrawal(
            team_decisions::new_id(),
            self.share.person.clone(),
            (self.now)(),
            &writer,
            &key,
        );
        let (sealed, store) = self.seal_decision_doc(&record)?;
        self.decisions.outcome = None;
        self.decisions.request = Some(Request::Withdraw {
            target,
            key,
            record: Some(record),
        });
        Ok(vec![store, self.put_frame(sealed)])
    }

    fn delete_frame(&mut self, blob: BlobId) -> Effect {
        self.frame(
            Op::DecisionDelete(blob.as_str().to_owned()),
            ClientMessage::BlobDelete(BlobDelete { blob }),
        )
    }

    /// Keeps this device's own stored record as an opened entry, newest last.
    fn keep_own(&mut self, blob: BlobId, decision: Decision) {
        let Some(me) = self.me.clone() else {
            return;
        };
        let id = blob.as_str().to_owned();
        self.decisions.entries.insert(
            id.clone(),
            Entry {
                blob,
                by: me,
                created_at: (self.now)(),
                state: State::Open(decision),
            },
        );
        if !self.decisions.order.contains(&id) {
            self.decisions.order.push(id);
        }
    }

    /// The relay stored this device's decision or withdrawal as `blob`.
    pub(super) fn on_decision_stored(&mut self, blob: Option<BlobId>) -> Vec<Effect> {
        let mut effects = vec![Effect::Changed];
        match self.decisions.request.take() {
            Some(Request::Record {
                decision, replaces, ..
            }) => {
                self.decisions.outcome = Some(Ok(()));
                if let Some(blob) = blob {
                    self.keep_own(blob, decision);
                }
                if let Some((old, key)) = replaces {
                    self.decisions.hidden.insert(key);
                    effects.push(self.delete_frame(old));
                }
            }
            Some(Request::Withdraw {
                target,
                key,
                record,
            }) => {
                if let (Some(blob), Some(record)) = (blob, record) {
                    self.keep_own(blob, record);
                }
                self.decisions.hidden.insert(key.clone());
                self.decisions.request = Some(Request::Withdraw {
                    target: target.clone(),
                    key,
                    record: None,
                });
                effects.push(self.delete_frame(target));
            }
            None => {}
        }
        effects
    }

    /// The relay deleted `blob` (this device's `blob.delete`).
    pub(super) fn on_decision_deleted(&mut self, blob: &str) -> Vec<Effect> {
        if matches!(&self.decisions.request, Some(Request::Withdraw { target, .. }) if target.as_str() == blob)
        {
            self.decisions.request = None;
            self.decisions.outcome = Some(Ok(()));
        }
        self.decisions.entries.remove(blob);
        self.decisions.order.retain(|id| id != blob);
        vec![Effect::Changed]
    }

    /// Deletes old withdrawals whose decision the relay no longer lists, on the devices that may
    /// (their writer, an owner or an admin): they would fill the team's room for good.
    fn decisions_collect(&mut self) -> Vec<Effect> {
        let now = (self.now)();
        let open = self.open_entries();
        let live: BTreeSet<Key> = open
            .iter()
            .filter(|(d, _)| !d.withdrawn)
            .map(|(d, e)| (e.by.member.as_str().to_owned(), d.id.clone()))
            .collect();
        let old: Vec<BlobId> = open
            .iter()
            .filter(|(d, _)| d.withdrawn)
            .filter(|(d, e)| {
                d.hides(e.by.member.as_str())
                    .is_none_or(|target| !live.contains(&target))
            })
            .filter(|(_, e)| now >= e.created_at && now - e.created_at >= WITHDRAWAL_KEPT_MS)
            .filter(|(_, e)| self.may_withdraw(e))
            .map(|(_, e)| e.blob.clone())
            .collect();
        old.into_iter()
            .map(|blob| self.delete_frame(blob))
            .collect()
    }

    pub(super) fn on_decision_refused(&mut self, op: &Op, err: &ErrorBody) -> Vec<Effect> {
        let code = err.code;
        match op {
            Op::DecisionList => {
                self.decisions.listing = None;
                match code {
                    // The newest blob known is gone: from the first again.
                    ErrorCode::NotFound => self.decisions_list(true),
                    // Too soon after the last listing: with the next ping.
                    _ => {
                        if code != ErrorCode::RateLimited {
                            eprintln!("hangar: team decisions not listed ({code:?})");
                        }
                        self.decisions.wanted = Some(!self.decisions.listed);
                        Vec::new()
                    }
                }
            }
            Op::DecisionGet(blob) => {
                match code {
                    ErrorCode::NotFound => {
                        self.decisions.entries.remove(blob.as_str());
                        self.decisions.order.retain(|id| id != blob);
                    }
                    // Fetched again with the next step.
                    _ => {
                        if let Some(entry) = self.decisions.entries.get_mut(blob.as_str()) {
                            entry.state = State::Listed;
                        }
                    }
                }
                vec![Effect::Changed]
            }
            Op::DecisionPut => {
                eprintln!("hangar: team decision not stored ({code:?})");
                match self.decisions.request.take() {
                    // A withdrawal without room for its record still deletes the decision; it
                    // stays hidden here, the full listing tells the others.
                    Some(Request::Withdraw { target, key, .. }) if code == ErrorCode::Limit => {
                        self.decisions.hidden.insert(key.clone());
                        self.decisions.request = Some(Request::Withdraw {
                            target: target.clone(),
                            key,
                            record: None,
                        });
                        vec![Effect::Changed, self.delete_frame(target)]
                    }
                    // The relay counts more of this person's blobs than this device knew: its
                    // spare withdrawals make room, and the record goes once more.
                    Some(Request::Record {
                        decision,
                        replaces,
                        retry: Some(sealed),
                    }) if code == ErrorCode::Limit && !self.spare_withdrawals().is_empty() => {
                        let needed = (self.own_count() + 1)
                            .saturating_sub(MAX_DECISIONS_PER_MEMBER)
                            .max(1);
                        let room: Vec<BlobId> =
                            self.spare_withdrawals().into_iter().take(needed).collect();
                        self.decisions.request = Some(Request::Record {
                            decision,
                            replaces,
                            retry: None,
                        });
                        let mut effects = vec![Effect::Changed];
                        for blob in room {
                            effects.push(self.delete_frame(blob));
                        }
                        effects.push(self.put_frame(sealed));
                        effects
                    }
                    Some(request) => {
                        let record = matches!(request, Request::Record { .. });
                        // At this person's own limit the honest answer names it: withdrawing
                        // someone else's would not help.
                        let own = self.own_count() >= MAX_DECISIONS_PER_MEMBER;
                        self.decisions.outcome = Some(Err(match code {
                            ErrorCode::Limit if record && own => TeamError::DecisionLimitOwn,
                            ErrorCode::Limit if record => TeamError::DecisionLimit,
                            ErrorCode::BadRequest => TeamError::DecisionsUnsupported,
                            // Anything a device of the log may write is refused only when this
                            // connection does not count (yet): a connection problem, not a role.
                            _ => TeamError::Busy,
                        }));
                        vec![Effect::Changed]
                    }
                    None => Vec::new(),
                }
            }
            Op::DecisionDelete(blob) => {
                let withdrawing = matches!(
                    &self.decisions.request,
                    Some(Request::Withdraw { target, .. }) if target.as_str() == blob
                );
                if code == ErrorCode::NotFound {
                    // Gone already: what the person wanted.
                    return self.on_decision_deleted(blob);
                }
                eprintln!("hangar: team decision not withdrawn ({code:?})");
                if withdrawing {
                    self.decisions.request = None;
                    self.decisions.outcome = Some(Err(match code {
                        ErrorCode::Forbidden => TeamError::NotOwnerOrAdmin,
                        _ => TeamError::Busy,
                    }));
                }
                vec![Effect::Changed]
            }
            _ => Vec::new(),
        }
    }

    /// [`Engine::record_decision`] with a chosen id, for tests of ids two members share.
    #[cfg(test)]
    pub(super) fn record_decision_with_id(
        &mut self,
        id: &str,
        decision: &str,
    ) -> Result<Vec<Effect>, TeamError> {
        self.record_decision_as(id.to_owned(), decision, "", None, None)
    }
}

fn new_entry(listed: &BlobEntry) -> Entry {
    Entry {
        blob: listed.blob.clone(),
        by: listed.by.clone(),
        created_at: listed.created_at.0,
        state: State::Listed,
    }
}
