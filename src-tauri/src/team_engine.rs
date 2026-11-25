//! The team protocol of one project on one connection, without I/O (team service step F part
//! 2a). The worker (`team_service.rs`) feeds it the relay's frames and its timers and carries
//! out the [`Effect`]s it returns, in order: a [`Effect::StoreKeys`] before a following
//! [`Effect::Send`] is the app's duty "store a rotation's secret before `key.rotate`"
//! (team-crypto, obligations of the app), and a failed store drops the rest.
//!
//! On every accepted `hello` the engine registers the device's keys (`device.keys`), on protocol
//! version 3 answers the `welcome`'s challenge with its proof of possession (`device.prove`,
//! `hangar_team_crypto::possession`; a refused proof ends the connection once the log fetch
//! already sent is read, so a removed device still learns of its removal), fetches the
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
//! detail. Records of an epoch older than the current one are refused too. A rising mark is
//! written at once when the last write is [`MARKS_STORE_EVERY_MS`] ago, otherwise with the next
//! state file write for any reason, the next ping, a new connection or the end of one; all
//! together, so the file always holds a consistent earlier snapshot. Marks compare by the key
//! period of the signing key first, then the counter, so a device id admitted again with a clock
//! behind is not hidden and its old keys' records stay old.
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
//! verified log changes, and the log is fetched again. The team stops
//! ([`AttentionReason::HistoryMismatch`]) only when this device's own log fetch contradicts what
//! it verified before, so members cannot stop everyone else's sharing with signed records.
//!
//! The device identity is bound to the account it was first welcomed for: a `welcome` naming
//! another member stops with [`AttentionReason::OtherAccount`] and touches no key.

mod notes;

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};

use hangar_team_crypto::identity::{safety_number, DeviceIdentity, DevicePublicKeys};
use hangar_team_crypto::memberlog::{
    DeviceEntry, Endorsement, EntryBody, EntryKind, LogError, LogVerifier, TeamState,
    MAX_LOG_ENTRIES,
};
use hangar_team_crypto::record::{
    sealed_header, task_record_id as record_task_id, EpochCipher, OpenError, Opened, RecordContext,
    RecordKind, ReplayGuard, SendCounter,
};
use hangar_team_crypto::rotation::{plan_rotation, wrap_for_device};
use hangar_team_crypto::{Entropy, EntropyError};
use hangar_team_protocol::e2e::{ChainHead, PublicKey};
use hangar_team_protocol::hash::{KeyedHash, RepoId, TaskId};
use hangar_team_protocol::ids::{
    Ciphertext, DeviceId, Epoch, MemberId, OpId, Sealed, SessionKey, TeamId, Ticket, UnixMillis,
};
use hangar_team_protocol::limits::{
    MAX_HEARTBEAT_TASKS, MAX_PREV_HASHES, MAX_RESERVATIONS_PER_MEMBER,
};
use hangar_team_protocol::log::{ClientCursor, Observed};
use hangar_team_protocol::wire::{
    check_welcome, decode_server, ClaimClear, ClaimOverlap, ClaimSet, ClientFrame, ClientMessage,
    DeviceInfo, DeviceRemove, ErrorBody, ErrorCode, Heartbeat, Hello, Holder, KeyGet, MatchLevel,
    MemberLogEntries, MemberLogGet, PresenceSet, PresenceState, PresenceUpdate, ProtoRange,
    ReleaseReason, RequestEndReason, ReservationExpiring, Role, ServerFrame, ServerMessage,
    SessionRef, TaskRef, TaskReleaseAnswer, TaskReleaseAnswered, TaskReleaseRequest,
    TaskReleaseRequestEnded, TaskReleaseRequested, TaskReleased, TaskReserve, TaskReserved,
    TicketRenew,
};
use sha2::{Digest as _, Sha256};

use crate::claims::{LineRange, OverlapLevel};
use crate::session::SessionId;
use crate::team::{
    hex, safety_groups, AttentionReason, DetailMark, HolderMark, KeyChangeWait, LocalState,
    OwnHold, PendingConfirmation, PendingKind, ReplayMark, SessionEnd, StopReason, StoredDevice,
    StoredKeys, TeamError, TeamStatus,
};
use crate::team_share::{
    self, DivergentDevice, LocalShare, LogSafety, OtherDetail, OwnDevice, PeerStatus, PeerView,
    SessionRepo, SessionState, TeamActivity, TeammateFile, TeammateOverlap,
};
use crate::team_tasks::{
    self, AttemptState, AttemptView, NoticeKind, RequestView, TaskNote, TaskNotice, TaskView,
    TeamTask, MAX_NOTICES, MAX_TITLE_CHARS,
};

pub use notes::NotesKeysView;

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
    /// Read the person's projects on the website again (authenticated HTTPS) and hand role and
    /// owner, with `members` also the project's member list, to [`Engine::set_website`]; end the
    /// connection when the project is gone there (with `removal`, the relay said this member was
    /// removed: the keys go too). The answer to relay events only the website can confirm
    /// (`member.removed`, `member.role_changed`), at most once per [`WEBSITE_CHECK_EVERY_MS`],
    /// and the regular member check of every admitted device.
    CheckWebsite { removal: bool, members: bool },
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
    /// `device.prove` (protocol version 3).
    Prove,
    LogGet,
    DevicesGet,
    KeyGet(Epoch),
    /// `memberlog.append` creating the team, confirming a device (wrapped to afterwards), or
    /// removing a member.
    Append(Option<Holder>),
    Rotate(Epoch),
    Wrap,
    Remove,
    /// `task.reserve` of the attempt with this normalized task id.
    Reserve(String),
    /// `task.release_request` for this task.
    AskRelease(KeyedHash),
    /// `task.release_answer` for this task.
    Answer(KeyedHash),
    /// `notes.keys.get`.
    NotesGet,
    /// `notes.keys.set`.
    NotesSet,
    Quiet,
}

/// A log fetch for task events of holders the log does not list yet goes out at most this often;
/// the next ping fetches for those that wait.
const UNKNOWN_FETCH_EVERY_MS: u64 = 5_000;

/// How far ahead of this device's clock the unverified counter of a note that never opened may
/// set its mark: send counters follow their device's clock ([`crate::team::counter_floor`]), so a
/// relay that forges a huge counter cannot make that holder's notes for the task look old for
/// longer than this, while a sender whose clock runs a little ahead is still marked in full.
const HEADER_COUNTER_SLACK_MS: u64 = 10 * 60 * 1_000;

/// Task events held back until the verified log can vouch for their holders, in arrival order;
/// beyond this the device reconnects and takes the relay's snapshot instead.
const MAX_TASK_QUEUE: usize = 1024;

/// Heartbeats for this device's reservations go out at most this often while its agents change
/// files.
const HEARTBEAT_EVERY_MS: u64 = 60_000;

/// Website checks the relay's events ask for go out at most this often; one that comes sooner
/// waits for the next ping after this span (the relay's events are unsigned and may come in
/// floods).
pub const WEBSITE_CHECK_EVERY_MS: u64 = 30_000;

/// Every admitted device compares the log's members with the website's this often, also without
/// any relay event (the relay may withhold `member.removed` and the log's removal).
pub const MEMBERS_CHECK_EVERY_MS: u64 = 10 * 60 * 1_000;

/// A due rotation that is not urgent (it only removes devices added after the current epoch
/// began, `TeamState::rotation_urgent`) goes out from this device at most this often, so one
/// member adding and removing devices of its own cannot keep the team changing keys. Removing a
/// member, or a device from before the epoch, rotates at once.
pub const DEFERRED_ROTATION_EVERY_MS: u64 = 10 * 60 * 1_000;

/// How long the log may still name a member the website no longer lists before a device that
/// cannot remove it stops sealing (measured on this device's clock from the first list that left
/// the member out): an owner's or admin's device follows the list within
/// [`MEMBERS_CHECK_EVERY_MS`], so a removal still missing after that was most likely held back.
pub const DEPARTURE_GRACE_MS: u64 = 10 * 60 * 1_000;

/// Refused `key.rotate` per connection; after that this device tries again only on the next
/// connection. A refusal other than another rotation coming first (`conflict`, `staleEpoch`)
/// also waits for the next ping, so a relay refusing every rotation cannot make it loop.
const MAX_ROTATE_FAILURES: u32 = 2;

/// A member as the website lists it for the project: its role there, `None` for a role this
/// Hangar does not know (it still counts as a member, but its log role is left as it is).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebsiteMember {
    pub member: MemberId,
    pub role: Option<Role>,
}

/// What the website said about the project (authenticated HTTPS).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Website {
    /// The person's role (the ticket's role); caps the log role.
    pub role: Role,
    /// The project's owner, when the website names one.
    pub owner: Option<MemberId>,
    /// The project's members, when they were asked for and the website answered.
    pub members: Option<Vec<WebsiteMember>>,
}

/// The website's member list, with the log head index of the moment it was asked for: members
/// the log added after that are newer than the list and never removed by it.
#[derive(Debug, Clone)]
struct Roster {
    members: BTreeMap<MemberId, Option<Role>>,
    at: u64,
}

/// A reservation in the team, as the relay reported it.
#[derive(Debug, Clone)]
struct Hold {
    holder: Holder,
    epoch: Epoch,
    since: UnixMillis,
    expires_at: Option<UnixMillis>,
    /// The relay warned that the hold runs out.
    expiring: bool,
    /// The holder's note while it cannot be opened yet (a log entry or epoch this device does
    /// not have yet); tried again after the next log fetch.
    unopened: Option<Sealed>,
    /// The holder's signed note: the only source of the title and the area.
    note: Option<TaskNote>,
    request: Option<HoldRequest>,
}

#[derive(Debug, Clone)]
struct HoldRequest {
    requester: MemberId,
    /// The requester's listed device whose note for this task opened (and was not shown
    /// before); `None`: the relay's word alone, which shows no card.
    signer: Option<Holder>,
    /// From the requester's signed note; empty without one.
    name: String,
    /// This device sent its answer; the card is gone until the relay's event or an error.
    answered: bool,
}

/// A task this device reserved or tries to reserve.
#[derive(Debug, Clone)]
struct Attempt {
    title: String,
    session: Option<SessionId>,
    repo: Option<String>,
    area: Option<String>,
    /// The task's keyed hash under the epoch it was last sent with.
    hash: Option<KeyedHash>,
    /// Someone else holds the task (first wins).
    blocked: Option<Holder>,
    /// This person's release request is open.
    asked: bool,
    /// The note went out again after a handover (which carries none of this device's).
    renoted: bool,
    /// `blocked` names a holder the verified log listed when the relay named it; one it did not
    /// list yet waits for the next log fetch.
    verified: bool,
}

/// How opening a reservation note went.
enum NoteOpen {
    Note(TaskNote),
    Later,
    Fork,
    /// Not readable, or older than the note shown: the hold keeps what it had.
    Keep,
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

/// When a teammate's live record was sealed, in the order marks compare: the log index of the
/// entry that added the signing key (its key period), then the record counter. A device id
/// admitted again signs with keys of a later period, so its records are newer than any of the
/// old keys whatever its clock says, and a record of the old keys stays older.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Stamp {
    /// `added_at` of the key period; 0 for a mark stored before periods were kept (it then
    /// counts as the period of the record it is compared with).
    period: u64,
    counter: u64,
}

impl Stamp {
    /// `self` as a mark compared with `record`: a mark without a known period takes the
    /// record's, so marks of the earlier format compare by counter alone.
    fn against(self, record: Stamp) -> Stamp {
        if self.period == 0 {
            Stamp {
                period: record.period,
                counter: self.counter,
            }
        } else {
            self
        }
    }
}

/// High-water marks of the teammates' live state shown on this device: presence status per
/// device, claim detail per device and session key, as [`Stamp`]s. Never cleared by `offline`,
/// `hello` or a restart (they are stored with [`LocalState`]); trimmed to the devices the
/// verified log ever listed and to [`MAX_DETAIL_SESSIONS`] sessions per device.
///
/// The presence mark is per device, not per signing key: a status signed with the old keys of a
/// device id admitted again belongs to an earlier key period and stays older than anything
/// shown since.
#[derive(Debug, Default, PartialEq, Eq)]
struct LiveMarks {
    presence: BTreeMap<Holder, Stamp>,
    detail: BTreeMap<(Holder, String), Stamp>,
    /// Per device: the highest stamp of a detail mark dropped to stay within bounds; a detail of
    /// a session without a mark must be newer.
    floors: BTreeMap<Holder, Stamp>,
    /// Reservation and release request notes per device and record ([`note_key`]), bounded
    /// like the details with a floor of their own: a note shown once, or of a hold that ended,
    /// never comes back as new, also after a resync or a restart.
    notes: BTreeMap<(Holder, String), Stamp>,
    note_floors: BTreeMap<Holder, Stamp>,
}

/// Which note a mark belongs to: the reservation note of a task (its record id), or the release
/// request note of it.
fn note_key(kind: RecordKind, task: &KeyedHash) -> String {
    let id = record_task_id(task);
    match kind {
        RecordKind::ReleaseRequestNote => format!("request/{id}"),
        _ => id,
    }
}

/// A mark per device and key, at most [`MAX_DETAIL_SESSIONS`] per device: beyond that the lowest
/// key `protected` does not name goes and raises the device's floor, which a key without a mark
/// must exceed. Protected keys (the notes of holds that still run) never go, so a restart or a
/// resync cannot make a running hold's note look old; they are bounded by the holds themselves.
fn bounded_mark(
    marks: &mut BTreeMap<(Holder, String), Stamp>,
    floors: &mut BTreeMap<Holder, Stamp>,
    holder: &Holder,
    key: &str,
    record: Stamp,
    protected: &dyn Fn(&str) -> bool,
) -> Mark {
    let entry = (holder.clone(), key.to_owned());
    if let Some(mark) = marks.get_mut(&entry) {
        return LiveMarks::raise(mark, record);
    }
    let floor = floors.get(holder).copied().unwrap_or_default();
    if record <= floor.against(record) {
        return Mark::Stale;
    }
    marks.insert(entry, record);
    // The device's marks are adjacent in the map: count them and find the lowest without
    // walking every other device's (at most MAX_DETAIL_SESSIONS + 1 entries).
    let mut count = 0;
    let mut lowest: Option<(&String, Stamp)> = None;
    for ((_, held), at) in marks
        .range((holder.clone(), String::new())..)
        .take_while(|((h, _), _)| h == holder)
    {
        count += 1;
        if protected(held) {
            continue;
        }
        let at = at.against(record);
        if lowest.is_none_or(|(_, low)| at < low) {
            lowest = Some((held, at));
        }
    }
    if count > MAX_DETAIL_SESSIONS {
        if let Some((dropped, at)) = lowest.map(|(s, at)| (s.clone(), at)) {
            marks.remove(&(holder.clone(), dropped.clone()));
            let floor = floors.entry(holder.clone()).or_default();
            *floor = (*floor).against(at).max(at);
            if dropped == key {
                return Mark::Stale;
            }
        }
    }
    Mark::Raised
}

impl LiveMarks {
    fn from_local(local: &LocalState) -> Self {
        // A file of the first format may hold several marks per device (one per signing key):
        // the highest one counts.
        let mut presence: BTreeMap<Holder, Stamp> = BTreeMap::new();
        for mark in &local.presence_marks {
            let stamp = presence.entry(mark.holder.clone()).or_default();
            *stamp = (*stamp).max(Stamp {
                period: mark.period,
                counter: mark.counter,
            });
        }
        Self {
            presence,
            detail: local
                .detail_marks
                .iter()
                .map(|m| {
                    let stamp = Stamp {
                        period: m.period,
                        counter: m.counter,
                    };
                    ((m.holder.clone(), m.session.clone()), stamp)
                })
                .collect(),
            floors: local
                .detail_floors
                .iter()
                .map(|m| {
                    let stamp = Stamp {
                        period: m.period,
                        counter: m.counter,
                    };
                    (m.holder.clone(), stamp)
                })
                .collect(),
            notes: local
                .note_marks
                .iter()
                .map(|m| {
                    let stamp = Stamp {
                        period: m.period,
                        counter: m.counter,
                    };
                    ((m.holder.clone(), m.session.clone()), stamp)
                })
                .collect(),
            note_floors: local
                .note_floors
                .iter()
                .map(|m| {
                    let stamp = Stamp {
                        period: m.period,
                        counter: m.counter,
                    };
                    (m.holder.clone(), stamp)
                })
                .collect(),
        }
    }

    fn store(&self, local: &mut LocalState) {
        let holder_mark = |(holder, stamp): (&Holder, &Stamp)| HolderMark {
            holder: holder.clone(),
            period: stamp.period,
            counter: stamp.counter,
        };
        local.presence_marks = self.presence.iter().map(holder_mark).collect();
        local.detail_marks = self
            .detail
            .iter()
            .map(|((holder, session), stamp)| DetailMark {
                holder: holder.clone(),
                session: session.clone(),
                period: stamp.period,
                counter: stamp.counter,
            })
            .collect();
        local.detail_floors = self.floors.iter().map(holder_mark).collect();
        local.note_marks = self
            .notes
            .iter()
            .map(|((holder, key), stamp)| DetailMark {
                holder: holder.clone(),
                session: key.clone(),
                period: stamp.period,
                counter: stamp.counter,
            })
            .collect();
        local.note_floors = self.note_floors.iter().map(holder_mark).collect();
    }

    /// `protected`: the keys of this holder's notes that stand for a hold or request still
    /// running ([`Engine::running_notes`]); their marks are never dropped to stay within bounds.
    fn note(
        &mut self,
        holder: &Holder,
        key: &str,
        record: Stamp,
        protected: &BTreeSet<String>,
    ) -> Mark {
        bounded_mark(
            &mut self.notes,
            &mut self.note_floors,
            holder,
            key,
            record,
            &|k| protected.contains(k),
        )
    }

    /// Raises the mark of a note to `record` at least (a note that never opened, whose hold
    /// ended: the same record shown again is old). `true` when it rose. The record comes from
    /// an unverified header, so it never takes the place of another mark and never raises the
    /// floor: with the device's marks full, or at or below its floor, nothing changes.
    fn note_at_least(&mut self, holder: &Holder, key: &str, record: Stamp) -> bool {
        let entry = (holder.clone(), key.to_owned());
        if let Some(mark) = self.notes.get_mut(&entry) {
            return Self::raise(mark, record) == Mark::Raised;
        }
        let count = self
            .notes
            .range((holder.clone(), String::new())..)
            .take_while(|((h, _), _)| h == holder)
            .count();
        let floor = self.note_floors.get(holder).copied().unwrap_or_default();
        if count >= MAX_DETAIL_SESSIONS || record <= floor.against(record) {
            return false;
        }
        self.notes.insert(entry, record);
        true
    }

    /// The hold this note stood for ended: the same note shown again is old from now on (a new
    /// reservation always seals a new note, with a higher counter). `true` when a mark rose.
    fn consume_note(&mut self, holder: &Holder, key: &str) -> bool {
        match self.notes.get_mut(&(holder.clone(), key.to_owned())) {
            Some(mark) => {
                mark.counter = mark.counter.saturating_add(1);
                true
            }
            None => false,
        }
    }

    fn raise(mark: &mut Stamp, record: Stamp) -> Mark {
        match record.cmp(&mark.against(record)) {
            std::cmp::Ordering::Less => Mark::Stale,
            std::cmp::Ordering::Equal => Mark::Seen,
            std::cmp::Ordering::Greater => {
                *mark = record;
                Mark::Raised
            }
        }
    }

    fn presence(&mut self, holder: &Holder, record: Stamp) -> Mark {
        let mark = self.presence.entry(holder.clone()).or_default();
        Self::raise(mark, record)
    }

    fn detail(&mut self, holder: &Holder, session: &str, record: Stamp) -> Mark {
        bounded_mark(
            &mut self.detail,
            &mut self.floors,
            holder,
            session,
            record,
            &|_| false,
        )
    }

    /// Drops the marks of devices `known` does not vouch for; `true` when any went.
    fn retain(&mut self, known: impl Fn(&Holder) -> bool) -> bool {
        let count = |m: &Self| {
            (
                m.presence.len(),
                m.detail.len(),
                m.floors.len(),
                m.notes.len(),
                m.note_floors.len(),
            )
        };
        let before = count(self);
        self.presence.retain(|holder, _| known(holder));
        self.detail.retain(|(holder, _), _| known(holder));
        self.floors.retain(|holder, _| known(holder));
        self.notes.retain(|(holder, _), _| known(holder));
        self.note_floors.retain(|holder, _| known(holder));
        before != count(self)
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
    /// The sender names a newer log or epoch: fetch the log and try once more. With the log
    /// index a listed device signed as its head (`OpenError::AheadOfLog` with `signed`).
    Later(Option<u64>),
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
    /// The account signed in on the website (`/me`), when known: the first `welcome` must name
    /// it, so the relay cannot bind this device to another member id.
    pub account: Option<MemberId>,
    /// The person's role in the project on the website (the ticket's role); caps the log role.
    pub website_role: Role,
    /// The project's owner on the website; the verified log's owner must be the same.
    pub owner: Option<MemberId>,
    pub keys: StoredKeys,
    pub local: LocalState,
    /// Unix milliseconds (log entries' `issued_at`, the send counter's floor).
    pub now: Box<dyn Fn() -> u64 + Send + Sync>,
    pub entropy: EntropySource,
    /// Website checks the relay's events ask for go out at most this often (milliseconds;
    /// [`WEBSITE_CHECK_EVERY_MS`] in the app).
    pub website_every_ms: u64,
    /// Every admitted device compares the members this often (milliseconds;
    /// [`MEMBERS_CHECK_EVERY_MS`] in the app).
    pub members_every_ms: u64,
}

pub struct Engine {
    project: String,
    team: TeamId,
    device: DeviceId,
    identity: DeviceIdentity,
    member: Option<MemberId>,
    account: Option<MemberId>,
    website_role: Role,
    owner: Option<MemberId>,
    /// The website's member list as last read; `None` before the first read. Without it nobody
    /// new is offered for admission.
    roster: Option<Roster>,
    /// The relay refused a member entry from [`Engine::roster`]: no further one until the next
    /// read, so a refusing relay cannot make this device send it in a loop. The list itself
    /// stays, so admissions keep following the website meanwhile.
    roster_spent: bool,
    /// When the last [`Effect::CheckWebsite`] a relay event (or a device the list does not name)
    /// asked for went out (clock milliseconds).
    website_checked_at: Option<u64>,
    /// A website check waits for [`WEBSITE_CHECK_EVERY_MS`] to pass; with `website_removal`
    /// the relay said this member was removed.
    website_wanted: bool,
    website_removal: bool,
    /// When the member list was last asked for (clock milliseconds), for the regular check
    /// every `members_every_ms`. Kept apart from the events' rate, so the check on connecting
    /// never delays the one a removal event asks for.
    members_checked_at: Option<u64>,
    /// The member list is wanted with the next check: a new connection, an admission just
    /// confirmed, or the regular interval.
    members_wanted: bool,
    /// The log head index when the member list now being read was asked for.
    roster_asked_at: Option<u64>,
    website_every_ms: u64,
    members_every_ms: u64,
    /// A remove member or change role entry this device sent waits for its answer.
    appending: bool,
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
    /// The relay refused this connection's `device.prove` while a log fetch was running; the
    /// connection ends once the fetch is read.
    prove_refused: bool,
    creating: bool,
    rotating: bool,
    /// Refused `key.rotate` on this connection ([`MAX_ROTATE_FAILURES`]).
    rotate_failures: u32,
    /// A refused `key.rotate` waits for the next ping before the next try.
    rotate_hold: bool,
    /// The epoch of the `key.rotate` this device sent on this connection, until the log shows
    /// it; a relay that acknowledges a rotation it never stores counts as refusing it.
    rotation_sent: Option<Epoch>,
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
    // Reservations (part 2b, tasks); kept across connections of this engine: a reconnect
    // replays the events after the cursor.
    holds: BTreeMap<KeyedHash, Hold>,
    /// By normalized task id.
    attempts: BTreeMap<String, Attempt>,
    task_queue: Vec<ServerMessage>,
    /// A note mark (in [`LiveMarks`]) rose while handling a task event; stored after it.
    note_marks_rose: bool,
    /// The holds before a `resync_required`, checked against the snapshot after it.
    before_resync: BTreeMap<KeyedHash, Hold>,
    /// A `resync_required` came: this device's holds as last settled
    /// (`LocalState::own_holds`, also from before a restart) wait for the snapshot, which tells
    /// which of them ended while the device was away.
    own_check: bool,
    notices: VecDeque<TaskNotice>,
    notice_seq: u64,
    /// This engine took the relay's state of the reservations once (a resync on its first
    /// connection); until then `hello` sends no cursor, since the reservations of before a
    /// restart are not stored.
    book_known: bool,
    /// When the last heartbeat went out (clock milliseconds).
    heartbeat_at: Option<u64>,
    /// When the last log fetch for an unknown holder went out (clock milliseconds).
    unknown_fetch_at: Option<u64>,
    /// When this device last sent `key.rotate` (clock milliseconds), for
    /// [`DEFERRED_ROTATION_EVERY_MS`]; kept across connections.
    rotated_at: Option<u64>,
    /// The owner's confirmations of admins the website named (`PendingKind::RoleChange`).
    promotions: Vec<PendingConfirmation>,
    /// The most log entries the verifier takes: `MAX_LOG_ENTRIES`, smaller only in tests.
    max_log_entries: u64,
    /// Members the verified log names but the website's list left out, with the moment (clock
    /// milliseconds) this device first saw that; kept across connections ([`DEPARTURE_GRACE_MS`]).
    departed: BTreeMap<MemberId, u64>,
    /// When this device first saw a rotation due that is not urgent (clock milliseconds); kept
    /// across connections until the log no longer says one is due.
    deferred_since: Option<u64>,
    /// The highest log index a listed device signed as its head while this device's log ended
    /// before it, with the number of the log fetch that must reach it.
    ahead: Option<(u64, u64)>,
    /// Log fetches started, to tell the fetch after [`Engine::ahead`] was noted.
    log_fetches: u64,
    /// A fetch after a signed newer head did not reach it: the relay holds back log entries.
    history_withheld: bool,
    /// The team's notes key ring (ADR 0006, phase 2).
    notes: notes::NotesKeys,
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
            max_log_entries: MAX_LOG_ENTRIES,
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
            account: setup.account,
            website_role: setup.website_role,
            owner: setup.owner,
            roster: None,
            roster_spent: false,
            website_checked_at: None,
            website_wanted: false,
            members_checked_at: None,
            members_wanted: false,
            website_removal: false,
            roster_asked_at: None,
            website_every_ms: setup.website_every_ms,
            members_every_ms: setup.members_every_ms,
            appending: false,
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
            prove_refused: false,
            creating: false,
            rotating: false,
            rotate_failures: 0,
            rotate_hold: false,
            rotation_sent: None,
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
            holds: BTreeMap::new(),
            attempts: BTreeMap::new(),
            task_queue: Vec::new(),
            note_marks_rose: false,
            before_resync: BTreeMap::new(),
            own_check: false,
            notices: VecDeque::new(),
            notice_seq: 0,
            book_known: false,
            heartbeat_at: None,
            unknown_fetch_at: None,
            rotated_at: None,
            promotions: Vec::new(),
            departed: BTreeMap::new(),
            deferred_since: None,
            ahead: None,
            log_fetches: 0,
            history_withheld: false,
            notes: notes::NotesKeys::default(),
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

    /// What the website says, read before every connection and after
    /// [`Effect::CheckWebsite`]; the relay's events never set it. A member list arrives only for
    /// a check that asked for one; it replaces the last one.
    pub fn set_website(&mut self, website: Website) -> Vec<Effect> {
        self.website_role = website.role;
        self.owner = website.owner;
        let asked = self.roster_asked_at.take();
        if let (Some(members), Some(at)) = (website.members, asked) {
            self.roster = Some(Roster {
                members: members.into_iter().map(|m| (m.member, m.role)).collect(),
                at,
            });
            self.roster_spent = false;
            // A device offered under the last list goes when this one no longer names its
            // member, not only with the next device poll.
            let pending = std::mem::take(&mut self.pending);
            self.pending = pending
                .into_iter()
                .filter(|p| self.confirmation_body(&p.info).is_some())
                .collect();
        }
        self.reconcile()
    }

    /// The account signed in on the website, read before every connection.
    pub fn set_account(&mut self, account: Option<MemberId>) {
        self.account = account;
    }

    /// Whether this device acts for the team's owner or an admin (log role capped by the
    /// website's), with an admission the person confirmed.
    fn manages(&self) -> bool {
        let (Some(state), Some(me)) = (self.verifier.state(), self.me.as_ref()) else {
            return false;
        };
        self.keys.admitted
            && state.device(me).is_some()
            && state
                .effective_role(&me.member, self.website_role)
                .is_some_and(|r| role_rank(r) <= role_rank(Role::Admin))
    }

    /// Whether this device reads the website's member list: any device in the log whose
    /// admission the person confirmed. An owner's or admin's device follows it with log entries;
    /// any other device notices when the log keeps a member the website removed
    /// ([`DEPARTURE_GRACE_MS`]), so a relay that withholds the removal cannot keep the old key
    /// in use for good.
    fn reads_members(&self) -> bool {
        self.keys.admitted && self.in_log()
    }

    /// Asks for a website check; `removal`: the relay said this member was removed. At most one
    /// per [`WEBSITE_CHECK_EVERY_MS`]: a later wish waits for the next ping after that.
    fn want_website(&mut self, removal: bool) -> Vec<Effect> {
        self.website_wanted = true;
        self.website_removal |= removal;
        self.website_due()
    }

    /// Asks for the member list with the next check (it goes out at once): on a new connection,
    /// after the admission was confirmed and every `members_every_ms`. Those are bounded by the
    /// reconnect backoff and the interval, not by the events' rate.
    fn want_members(&mut self) -> Vec<Effect> {
        if !self.reads_members() {
            return Vec::new();
        }
        self.members_wanted = true;
        self.website_due()
    }

    fn website_due(&mut self) -> Vec<Effect> {
        let now = (self.now)();
        let recent = self
            .website_checked_at
            .is_some_and(|at| now >= at && now - at < self.website_every_ms);
        let event = self.website_wanted && !recent;
        if !event && !self.members_wanted {
            return Vec::new();
        }
        let removal = if event {
            self.website_checked_at = Some(now);
            self.website_wanted = false;
            std::mem::take(&mut self.website_removal)
        } else {
            false
        };
        self.members_wanted = false;
        let members = self.reads_members();
        if members {
            self.members_checked_at = Some(now);
            self.roster_asked_at = self.verifier.state().map(|s| s.head().index);
        }
        vec![Effect::CheckWebsite { removal, members }]
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
        self.pending
            .iter()
            .map(|p| p.view.clone())
            .chain(self.promotions.iter().cloned())
            .collect()
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

    /// A new connection: forget the last one's ops and send `hello`, with the cursor once this
    /// engine took the relay's snapshot (the reservations live in memory only), else without.
    pub fn hello(&mut self, ticket: Ticket) -> Vec<Effect> {
        self.me = None;
        self.ops.clear();
        self.resyncing = false;
        self.syncing = false;
        self.sync_again = false;
        self.synced = false;
        self.prove_refused = false;
        self.creating = false;
        self.rotating = false;
        self.rotate_failures = 0;
        self.rotate_hold = false;
        self.rotation_sent = None;
        self.appending = false;
        self.key_get = None;
        self.devices_get = false;
        self.inline.clear();
        self.notes.new_connection();
        self.verifier = LogVerifier::with_max_entries(self.team.clone(), self.max_log_entries);
        self.status = TeamStatus::Connecting { retrying: false };
        // The relay keeps no claims or presence of a closed connection: everything goes again,
        // and the others' presence arrives anew.
        self.sent_claims.clear();
        self.sent_presence = None;
        self.shared_epoch = None;
        self.peers.clear();
        self.overlaps.clear();
        self.stashed.clear();
        // Without the reservations of this engine's own connections, a replay would miss every
        // hold from before the start: the first connection takes the relay's snapshot.
        let last_seq = if self.book_known {
            self.cursor.cursor().cloned()
        } else {
            None
        };
        let hello = ClientMessage::Hello(Hello {
            ticket,
            device: self.device.clone(),
            proto: ProtoRange::CURRENT,
            last_seq,
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

    /// [`Engine::local`] with every live mark in it, for an [`Effect::StoreLocal`]: marks that
    /// rose since the last write go along with any write that happens anyway.
    pub fn local_to_store(&mut self) -> &LocalState {
        if self.marks_dirty {
            self.store_marks();
        }
        &self.local
    }

    fn store_marks(&mut self) {
        self.live.store(&mut self.local);
        self.marks_dirty = false;
        self.marks_stored_at = Some((self.now)());
    }

    /// A mark rose: stored at once when the last store is [`MARKS_STORE_EVERY_MS`] ago (or the
    /// clock went back), otherwise left for the next state file write ([`Engine::local_to_store`]),
    /// the next ping or the end of the connection.
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
        // Task events that wait for a holder the log does not list yet.
        let unverified = self
            .attempts
            .values()
            .any(|a| a.blocked.is_some() && !a.verified);
        if (!self.task_queue.is_empty() || unverified) && self.synced && !self.syncing {
            self.unknown_fetch_at = Some((self.now)());
            effects.extend(self.sync_log());
        }
        // The member check, also when the relay says nothing.
        let now = (self.now)();
        let stale = self
            .members_checked_at
            .is_none_or(|at| now < at || now - at >= self.members_every_ms);
        if self.synced && stale && self.reads_members() {
            self.members_wanted = true;
        }
        effects.extend(self.website_due());
        // A departure or a deferred rotation that waits may have run past its time.
        let sharing = matches!(self.status, TeamStatus::Connected { .. });
        let overdue = (!self.manages() && self.overdue_at(now)) || self.departure_stuck(now);
        if sharing && self.synced && !self.syncing && overdue {
            effects.extend(self.reconcile());
        }
        // A due rotation that waited (a refusal, or the interval of a deferred one) is tried
        // again from here.
        self.rotate_hold = false;
        let due = self.verifier.state().is_some_and(TeamState::rotation_due);
        if due && self.synced && !self.syncing && !self.rotating && self.manages() {
            effects.extend(self.reconcile());
        }
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
                    // The first welcome binds the device for good, so only once the website
                    // said who is signed in (`/me`): the connection ends without binding and the
                    // worker connects again after reading it.
                    None if self.account.is_none() => {
                        eprintln!("hangar: the signed-in account is not known yet; not binding");
                        return vec![Effect::End(SessionEnd::Lost)];
                    }
                    // It must name the account signed in on the website, not one the relay chose.
                    None if self
                        .account
                        .as_ref()
                        .is_some_and(|account| *account != welcome.member) =>
                    {
                        eprintln!("hangar: the relay named another account than the signed-in one");
                        return vec![Self::stop(AttentionReason::OtherAccount)];
                    }
                    None => {
                        self.member = Some(welcome.member.clone());
                        effects.push(Effect::StoreDevice);
                    }
                }
                self.notes.welcome(welcome.proto);
                self.cursor
                    .welcome(&welcome.stream, welcome.head, self.resyncing);
                self.resyncing = false;
                self.book_known = true;
                self.local.cursor = self.cursor.cursor().cloned();
                let me = Holder {
                    member: welcome.member,
                    device: self.device.clone(),
                };
                let keys = self.identity.device_keys(&self.team, &me);
                // Protocol version 3: the relay counts this connection as the device the log
                // lists only after it proved possession of the device's signing key, so the proof
                // goes before the log fetch and everything that follows it.
                let prove = welcome.challenge.map(|challenge| {
                    self.identity
                        .prove_connection(&self.team, &me, welcome.proto, &challenge)
                });
                self.me = Some(me);
                effects.push(Effect::StoreLocal);
                effects.push(self.frame(Op::DeviceKeys, ClientMessage::DeviceKeys(keys)));
                if let Some(prove) = prove {
                    effects.push(self.frame(Op::Prove, ClientMessage::DeviceProve(prove)));
                }
                effects.extend(self.sync_log());
            }
            ServerMessage::ResyncRequired(_) => {
                // Of the state events without `seq` that follow, only the reservations count:
                // the log is fetched anew after the welcome that ends the snapshot, presence
                // arrives anew.
                self.cursor.resync();
                self.resyncing = true;
                self.reset_tasks();
                effects.push(Effect::Changed);
            }
            ServerMessage::Err(err) => effects.extend(self.on_err(op, err)),
            ServerMessage::Ack(ack) => effects.extend(self.on_ack(op, ack.keys)),
            ServerMessage::MemberLogEntries(page) => effects.extend(self.on_entries(page)),
            ServerMessage::Devices(list) => {
                self.devices_get = false;
                let unlisted = self.update_pending(list.devices);
                effects.push(Effect::Changed);
                // Someone joined on the website after its list was read: read it again (within
                // the check's rate) so the person is offered without waiting for the next round.
                if unlisted && self.manages() {
                    effects.extend(self.want_website(false));
                }
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
                // the log fetch may never be answered. Another member's removal makes an owner's
                // or admin's device compare the log's members with the website's.
                let mine = self
                    .me
                    .as_ref()
                    .is_some_and(|me| me.member == removed.member);
                if mine || self.manages() {
                    effects.extend(self.want_website(mine));
                }
                effects.extend(self.sync_log());
            }
            ServerMessage::MemberRoleChanged(changed) => {
                let mine = self
                    .me
                    .as_ref()
                    .is_some_and(|me| me.member == changed.member);
                if mine || self.manages() {
                    effects.extend(self.want_website(false));
                }
            }
            ServerMessage::PresenceUpdate(update) => effects.extend(self.on_presence(update, true)),
            ServerMessage::ClaimOverlap(overlap) => effects.extend(self.on_overlap(overlap, true)),
            ServerMessage::NotesKeys(answer) => effects.extend(self.on_notes_keys(answer)),
            ServerMessage::NotesKeysUpdated(_) => effects.extend(self.on_notes_updated()),
            message @ (ServerMessage::TaskReserved(_)
            | ServerMessage::TaskReleased(_)
            | ServerMessage::ReservationExpiring(_)
            | ServerMessage::ReservationExpired(_)
            | ServerMessage::TaskReleaseRequested(_)
            | ServerMessage::TaskReleaseAnswered(_)
            | ServerMessage::TaskReleaseRequestEnded(_)) => effects.extend(self.on_task(message)),
            // `notify` is the relay's cue to tell a person out of band; the events it stands for
            // reach every device of the person in the log (warnings, release requests).
            _ => {}
        }
        effects
    }

    fn on_err(&mut self, op: Option<Op>, err: ErrorBody) -> Vec<Effect> {
        let code = err.code;
        match op {
            Some(Op::Hello) => match code {
                ErrorCode::Forbidden => vec![Effect::End(SessionEnd::HelloForbidden)],
                ErrorCode::OutdatedClient => vec![Self::stop(AttentionReason::NeedsUpdate)],
                _ => vec![Effect::End(SessionEnd::Lost)],
            },
            Some(Op::LogGet) => vec![Effect::End(SessionEnd::Lost)],
            // The relay refused the proof of possession: it would refuse every intent of this
            // connection after it. A new connection brings a new challenge; the reconnect
            // policy backs off, since such a connection never settles. A log fetch still running
            // is read first: a removed device is served only the log up to its removal, and must
            // learn from it that it was removed (`after_sync`).
            Some(Op::Prove) => {
                eprintln!("hangar: team relay refused the proof of possession ({code:?})");
                if self.syncing {
                    self.prove_refused = true;
                    Vec::new()
                } else {
                    vec![Effect::End(SessionEnd::Lost)]
                }
            }
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
                if std::mem::take(&mut self.appending) {
                    // A member entry from the website's list: not again before the next read.
                    // The list stays for admissions; dropping it would leave nothing to check a
                    // new member against.
                    self.roster_spent = true;
                }
                self.sync_log()
            }
            Some(Op::Rotate(epoch)) => {
                // Another rotation came first: the unused secret goes, the log decides again.
                eprintln!("hangar: team key rotation refused ({code:?})");
                self.rotating = false;
                self.rotate_failures = self.rotate_failures.saturating_add(1);
                // Another rotation coming first is normal and decided by the log at once; any
                // other refusal waits for the next ping.
                if !matches!(code, ErrorCode::Conflict | ErrorCode::StaleEpoch) {
                    self.rotate_hold = true;
                }
                self.rotation_sent = None;
                self.keys.epochs.remove(&epoch);
                let mut effects = vec![Effect::StoreKeys];
                effects.extend(self.sync_log());
                effects
            }
            Some(Op::Wrap) | Some(Op::Remove) => {
                eprintln!("hangar: team key or device change refused ({code:?})");
                self.sync_log()
            }
            Some(Op::NotesGet) => self.on_notes_refused(false, code),
            Some(Op::NotesSet) => self.on_notes_refused(true, code),
            Some(Op::Reserve(key)) => self.reserve_refused(&key, err),
            Some(Op::AskRelease(task)) => self.ask_refused(&task, code),
            Some(Op::Answer(task)) => {
                eprintln!("hangar: team release answer refused ({code:?})");
                let me = self.me.clone();
                match self
                    .holds
                    .get_mut(&task)
                    .filter(|h| Some(&h.holder) == me.as_ref())
                    .and_then(|h| h.request.as_mut())
                {
                    Some(request) => {
                        request.answered = false;
                        vec![Effect::Changed]
                    }
                    None => Vec::new(),
                }
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
                self.appending = false;
                if let Some(holder) = confirmed {
                    self.wrap_queue.push(holder);
                }
                self.sync_log()
            }
            Some(Op::Rotate(_)) => {
                // Whether it landed, the log fetch tells (`reconcile`).
                self.rotating = false;
                self.sync_log()
            }
            Some(Op::NotesSet) => self.on_notes_stored(),
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
        self.log_fetches = self.log_fetches.saturating_add(1);
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
            let reason = match error {
                LogError::LogFull => AttentionReason::LogFull,
                _ => AttentionReason::HistoryMismatch,
            };
            return vec![Self::stop(reason)];
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
        // The log's owner must be the project's owner on the website: a relay could otherwise
        // show a newcomer a team it invented under the project's id.
        if let (Some(state), Some(owner)) = (self.verifier.state(), &self.owner) {
            if state.owner() != owner {
                eprintln!("hangar: the team log names another owner than the website");
                return vec![Self::stop(AttentionReason::OwnerMismatch)];
            }
        }
        // Without an owner from the website (an older server) nothing vouches for the log: a
        // device admitted before goes on, a new one does not start trusting it. The device that
        // created the team admits itself.
        if self.owner.is_none() && !self.keys.admitted {
            if let (Some(state), Some(me)) = (self.verifier.state(), &self.me) {
                if state.added_by(me) != Some(me) {
                    eprintln!("hangar: the website names no project owner to check the log by");
                    return vec![Self::stop(AttentionReason::NoWebsiteOwner)];
                }
            }
        }
        // The relay refused this connection's proof: only a removal the log shows still counts,
        // nothing acts on the connection and it never settles (the reconnect backs off).
        if self.prove_refused {
            if self.removed_from_log() {
                return vec![
                    Effect::WipeDevice,
                    Effect::End(SessionEnd::Stop(StopReason::Removed)),
                ];
            }
            return vec![Effect::End(SessionEnd::Lost)];
        }
        let mut effects = Vec::new();
        if self.check_withheld(head.map_or(0, |h| h.index)) {
            effects.push(Effect::Changed);
        }
        let first = !self.synced;
        if first {
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
        if self.removed_from_log() {
            return vec![
                Effect::WipeDevice,
                Effect::End(SessionEnd::Stop(StopReason::Removed)),
            ];
        }
        effects.extend(self.reconcile());
        effects.extend(self.poll_devices());
        effects.extend(self.retry_stashed());
        effects.extend(self.drain_tasks());
        // Every admitted device compares the members with the website's on every new connection.
        if first {
            effects.extend(self.want_members());
        }
        effects
    }

    /// The verified log removed this device (its last key period ended and it is not listed).
    fn removed_from_log(&self) -> bool {
        let (Some(state), Some(me)) = (self.verifier.state(), &self.me) else {
            return false;
        };
        let removed = state
            .key_periods(me)
            .last()
            .is_some_and(|p| p.removed_at.is_some());
        removed && state.device(me).is_none()
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
        if !self.keys.admitted {
            // The device that created the team admitted itself; any other was admitted by
            // another device, which the person checks before this one shares anything.
            match state.added_by(&me) {
                Some(by) if *by == me => {
                    self.keys.admitted = true;
                    effects.push(Effect::StoreKeys);
                }
                Some(by) => {
                    self.status = Self::admission(state, by);
                    return effects;
                }
                // A listed device always has the entry that added it (`TeamState` keeps both
                // together); without one there is nothing to compare, so nothing to confirm.
                None => {
                    self.status = waiting;
                    return effects;
                }
            }
        }
        let role = state.effective_role(&me.member, self.website_role);
        let manager = role.is_some_and(|r| role_rank(r) <= role_rank(Role::Admin));
        let current = state.current_epoch().map(|e| e.epoch);
        let due = state.rotation_due();
        let urgent = state.rotation_urgent();
        self.check_rotation_landed(current);
        let wants = current.is_none() || urgent || (due && self.deferred_rotation_free());
        if manager && !self.rotating && wants && self.may_rotate() {
            effects.extend(self.rotate(&me));
            self.status = TeamStatus::Connecting { retrying: false };
            return effects;
        }
        let Some(state) = self.verifier.state() else {
            return effects;
        };
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
        let overdue = self.overdue_key_change(manager);
        let Some(state) = self.verifier.state() else {
            return effects;
        };
        if state.rotation_urgent() {
            // A device or member left and only an owner or admin changes the key: until then
            // the current key may be in the removed one's hands, so nothing new is sealed. A
            // rotation that only removes devices added after the epoch began does not hold
            // sharing: their member had the key anyway (`TeamState::rotation_urgent`).
            self.status = TeamStatus::WaitingForKeyChange {
                reason: KeyChangeWait::Removal,
            };
            return effects;
        }
        if let Some(reason) = overdue {
            self.status = TeamStatus::WaitingForKeyChange { reason };
            return effects;
        }
        self.status = TeamStatus::Connected {
            epoch: epoch.0,
            members: state.members().len(),
            devices: state.device_count(),
        };
        if self.shared_epoch != Some(epoch) {
            effects.extend(self.share_now());
        }
        effects.extend(self.reconcile_members());
        effects.extend(self.notes_step());
        effects
    }

    /// The status asking the person to check who admitted this device (`by`, from the log).
    fn admission(state: &TeamState, by: &Holder) -> TeamStatus {
        let head = *state.head();
        let log_safety_number = state
            .log_safety_number_at(head.index)
            .map(|n| safety_groups(&n))
            .unwrap_or_default();
        let keys = state
            .key_periods(by)
            .iter()
            .rev()
            .find(|p| p.removed_at.is_none())
            .or_else(|| state.key_periods(by).last())
            .map(|p| p.keys);
        let number = keys
            .map(|k| safety_groups(&safety_number(by, &k)))
            .unwrap_or_default();
        TeamStatus::ConfirmAdmission {
            member: by.member.as_str().to_owned(),
            device: by.device.as_str().to_owned(),
            safety_number: number,
            log_index: head.index,
            log_safety_number,
        }
    }

    /// The person compared the safety numbers of the admitting device and of the log with that
    /// device and confirms: stored with the head, then this device fetches its key and shares.
    /// `log_index` and `safety_number` (the admitting device's) are what the window showed: when
    /// the team shows something else by now, nothing is confirmed ([`TeamError::Changed`]) and
    /// the window shows the new numbers to compare.
    pub fn confirm_admission(
        &mut self,
        log_index: u64,
        safety_number: &[String],
    ) -> Result<Vec<Effect>, TeamError> {
        let TeamStatus::ConfirmAdmission {
            log_index: shown_index,
            safety_number: shown_number,
            log_safety_number,
            ..
        } = &self.status
        else {
            return Err(TeamError::UnknownRequest);
        };
        // An empty number compares nothing: no admitting device or keys behind it.
        if shown_number.is_empty() || log_safety_number.is_empty() {
            return Err(TeamError::UnknownRequest);
        }
        if *shown_index != log_index || shown_number.as_slice() != safety_number {
            return Err(TeamError::Changed);
        }
        self.keys.admitted = true;
        let mut effects = vec![Effect::StoreKeys];
        effects.extend(self.reconcile());
        effects.extend(self.want_members());
        Ok(effects)
    }

    /// On an owner's or admin's device: the first difference between the log's members and the
    /// website's list, as one log entry. A member the website no longer lists (and the log
    /// listed when the list was asked for) is removed, which makes a rotation due that leaves
    /// its devices out; on the owner's device a role the website changed is written into the
    /// log. One entry at a time; the next follows the log fetch after its answer.
    fn reconcile_members(&mut self) -> Vec<Effect> {
        if !self.synced || self.syncing || self.creating || self.rotating || self.appending {
            return Vec::new();
        }
        if !self.manages() {
            self.promotions.clear();
            return Vec::new();
        }
        if self.roster_spent {
            return Vec::new();
        }
        let (Some(roster), Some(state), Some(me)) =
            (&self.roster, self.verifier.state(), self.me.as_ref())
        else {
            return Vec::new();
        };
        let Some(role) = state.effective_role(&me.member, self.website_role) else {
            return Vec::new();
        };
        let removal_fits = self.removal_fits();
        // A list that names neither this member nor the log's owner is not this project's
        // member list (an empty answer, ids of another kind): it removes nobody.
        if !roster.members.contains_key(&me.member) || !roster.members.contains_key(state.owner()) {
            return Vec::new();
        }
        let now = self.now();
        let mut candidates = Vec::new();
        let mut promotions = Vec::new();
        for (member, entry) in state.members() {
            if *member == me.member || member == state.owner() {
                continue;
            }
            match roster.members.get(member) {
                None if entry.added_at <= roster.at && removal_fits => {
                    candidates.push(EntryKind::RemoveMember {
                        member: member.clone(),
                    });
                }
                Some(Some(listed))
                    if role == Role::Owner
                        && *listed != Role::Owner
                        && entry.role != Role::Owner
                        && *listed != entry.role =>
                {
                    // Fewer rights follow the website at once; more rights only once the owner
                    // confirmed them here, so a website (or whoever took it over) cannot make
                    // someone an admin of the end-to-end team alone.
                    if role_rank(*listed) > role_rank(entry.role) {
                        candidates.push(EntryKind::ChangeRole {
                            member: member.clone(),
                            role: *listed,
                        });
                    } else {
                        promotions.push(self.promotion_view(member, *listed, entry.added_at));
                    }
                }
                _ => {}
            }
        }
        promotions.retain(|p| !self.local.rejected.contains(&p.id));
        self.promotions = promotions;
        let Some(state) = self.verifier.state() else {
            return Vec::new();
        };
        let Some(me) = self.me.as_ref() else {
            return Vec::new();
        };
        let body = candidates.into_iter().find_map(|kind| {
            let body = state.next_body(me.clone(), now, kind);
            state.check(&body).is_ok().then_some(body)
        });
        let Some(body) = body else {
            return Vec::new();
        };
        match body.sign(&self.identity) {
            Ok(append) => {
                self.appending = true;
                vec![self.frame(Op::Append(None), ClientMessage::MemberLogAppend(append))]
            }
            Err(error) => {
                eprintln!("hangar: team member entry not signed: {error}");
                Vec::new()
            }
        }
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

    /// A rotation this device sent and the relay answered: once the log fetch after it is in,
    /// a log without its epoch means the relay did not store it (counted like a refusal).
    fn check_rotation_landed(&mut self, current: Option<Epoch>) {
        if self.rotating || self.syncing {
            return;
        }
        let Some(sent) = self.rotation_sent.take() else {
            return;
        };
        if current.is_none_or(|current| current < sent) {
            eprintln!("hangar: a team key rotation was answered but is not in the log");
            self.rotate_failures = self.rotate_failures.saturating_add(1);
            self.rotate_hold = true;
        }
    }

    /// Whether this connection may try a rotation: fewer than [`MAX_ROTATE_FAILURES`] refused,
    /// and none waiting for the next ping.
    fn may_rotate(&self) -> bool {
        !self.rotate_hold && self.rotate_failures < MAX_ROTATE_FAILURES
    }

    /// Whether a rotation that is due but not urgent may go out now
    /// ([`DEFERRED_ROTATION_EVERY_MS`] after this device's last one).
    fn deferred_rotation_free(&self) -> bool {
        let now = (self.now)();
        self.rotated_at
            .is_none_or(|at| now < at || now - at >= DEFERRED_ROTATION_EVERY_MS)
    }

    /// Why a device that cannot change the key stops sealing although the log asks for no urgent
    /// rotation, if it does: the log still names a member the website removed
    /// [`DEPARTURE_GRACE_MS`] ago (the relay may hold back the remove member entry), or a
    /// rotation that waits ([`DEFERRED_ROTATION_EVERY_MS`]) is overdue because no owner or admin
    /// made it. Both are measured from this device's first observation on its own clock.
    fn overdue_key_change(&mut self, manager: bool) -> Option<KeyChangeWait> {
        let now = (self.now)();
        let state = self.verifier.state()?;
        let me = self.me.as_ref()?;
        // Departures: only from a list that names this member and the log's owner (the same
        // sanity rule as for removals), and never a member the log added after the list was
        // asked for. Without a list nothing new is concluded; departures the log has carried
        // out are forgotten.
        let usable = self.roster.as_ref().filter(|r| {
            r.members.contains_key(&me.member) && r.members.contains_key(state.owner())
        });
        match usable {
            Some(roster) => {
                let missing: BTreeSet<&MemberId> = state
                    .members()
                    .iter()
                    .filter(|(member, entry)| {
                        **member != me.member
                            && *member != state.owner()
                            && entry.added_at <= roster.at
                            && !roster.members.contains_key(*member)
                    })
                    .map(|(member, _)| member)
                    .collect();
                self.departed.retain(|member, _| missing.contains(member));
                for member in missing {
                    self.departed.entry(member.clone()).or_insert(now);
                }
            }
            None => self
                .departed
                .retain(|member, _| state.member(member).is_some()),
        }
        // A deferred rotation only removes devices a member added after the epoch began; such a
        // device got nothing its own member did not already have. Only a removal of one of this
        // member's own devices makes the wait count here, so another member's churn never stops
        // this device's sharing.
        let deferred = state.rotation_due()
            && !state.rotation_urgent()
            && state
                .removed_since_epoch()
                .any(|holder| holder.member == me.member);
        if !deferred {
            self.deferred_since = None;
        } else if self.deferred_since.is_none() {
            self.deferred_since = Some(now);
        }
        if self.departure_stuck(now) {
            return Some(KeyChangeWait::LogFull);
        }
        // An owner's or admin's device removes the member and rotates itself.
        if manager {
            return None;
        }
        if self.departure_overdue(now) {
            return Some(KeyChangeWait::MemberLeft);
        }
        if self.deferred_overdue(now) {
            return Some(KeyChangeWait::Overdue);
        }
        None
    }

    /// Whether a departure or a deferred rotation this device noted has run past its time at
    /// `now` (the ping then decides again).
    fn overdue_at(&self, now: u64) -> bool {
        self.departure_overdue(now) || self.deferred_overdue(now)
    }

    /// Whether the log still has room for a removal and the rotation it makes due (the log
    /// refuses the removal otherwise as well).
    fn removal_fits(&self) -> bool {
        self.verifier
            .state()
            .is_some_and(|state| state.head().index.saturating_add(2) <= self.max_log_entries)
    }

    /// A departure past its grace period that nobody can carry out: the log has no room for the
    /// removal, so only a new project helps.
    fn departure_stuck(&self, now: u64) -> bool {
        !self.removal_fits() && self.departure_overdue(now)
    }

    fn departure_overdue(&self, now: u64) -> bool {
        self.departed
            .values()
            .any(|since| now >= *since && now - since >= DEPARTURE_GRACE_MS)
    }

    fn deferred_overdue(&self, now: u64) -> bool {
        self.deferred_since
            .is_some_and(|since| now >= since && now - since >= DEFERRED_ROTATION_EVERY_MS)
    }

    /// A listed device signed a head this device's log has not reached; the next log fetch
    /// must reach it.
    fn note_ahead(&mut self, ahead: Option<u64>) {
        let Some(index) = ahead else {
            return;
        };
        if self.ahead.is_some_and(|(known, _)| known >= index) {
            return;
        }
        // `stash` starts the next fetch right after (or after the running one).
        let next = self.log_fetches.saturating_add(1);
        self.ahead = Some((index, next));
    }

    /// After a log fetch ended at `head`: whether the notice that the relay holds back log
    /// entries changed. Reaching the signed head ends it; a fetch that started after the head
    /// was noted and still ends before it raises it.
    fn check_withheld(&mut self, head: u64) -> bool {
        let before = self.history_withheld;
        match self.ahead {
            Some((index, _)) if head >= index => {
                self.ahead = None;
                self.history_withheld = false;
            }
            Some((_, fetch)) if self.log_fetches >= fetch => self.history_withheld = true,
            _ => {}
        }
        before != self.history_withheld
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
        self.rotation_sent = Some(rotation.epoch);
        self.rotated_at = Some((self.now)());
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

    /// The owner's confirmation that `member` (in the log since `added_at`) becomes `role`, as
    /// the website lists it. The id is stable for the member's time in the log, so a rejected
    /// promotion stays rejected until the member is added again.
    fn promotion_view(&self, member: &MemberId, role: Role, added_at: u64) -> PendingConfirmation {
        let mut sha = Sha256::new();
        for part in [
            b"hangar-team/app/role".as_slice(),
            self.team.as_str().as_bytes(),
            member.as_str().as_bytes(),
            &[role_rank(role)],
            &added_at.to_be_bytes(),
        ] {
            sha.update(u32::try_from(part.len()).unwrap_or(u32::MAX).to_be_bytes());
            sha.update(part);
        }
        PendingConfirmation {
            id: hex(&sha.finalize()[..16]),
            project_id: self.project.clone(),
            member: member.as_str().to_owned(),
            device: String::new(),
            kind: PendingKind::RoleChange,
            safety_number: Vec::new(),
            readmitted: false,
            log_full: self.verifier.state().is_some_and(TeamState::log_full),
        }
    }

    /// The change role entry a promotion stands for, while the website and the log still differ.
    fn promotion_body(&self, id: &str) -> Option<EntryBody> {
        let view = self.promotions.iter().find(|p| p.id == id)?;
        let state = self.verifier.state()?;
        let me = self.me.as_ref()?;
        let member = MemberId::new(view.member.clone()).ok()?;
        let listed = self
            .roster
            .as_ref()?
            .members
            .get(&member)
            .copied()
            .flatten()?;
        let entry = state.member(&member)?;
        if self.promotion_view(&member, listed, entry.added_at).id != id {
            return None;
        }
        let body = state.next_body(
            me.clone(),
            self.now(),
            EntryKind::ChangeRole {
                member,
                role: listed,
            },
        );
        state.check(&body).ok()?;
        Some(body)
    }

    /// The entry confirming `info`, if this device may and the log would take it; `true` when
    /// only the log's length stands in the way.
    fn confirmation_body(&self, info: &DeviceInfo) -> Option<(PendingKind, EntryBody, bool)> {
        let state = self.verifier.state()?;
        let me = self.me.as_ref()?;
        state.device(me)?;
        // A device whose own admission the person did not confirm vouches for nobody.
        if !self.keys.admitted || state.device(&info.holder).is_some() || info.holder == *me {
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
            // Only someone the website's list names: the relay alone must not get anyone offered
            // for admission, so without a list (not read yet) nobody new is.
            if !self.website_lists(&info.holder.member) {
                return None;
            }
            // Always as a member: a higher role on the website becomes its own confirmation on
            // the owner's device (`reconcile_members`, `PendingKind::RoleChange`), so confirming
            // a new device never makes anyone an admin unseen.
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
        // A request the full log has no room for stays visible as such (`full`), so the person
        // learns why it cannot be confirmed instead of the device waiting unseen.
        match state.check(&body) {
            Ok(()) => Some((kind, body, false)),
            Err(LogError::LogFull) => Some((kind, body, true)),
            Err(_) => None,
        }
    }

    /// Whether the website's list as last read names `member`; `false` without a list.
    fn website_lists(&self, member: &MemberId) -> bool {
        self.roster
            .as_ref()
            .is_some_and(|r| r.members.contains_key(member))
    }

    /// The waiting devices from `devices.get`; `true` when one belongs to a new member the
    /// website's list does not name (yet), or no list was read yet.
    fn update_pending(&mut self, devices: Vec<DeviceInfo>) -> bool {
        let mut pending = Vec::new();
        let mut unlisted = false;
        for info in devices {
            let Some((kind, _, log_full)) = self.confirmation_body(&info) else {
                unlisted |= self.verifier.state().is_some_and(|s| {
                    s.member(&info.holder.member).is_none()
                        && !self.website_lists(&info.holder.member)
                });
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
                    log_full,
                },
                info,
            });
        }
        self.pending = pending;
        unlisted
    }

    pub fn has_request(&self, id: &str) -> bool {
        self.pending.iter().any(|p| p.view.id == id) || self.promotions.iter().any(|p| p.id == id)
    }

    /// Confirms a waiting device: the add device or add member entry, then (after the relay
    /// took it) the current key wrapped to the device. For a promotion the website asked for,
    /// the change role entry.
    pub fn confirm(&mut self, id: &str) -> Result<Vec<Effect>, TeamError> {
        if self.promotions.iter().any(|p| p.id == id) {
            if self.verifier.state().is_some_and(TeamState::log_full) {
                return Err(TeamError::LogFull);
            }
            if self.syncing || self.creating || self.rotating || self.appending {
                return Err(TeamError::Busy);
            }
            let body = self.promotion_body(id).ok_or(TeamError::UnknownRequest)?;
            let append = body
                .sign(&self.identity)
                .map_err(|_| TeamError::UnknownRequest)?;
            self.promotions.retain(|p| p.id != id);
            self.appending = true;
            return Ok(vec![
                self.frame(Op::Append(None), ClientMessage::MemberLogAppend(append)),
                Effect::Changed,
            ]);
        }
        let at = self
            .pending
            .iter()
            .position(|p| p.view.id == id)
            .ok_or(TeamError::UnknownRequest)?;
        if self.syncing || self.creating || self.rotating {
            return Err(TeamError::Busy);
        }
        let info = self.pending[at].info.clone();
        let (_, body, log_full) = self
            .confirmation_body(&info)
            .ok_or(TeamError::UnknownRequest)?;
        if log_full {
            return Err(TeamError::LogFull);
        }
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
        if self.promotions.iter().any(|p| p.id == id) {
            self.promotions.retain(|p| p.id != id);
            self.local.reject(id);
            return Ok(vec![Effect::StoreLocal, Effect::Changed]);
        }
        let at = self
            .pending
            .iter()
            .position(|p| p.view.id == id)
            .ok_or(TeamError::UnknownRequest)?;
        self.pending.remove(at);
        self.local.reject(id);
        Ok(vec![Effect::StoreLocal, Effect::Changed])
    }

    /// Removes another device of the signed-in person from the team, a lost one for example
    /// (`device.remove` with its signed entry). A rotation follows on an owner's or admin's
    /// device; until then the team waits for the key change.
    pub fn remove_own_device(&mut self, device: &DeviceId) -> Result<Vec<Effect>, TeamError> {
        let me = self.me.clone().ok_or(TeamError::Busy)?;
        if !self.keys.admitted || *device == me.device {
            return Err(TeamError::UnknownRequest);
        }
        let holder = Holder {
            member: me.member,
            device: device.clone(),
        };
        let body = self.removal_body(EntryKind::RemoveDevice {
            holder: holder.clone(),
        })?;
        let log = body
            .sign(&self.identity)
            .map_err(|_| TeamError::UnknownRequest)?;
        let remove = DeviceRemove { holder, log };
        Ok(vec![
            self.frame(Op::Remove, ClientMessage::DeviceRemove(remove))
        ])
    }

    /// Removes a member with all its devices (owner or admin). The app writes it from the
    /// website's member list ([`Engine::reconcile_members`]); tests call it directly.
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
        state.check(&body).map_err(|error| match error {
            LogError::LogFull => TeamError::LogFull,
            _ => TeamError::UnknownRequest,
        })?;
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
        let new_epoch = self.shared_epoch != Some(epoch);
        if new_epoch {
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

        // The agents changed files: the reservations of this device count as active.
        if sends
            .iter()
            .any(|m| matches!(m, ClientMessage::ClaimSet(_)))
        {
            sends.extend(self.heartbeat());
        }

        let mut effects = Vec::new();
        if sealed_any {
            effects.push(Effect::StoreLocal);
        }
        for message in sends {
            effects.push(self.frame(Op::Quiet, message));
        }
        // A new connection or a new epoch: reservations of an earlier epoch move to the current
        // hash (`prev`), and attempts whose answer was lost with a connection go again.
        if new_epoch {
            effects.extend(self.resend_attempts(epoch));
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
        self.open_record(kind, record, sender, sealed, false)
    }

    /// [`Engine::open_sealed`]; `older`: a record of an earlier epoch whose key this device
    /// still has opens too (a reservation's note stands while its hold was not rekeyed).
    fn open_record(
        &self,
        kind: RecordKind,
        record: &[u8],
        sender: &Holder,
        sealed: &Sealed,
        older: bool,
    ) -> Open {
        let Some(state) = self.verifier.state() else {
            return Open::Later(None);
        };
        let current = state.current_epoch().map_or(0, |e| e.epoch.0);
        if sealed.epoch.0 < current && !older {
            // Live state is sealed under the current epoch; an older one is from before a
            // rotation (perhaps by a device removed since) and is not shown.
            return Open::Failed;
        }
        let Some(secret) = self.keys.epochs.get(&sealed.epoch) else {
            // An epoch this device has no key for yet (a rotation under way) comes later.
            return if sealed.epoch.0 > current {
                Open::Later(None)
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
            Err(OpenError::AheadOfLog { index, signed }) => Open::Later(signed.then_some(index)),
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
                    match self
                        .live
                        .presence(&update.holder, self.stamp(&update.holder, &opened))
                    {
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
                Open::Later(ahead) if first => {
                    self.note_ahead(ahead);
                    return self.stash(ServerMessage::PresenceUpdate(update));
                }
                // Only a record the listed sender signed reports a fork (`open_verified`).
                Open::Fork => return self.diverged(&update.holder),
                Open::Later(_) | Open::Failed => {}
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

    /// When `opened` was sealed: the key period its signing key belongs to, then its counter.
    fn stamp(&self, holder: &Holder, opened: &Opened) -> Stamp {
        let period = self.verifier.state().map_or(0, |state| {
            state
                .key_periods(holder)
                .iter()
                .find(|p| p.keys.sign == opened.sign)
                .map_or(0, |p| p.added_at)
        });
        Stamp {
            period,
            counter: opened.counter,
        }
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
        let in_status = signed.is_some_and(|peer| {
            peer.status
                .sessions
                .iter()
                .any(|s| s.key == overlap.other.session.as_str())
        });
        if signed.is_some() && !in_status {
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
            // Per own session and teammate device: one device inventing session keys cannot
            // take the room of the others.
            let held = self.overlaps.get(&session).map_or(0, |others| {
                others
                    .keys()
                    .filter(|other| other.holder == overlap.other.holder)
                    .count()
            });
            if held >= MAX_OVERLAPS_PER_DEVICE {
                return Vec::new();
            }
        }
        let mut effects = Vec::new();
        let mut detail = known.and_then(|e| e.detail.clone());
        // A detail counts only for a session the device's newest signed status lists: without
        // one, the relay alone vouches that the session runs, and only the level is kept.
        if !in_status {
            detail = None;
        }
        if let Some(sealed) = overlap.detail.as_ref().filter(|_| in_status) {
            let record = overlap.other.session.as_str().as_bytes();
            match self.open_sealed(
                RecordKind::ClaimDetail,
                record,
                &overlap.other.holder,
                sealed,
            ) {
                Open::Opened(opened) => {
                    let stamp = self.stamp(&overlap.other.holder, &opened);
                    let mark = self.live.detail(
                        &overlap.other.holder,
                        overlap.other.session.as_str(),
                        stamp,
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
                Open::Later(ahead) if first => {
                    self.note_ahead(ahead);
                    return self.stash(ServerMessage::ClaimOverlap(overlap));
                }
                Open::Fork => return self.diverged(&overlap.other.holder),
                Open::Later(_) | Open::Failed => {}
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
    ///
    /// The fork may also be this device's: the relay can show it an entry the others never got
    /// (after a restart, past the head it stored). So every divergence fetches the log again,
    /// and the team stops only when that fetch contradicts what this device verified before
    /// (`after_sync`). Reports of other devices never stop it, however many and of however many
    /// members: two members working together could otherwise stop everyone's sharing with one
    /// record each, and which devices are heard is the relay's word anyway. Every reporting
    /// device stays listed in the Team view, so the people can compare log safety numbers.
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
        let members: BTreeSet<&MemberId> = self.divergent.keys().map(|h| &h.member).collect();
        if members.len() >= 2 {
            eprintln!("hangar: devices of several members report another team history");
        }
        let mut effects = vec![Effect::Changed];
        effects.extend(self.sync_log());
        effects
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
            tasks: self.task_views(),
            attempts: self.attempt_views(),
            sessions: self.share.sessions.iter().map(|s| s.id).collect(),
            session_repos: self
                .share
                .sessions
                .iter()
                .map(|s| SessionRepo {
                    session_id: s.id,
                    repo: s.repo.clone(),
                })
                .collect(),
            notices: self.notices.iter().cloned().collect(),
            devices: self.own_devices(),
            log: self.verifier.state().and_then(|state| {
                let index = state.head().index;
                state.log_safety_number_at(index).map(|n| LogSafety {
                    index,
                    safety_number: safety_groups(&n),
                })
            }),
            log_full: self.verifier.state().is_some_and(TeamState::log_full),
            history_withheld: self.history_withheld,
        }
    }

    /// The signed-in person's devices the verified log lists, this one first.
    fn own_devices(&self) -> Vec<OwnDevice> {
        let (Some(state), Some(me)) = (self.verifier.state(), self.me.as_ref()) else {
            return Vec::new();
        };
        let Some(member) = state.member(&me.member) else {
            return Vec::new();
        };
        let mut devices: Vec<OwnDevice> = member
            .devices
            .values()
            .map(|entry| {
                let holder = Holder {
                    member: me.member.clone(),
                    device: entry.device.clone(),
                };
                OwnDevice {
                    device: entry.device.as_str().to_owned(),
                    this: entry.device == me.device,
                    recovery: entry.recovery,
                    safety_number: safety_groups(&safety_number(&holder, &entry.keys)),
                }
            })
            .collect();
        devices.sort_by_key(|d| !d.this);
        devices
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
    // Reservations ("I'm working on ...")
    //
    // The relay decides who holds a task, in arrival order (first wins); this device never
    // applies a reservation of its own before the relay's event says so. The relay's events are
    // not signed: a hold counts only for a holder the verified log lists, its title and area come
    // only from the note the holder signed (bound to the task's hash), and an older note shown
    // again is ignored. Events whose holder the log does not list yet wait for the next log
    // fetch, in order; then they are dropped.
    // -----------------------------------------------------------------------------------------

    /// Starts over with the relay's snapshot (`resync_required`): the holds go, this device's
    /// attempts stay and learn again who holds what. The note marks stay: the snapshot must not
    /// bring back a note of a hold that ended.
    fn reset_tasks(&mut self) {
        // Holds missing from the snapshot ended meanwhile: their notes are old after it.
        // A second resync before the first one's snapshot was checked keeps the first holds.
        for (task, hold) in std::mem::take(&mut self.holds) {
            self.before_resync.entry(task).or_insert(hold);
        }
        self.own_check = true;
        self.task_queue.clear();
        for attempt in self.attempts.values_mut() {
            attempt.blocked = None;
            attempt.asked = false;
            attempt.verified = false;
        }
    }

    /// The normalized id of this device's attempt whose last hash is `task`.
    fn attempt_key(&self, task: &KeyedHash) -> Option<String> {
        self.attempts
            .iter()
            .find(|(_, a)| a.hash.as_ref() == Some(task))
            .map(|(key, _)| key.clone())
    }

    fn parse_task(task: &str) -> Result<KeyedHash, TeamError> {
        KeyedHash::try_from(task.to_owned()).map_err(|_| TeamError::UnknownTask)
    }

    /// The task's hashes under the earlier epochs whose keys this device still has, newest
    /// first, but only those a hold of an earlier epoch stands under (this device's own, moved
    /// by the reserve, or a teammate's, who keeps the task while offline during a rotation): a
    /// hash nobody holds would only tell the relay which hashes of two epochs belong together.
    fn prev_hashes(&self, id: &TaskId, epoch: Epoch) -> Vec<KeyedHash> {
        let Some(state) = self.verifier.state() else {
            return Vec::new();
        };
        self.keys
            .epochs
            .iter()
            .rev()
            .filter(|(old, _)| **old < epoch)
            .filter_map(|(old, secret)| EpochCipher::for_epoch(state, secret, *old).ok())
            .map(|cipher| cipher.match_key().task(id))
            .filter(|hash| self.holds.get(hash).is_some_and(|h| h.epoch < epoch))
            .take(MAX_PREV_HASHES)
            .collect()
    }

    /// Reserves a task in the person's name: `title` as the person typed it, optionally linked
    /// to one of this device's shared sessions and a folder of its repository. The relay decides;
    /// the Team view shows the attempt until its answer.
    pub fn reserve_task(
        &mut self,
        title: &str,
        session: Option<SessionId>,
        area: Option<&str>,
    ) -> Result<Vec<Effect>, TeamError> {
        let title = title.trim();
        if title.chars().count() > MAX_TITLE_CHARS {
            return Err(TeamError::InvalidTask);
        }
        let id = TaskId::normalize(title).map_err(|_| TeamError::InvalidTask)?;
        let linked = match session {
            Some(id) => Some(
                self.share
                    .sessions
                    .iter()
                    .find(|s| s.id == id)
                    .cloned()
                    .ok_or(TeamError::UnknownRepository)?,
            ),
            None => None,
        };
        let area = match area.map(str::trim).filter(|a| !a.is_empty()) {
            // A folder means something only in the repository of a linked session.
            Some(raw) if linked.is_some() => {
                Some(team_tasks::normalize_area(raw).ok_or(TeamError::InvalidTask)?)
            }
            Some(_) => return Err(TeamError::InvalidTask),
            None => None,
        };
        if self.current_cipher().is_none() {
            return Err(TeamError::Busy);
        }
        let key = id.as_str().to_owned();
        if !self.attempts.contains_key(&key) && self.attempts.len() >= MAX_RESERVATIONS_PER_MEMBER {
            return Err(TeamError::TaskLimit);
        }
        self.attempts.insert(
            key.clone(),
            Attempt {
                title: title.to_owned(),
                session,
                repo: linked.map(|s| s.repo),
                area,
                hash: None,
                blocked: None,
                asked: false,
                renoted: false,
                verified: false,
            },
        );
        let mut effects = self.send_reserve(&key);
        effects.push(Effect::Changed);
        Ok(effects)
    }

    /// `task.reserve` for an attempt, with its note sealed under the current epoch; the record
    /// counter is stored first.
    fn send_reserve(&mut self, key: &str) -> Vec<Effect> {
        let Some((cipher, head, epoch)) = self.current_cipher() else {
            return Vec::new();
        };
        let Some(attempt) = self.attempts.get(key).cloned() else {
            return Vec::new();
        };
        let Ok(id) = TaskId::normalize(&attempt.title) else {
            return Vec::new();
        };
        let task = cipher.match_key().task(&id);
        let prev = self.prev_hashes(&id, epoch);
        let note = TaskNote {
            title: attempt.title.clone(),
            name: self.share.person.clone(),
            repo: attempt.repo.clone(),
            area: attempt.area.clone(),
            session: attempt
                .session
                .and_then(|s| self.session_keys.get(&s))
                .map(|k| k.as_str().to_owned()),
        };
        let plaintext = team_tasks::note_plaintext(&note);
        let record = record_task_id(&task);
        let Some(sealed) = self.seal(
            &cipher,
            &head,
            RecordKind::ReservationNote,
            record.as_bytes(),
            &plaintext,
        ) else {
            return Vec::new();
        };
        if let Some(attempt) = self.attempts.get_mut(key) {
            attempt.hash = Some(task);
        }
        vec![
            Effect::StoreLocal,
            self.frame(
                Op::Reserve(key.to_owned()),
                ClientMessage::TaskReserve(TaskReserve {
                    task,
                    epoch,
                    prev,
                    note: Some(sealed),
                }),
            ),
        ]
    }

    /// After a new connection or epoch: every attempt not blocked and not held under the current
    /// epoch goes again (the holder's own repeat only counts as activity).
    fn resend_attempts(&mut self, epoch: Epoch) -> Vec<Effect> {
        let me = self.me.clone();
        let keys: Vec<String> = self
            .attempts
            .iter()
            .filter(|(_, a)| a.blocked.is_none())
            .filter(|(key, a)| !self.awaits_own_check(key, a))
            .filter(|(_, a)| {
                let held = a
                    .hash
                    .and_then(|hash| self.holds.get(&hash))
                    .is_some_and(|h| Some(&h.holder) == me.as_ref() && h.epoch == epoch);
                !held
            })
            .map(|(key, _)| key.clone())
            .collect();
        keys.iter().flat_map(|key| self.send_reserve(key)).collect()
    }

    /// The reservations this device holds, as a heartbeat, at most every
    /// [`HEARTBEAT_EVERY_MS`].
    fn heartbeat(&mut self) -> Option<ClientMessage> {
        let me = self.me.clone()?;
        let tasks: Vec<KeyedHash> = self
            .holds
            .iter()
            .filter(|(_, h)| h.holder == me)
            .map(|(task, _)| *task)
            .take(MAX_HEARTBEAT_TASKS)
            .collect();
        if tasks.is_empty() {
            return None;
        }
        let now = (self.now)();
        if self
            .heartbeat_at
            .is_some_and(|at| now >= at && now - at < HEARTBEAT_EVERY_MS)
        {
            return None;
        }
        self.heartbeat_at = Some(now);
        Some(ClientMessage::Heartbeat(Heartbeat { tasks }))
    }

    /// Releases a task this device holds, or gives up an attempt (withdrawing its release
    /// request).
    pub fn release_task(&mut self, task: &str) -> Result<Vec<Effect>, TeamError> {
        let task = Self::parse_task(task)?;
        let me = self.me.clone().ok_or(TeamError::Busy)?;
        let key = self.attempt_key(&task);
        let mut effects = Vec::new();
        if self.holds.get(&task).is_some_and(|h| h.holder == me) {
            effects.push(self.frame(Op::Quiet, ClientMessage::TaskRelease(TaskRef { task })));
        } else {
            let attempt = key
                .as_ref()
                .and_then(|key| self.attempts.get(key))
                .ok_or(TeamError::UnknownTask)?;
            if attempt.asked {
                effects.push(self.frame(
                    Op::Quiet,
                    ClientMessage::TaskReleaseWithdraw(TaskRef { task }),
                ));
            }
        }
        if let Some(key) = key {
            self.attempts.remove(&key);
        }
        effects.push(Effect::Changed);
        Ok(effects)
    }

    /// Asks the holder of a task to hand it over (`task.release_request`, with this person's
    /// name sealed for the holder's card).
    pub fn ask_release(&mut self, task: &str) -> Result<Vec<Effect>, TeamError> {
        let task = Self::parse_task(task)?;
        let me = self.me.clone().ok_or(TeamError::Busy)?;
        let hold = self
            .holds
            .get(&task)
            .cloned()
            .ok_or(TeamError::UnknownTask)?;
        if hold.holder.member == me.member {
            return Err(TeamError::UnknownTask);
        }
        if hold.request.is_some() {
            return Err(TeamError::Busy);
        }
        // Before anything changes: without the epoch's key no request goes out, and no attempt
        // may be left behind for one that never went.
        let (cipher, head, _) = self.current_cipher().ok_or(TeamError::Busy)?;
        let key = match self.attempt_key(&task) {
            Some(key) => key,
            None => {
                // Asked from the list: an accepted request makes the task this device's, and its
                // title must be known to show it and to move it on a rotation.
                let note = hold.note.clone().ok_or(TeamError::UnknownTask)?;
                let id = TaskId::normalize(&note.title).map_err(|_| TeamError::UnknownTask)?;
                if self.attempts.len() >= MAX_RESERVATIONS_PER_MEMBER {
                    return Err(TeamError::TaskLimit);
                }
                let key = id.as_str().to_owned();
                self.attempts.insert(
                    key.clone(),
                    Attempt {
                        title: note.title,
                        session: None,
                        repo: None,
                        area: None,
                        hash: Some(task),
                        blocked: Some(hold.holder.clone()),
                        asked: false,
                        renoted: false,
                        verified: true,
                    },
                );
                key
            }
        };
        let plaintext = team_tasks::request_plaintext(&self.share.person);
        let record = record_task_id(&task);
        let note = self.seal(
            &cipher,
            &head,
            RecordKind::ReleaseRequestNote,
            record.as_bytes(),
            &plaintext,
        );
        if let Some(attempt) = self.attempts.get_mut(&key) {
            attempt.asked = true;
        }
        Ok(vec![
            Effect::StoreLocal,
            self.frame(
                Op::AskRelease(task),
                ClientMessage::TaskReleaseRequest(TaskReleaseRequest { task, note }),
            ),
            Effect::Changed,
        ])
    }

    /// The holder's answer to the open release request of a task this device holds. `requester`
    /// is the member the card showed; the answer goes only to that request, only when one of its
    /// listed devices signed it, and only once (the card is gone until the relay's word).
    pub fn answer_release(
        &mut self,
        task: &str,
        requester: &str,
        accept: bool,
    ) -> Result<Vec<Effect>, TeamError> {
        let task = Self::parse_task(task)?;
        let me = self.me.clone().ok_or(TeamError::Busy)?;
        let request = self
            .holds
            .get_mut(&task)
            .filter(|h| h.holder == me)
            .and_then(|h| h.request.as_mut())
            .filter(|r| r.signer.is_some() && !r.answered && r.requester.as_str() == requester)
            .ok_or(TeamError::UnknownTask)?;
        request.answered = true;
        let requester = request.requester.clone();
        Ok(vec![
            self.frame(
                Op::Answer(task),
                ClientMessage::TaskReleaseAnswer(TaskReleaseAnswer {
                    task,
                    requester,
                    accept,
                }),
            ),
            Effect::Changed,
        ])
    }

    fn reserve_refused(&mut self, key: &str, err: ErrorBody) -> Vec<Effect> {
        let Some(attempt) = self.attempts.get(key) else {
            return Vec::new();
        };
        let (title, session) = (attempt.title.clone(), attempt.session);
        match err.code {
            // First wins: someone else holds the task. The relay names the holder; it counts as
            // soon as the verified log lists it, and one it does not list yet waits for the next
            // log fetch (`drain_tasks`).
            ErrorCode::Conflict => {
                let Some(holder) = err.holder else {
                    self.attempts.remove(key);
                    self.notice(NoticeKind::ReserveFailed, title, None, None, session);
                    return vec![Effect::Changed];
                };
                let verified = self.listed(&holder);
                if let Some(attempt) = self.attempts.get_mut(key) {
                    attempt.blocked = Some(holder);
                    attempt.verified = verified;
                }
                let mut effects = vec![Effect::Changed];
                if !verified {
                    effects.extend(self.fetch_for_unknown());
                }
                effects
            }
            ErrorCode::Limit => {
                self.attempts.remove(key);
                self.notice(NoticeKind::Limit, title, None, None, session);
                vec![Effect::Changed]
            }
            // Another epoch began meanwhile: the attempt goes again with it.
            ErrorCode::StaleEpoch => Vec::new(),
            code => {
                eprintln!("hangar: team reservation refused ({code:?})");
                self.attempts.remove(key);
                self.notice(NoticeKind::ReserveFailed, title, None, None, session);
                vec![Effect::Changed]
            }
        }
    }

    fn ask_refused(&mut self, task: &KeyedHash, code: ErrorCode) -> Vec<Effect> {
        eprintln!("hangar: team release request refused ({code:?})");
        let Some(key) = self.attempt_key(task) else {
            return Vec::new();
        };
        let (title, session) = match self.attempts.get_mut(&key) {
            Some(attempt) => {
                attempt.asked = false;
                (attempt.title.clone(), attempt.session)
            }
            None => return Vec::new(),
        };
        if matches!(code, ErrorCode::Conflict | ErrorCode::RateLimited) {
            let holder = self.holds.get(task).map(|h| h.holder.clone());
            self.notice(
                NoticeKind::AskRefused,
                title,
                holder.as_ref(),
                None,
                session,
            );
        }
        vec![Effect::Changed]
    }

    /// A notice for the window, once; the same warning for the same deadline is not repeated.
    fn notice(
        &mut self,
        kind: NoticeKind,
        title: String,
        other: Option<&Holder>,
        at: Option<UnixMillis>,
        session_id: Option<SessionId>,
    ) {
        let at = at.map(|a| a.0);
        if self
            .notices
            .iter()
            .any(|n| n.kind == kind && n.title == title && n.at == at && at.is_some())
        {
            return;
        }
        self.notice_seq += 1;
        let (member, name) = match other {
            Some(holder) => (holder.member.as_str().to_owned(), self.name_of(holder)),
            None => (String::new(), String::new()),
        };
        self.notices.push_back(TaskNotice {
            id: (self.now)()
                .saturating_mul(1000)
                .saturating_add(self.notice_seq % 1000),
            kind,
            title,
            member,
            name,
            at,
            session_id,
        });
        while self.notices.len() > MAX_NOTICES {
            self.notices.pop_front();
        }
    }

    /// The display name a device of the team gave: its hold's note, else its presence.
    fn name_of(&self, holder: &Holder) -> String {
        self.holds
            .values()
            .filter(|h| h.holder.member == holder.member)
            .find_map(|h| h.note.as_ref().map(|n| n.name.clone()))
            .or_else(|| {
                self.peers
                    .iter()
                    .find(|(h, _)| h.member == holder.member)
                    .map(|(_, p)| p.status.name.clone())
            })
            .unwrap_or_default()
    }

    /// A relay event about a reservation: applied once the verified log can vouch for its holder,
    /// in the order the relay sent them.
    fn on_task(&mut self, message: ServerMessage) -> Vec<Effect> {
        if !self.synced || self.syncing || !self.task_queue.is_empty() {
            return self.queue_task(message);
        }
        let mut effects = self.apply_task(message, true);
        effects.extend(self.remember_own_holds());
        effects
    }

    fn queue_task(&mut self, message: ServerMessage) -> Vec<Effect> {
        if self.task_queue.len() >= MAX_TASK_QUEUE {
            // Too far behind to keep the order: start over from the relay's snapshot.
            eprintln!("hangar: team reservations fell behind; taking a new snapshot");
            self.reset_tasks();
            self.book_known = false;
            return vec![Effect::End(SessionEnd::Gap)];
        }
        self.task_queue.push(message);
        self.fetch_for_unknown()
    }

    /// A log fetch for events that wait for an unknown holder, at most every
    /// [`UNKNOWN_FETCH_EVERY_MS`] (the next ping fetches otherwise), so a relay naming made-up
    /// holders cannot make this device fetch the log for every event.
    fn fetch_for_unknown(&mut self) -> Vec<Effect> {
        if !self.synced || self.syncing {
            return Vec::new();
        }
        let now = (self.now)();
        if self
            .unknown_fetch_at
            .is_some_and(|at| now >= at && now - at < UNKNOWN_FETCH_EVERY_MS)
        {
            return Vec::new();
        }
        self.unknown_fetch_at = Some(now);
        self.sync_log()
    }

    /// The waiting task events after a log fetch, then the attempts that waited for their
    /// holder, then the notes that could not be opened.
    fn drain_tasks(&mut self) -> Vec<Effect> {
        let mut effects = Vec::new();
        // Holds of devices the log no longer lists go.
        let gone: Vec<KeyedHash> = self
            .holds
            .iter()
            .filter(|(_, h)| !self.listed(&h.holder))
            .map(|(task, _)| *task)
            .collect();
        for task in &gone {
            self.holds.remove(task);
        }
        if !gone.is_empty() {
            effects.push(Effect::Changed);
        }
        for message in std::mem::take(&mut self.task_queue) {
            effects.extend(self.apply_task(message, false));
        }
        // After a resync's snapshot: a hold from before that is gone (or another device's now)
        // ended while this device did not hear it; its note is old from now on. A log answer
        // in the middle of the snapshot waits: holds still to come are not gone.
        let before = if self.resyncing {
            BTreeMap::new()
        } else {
            std::mem::take(&mut self.before_resync)
        };
        for (task, hold) in before {
            let still = self
                .holds
                .get(&task)
                .is_some_and(|h| h.holder == hold.holder);
            if !still {
                self.mark_ended(&task, &hold);
            }
        }
        if self.own_check && !self.resyncing {
            effects.extend(self.check_own_holds());
        }
        // A conflict named a holder the log did not list: listed now, it counts; still not
        // listed, the relay's word was empty and the attempt ends with a notice.
        let waiting: Vec<(String, Holder)> = self
            .attempts
            .iter()
            .filter(|(_, a)| !a.verified)
            .filter_map(|(key, a)| Some((key.clone(), a.blocked.clone()?)))
            .collect();
        for (key, holder) in waiting {
            if self.listed(&holder) {
                if let Some(attempt) = self.attempts.get_mut(&key) {
                    attempt.verified = true;
                }
            } else if let Some(attempt) = self.attempts.remove(&key) {
                self.notice(
                    NoticeKind::ReserveFailed,
                    attempt.title,
                    None,
                    None,
                    attempt.session,
                );
            }
            effects.push(Effect::Changed);
        }
        let unopened: Vec<(KeyedHash, Holder, Sealed)> = self
            .holds
            .iter()
            .filter_map(|(task, h)| Some((*task, h.holder.clone(), h.unopened.clone()?)))
            .collect();
        for (task, holder, sealed) in unopened {
            let opened = self.open_note(&task, &holder, &sealed);
            let Some(hold) = self.holds.get_mut(&task) else {
                continue;
            };
            match opened {
                NoteOpen::Note(note) => {
                    hold.note = Some(note);
                    hold.unopened = None;
                    effects.push(Effect::Changed);
                }
                NoteOpen::Fork => {
                    hold.unopened = None;
                    effects.extend(self.diverged(&holder));
                }
                // Still waiting for a log entry or an epoch: kept (one per hold), so its
                // hold's end can mark it old.
                NoteOpen::Later => {}
                NoteOpen::Keep => hold.unopened = None,
            }
        }
        effects.extend(self.remember_own_holds());
        effects.extend(self.marks_raised_if_dirty());
        effects
    }

    /// The normalized task id of a title (the key of [`Engine::attempts`]).
    fn task_key(title: &str) -> Option<String> {
        TaskId::normalize(title)
            .ok()
            .map(|id| id.as_str().to_owned())
    }

    /// After the relay's snapshot (a resync, also the first connection after a restart): a hold
    /// this device had before that no hold of this person stands for now, under its hash or
    /// its title, ended while the device was away. The person gets a notice, and its attempt
    /// goes, so it is not reserved again without the person knowing. A hold that still stands
    /// under an earlier epoch moves to the current hash.
    fn check_own_holds(&mut self) -> Vec<Effect> {
        self.own_check = false;
        let Some(me) = self.me.clone() else {
            return Vec::new();
        };
        let epoch = self.current_cipher().map(|(_, _, epoch)| epoch);
        let mut effects = Vec::new();
        let before: Vec<OwnHold> = self
            .local
            .own_holds
            .iter()
            .take(MAX_RESERVATIONS_PER_MEMBER)
            .cloned()
            .collect();
        for own in before {
            let key = Self::task_key(&own.title);
            let standing = self
                .holds
                .iter()
                .find(|(task, hold)| {
                    hold.holder.member == me.member
                        && (record_task_id(task) == own.task
                            || (key.is_some()
                                && hold.note.as_ref().and_then(|n| Self::task_key(&n.title))
                                    == key))
                })
                .map(|(_, hold)| (hold.holder.clone(), hold.epoch));
            match standing {
                Some((holder, held_in)) => {
                    let older = epoch.is_some_and(|current| held_in < current);
                    if holder == me && older {
                        if let Some(key) = key.filter(|k| self.attempts.contains_key(k)) {
                            effects.extend(self.send_reserve(&key));
                        }
                    }
                }
                None => {
                    let session = key
                        .and_then(|k| self.attempts.remove(&k))
                        .and_then(|attempt| attempt.session);
                    self.notice(NoticeKind::EndedAway, own.title, None, None, session);
                    effects.push(Effect::Changed);
                }
            }
        }
        effects
    }

    /// Keeps [`LocalState::own_holds`] in step with this device's holds once they are settled:
    /// not while a snapshot is still to come or to be checked. A hold whose note did not open
    /// keeps the title it had.
    fn remember_own_holds(&mut self) -> Vec<Effect> {
        if self.own_check || self.resyncing || !self.synced {
            return Vec::new();
        }
        let Some(me) = self.me.clone() else {
            return Vec::new();
        };
        let own: Vec<OwnHold> = self
            .holds
            .iter()
            .filter(|(_, hold)| hold.holder == me)
            .filter_map(|(task, hold)| {
                let id = record_task_id(task);
                let title = hold.note.as_ref().map(|n| n.title.clone()).or_else(|| {
                    self.local
                        .own_holds
                        .iter()
                        .find(|own| own.task == id)
                        .map(|own| own.title.clone())
                })?;
                Some(OwnHold { task: id, title })
            })
            .take(MAX_RESERVATIONS_PER_MEMBER)
            .collect();
        if own == self.local.own_holds {
            return Vec::new();
        }
        self.local.own_holds = own;
        vec![Effect::StoreLocal]
    }

    /// An attempt for a hold of this device that waits for the snapshot's check: sending it
    /// again before would reserve a task that ran out meanwhile without the person knowing.
    fn awaits_own_check(&self, key: &str, attempt: &Attempt) -> bool {
        self.own_check
            && self.local.own_holds.iter().any(|own| {
                attempt
                    .hash
                    .is_some_and(|hash| record_task_id(&hash) == own.task)
                    || Self::task_key(&own.title).as_deref() == Some(key)
            })
    }

    /// The live marks rose while handling task events: stored like any other rise.
    fn marks_raised_if_dirty(&mut self) -> Vec<Effect> {
        if self.note_marks_rose {
            self.note_marks_rose = false;
            return self.marks_raised();
        }
        Vec::new()
    }

    fn task_holder(message: &ServerMessage) -> Option<&Holder> {
        match message {
            ServerMessage::TaskReserved(m) => Some(&m.holder),
            ServerMessage::TaskReleased(m) => Some(&m.holder),
            ServerMessage::ReservationExpiring(m) => Some(&m.holder),
            ServerMessage::ReservationExpired(m) => Some(&m.holder),
            ServerMessage::TaskReleaseRequested(m) => Some(&m.holder),
            ServerMessage::TaskReleaseAnswered(m) => Some(&m.holder),
            ServerMessage::TaskReleaseRequestEnded(m) => Some(&m.holder),
            _ => None,
        }
    }

    /// `can_wait`: a holder the log does not list yet may be one added since the last fetch;
    /// the event waits for the next fetch once.
    fn apply_task(&mut self, message: ServerMessage, can_wait: bool) -> Vec<Effect> {
        let Some(holder) = Self::task_holder(&message).cloned() else {
            return Vec::new();
        };
        if !self.listed(&holder) {
            if can_wait {
                self.task_queue.push(message);
                return self.fetch_for_unknown();
            }
            // A device the log does not vouch for holds nothing here; the end of a hold it had
            // before its removal still ends it.
            let ended = match &message {
                ServerMessage::TaskReleased(m) => Some(m.task),
                ServerMessage::ReservationExpired(m) => Some(m.task),
                _ => None,
            };
            if let Some(task) = ended {
                if self.holds.get(&task).is_some_and(|h| h.holder == holder) {
                    self.holds.remove(&task);
                    return vec![Effect::Changed];
                }
            }
            return Vec::new();
        }
        let mut effects = match message {
            ServerMessage::TaskReserved(m) => self.on_reserved(m),
            ServerMessage::TaskReleased(m) => self.on_released(m),
            ServerMessage::ReservationExpiring(m) => self.on_expiring(m),
            ServerMessage::ReservationExpired(m) => {
                self.hold_ended(m.task, &m.holder, Some(NoticeKind::Expired), true)
            }
            ServerMessage::TaskReleaseRequested(m) => self.on_requested(m),
            ServerMessage::TaskReleaseAnswered(m) => self.on_answered(m),
            ServerMessage::TaskReleaseRequestEnded(m) => self.on_request_ended(m),
            _ => Vec::new(),
        };
        effects.extend(self.marks_raised_if_dirty());
        effects
    }

    /// Whether `holder` may hold one more task here: at most
    /// [`MAX_RESERVATIONS_PER_MEMBER`] per member, and only as many members as the log lists.
    fn room_for(&self, holder: &Holder) -> bool {
        let members = self.verifier.state().map_or(0, |s| s.members().len());
        let of_member = self
            .holds
            .values()
            .filter(|h| h.holder.member == holder.member)
            .count();
        of_member < MAX_RESERVATIONS_PER_MEMBER
            && self.holds.len() < members.saturating_mul(MAX_RESERVATIONS_PER_MEMBER)
    }

    /// A name a note may carry: no control, invisible or reordering character (it could hide
    /// or reorder what the person reads next to it).
    fn plain_name(name: &str) -> bool {
        !name
            .chars()
            .any(|c| c.is_control() || crate::text::is_invisible(c))
    }

    /// Opens the holder's reservation note of `task`: signed by the holder for this task, not
    /// older than a note shown before, a title the team takes whose keyed hash under the note's
    /// epoch is the task's, and a plain name.
    fn open_note(&mut self, task: &KeyedHash, holder: &Holder, sealed: &Sealed) -> NoteOpen {
        let record = record_task_id(task);
        let opened = match self.open_record(
            RecordKind::ReservationNote,
            record.as_bytes(),
            holder,
            sealed,
            true,
        ) {
            Open::Opened(opened) => opened,
            Open::Later(_) => return NoteOpen::Later,
            Open::Fork => return NoteOpen::Fork,
            Open::Failed => return NoteOpen::Keep,
        };
        // The last counter of all could not be marked as shown once more when its hold ends.
        if opened.counter == u64::MAX {
            return NoteOpen::Keep;
        }
        let stamp = self.stamp(holder, &opened);
        match self.live.note(
            holder,
            &note_key(RecordKind::ReservationNote, task),
            stamp,
            &self.running_notes(holder),
        ) {
            Mark::Stale => return NoteOpen::Keep,
            Mark::Seen => {}
            Mark::Raised => self.note_marks_rose = true,
        }
        let Some(note) = team_tasks::parse_note(&opened.plaintext) else {
            return NoteOpen::Keep;
        };
        if !Self::plain_name(&note.name) || !self.title_matches(&note.title, task, sealed.epoch) {
            eprintln!("hangar: a team reservation note does not fit its task");
            return NoteOpen::Keep;
        }
        NoteOpen::Note(note)
    }

    /// Whether `title` is the task `task` names: its keyed hash under `epoch`'s key.
    fn title_matches(&self, title: &str, task: &KeyedHash, epoch: Epoch) -> bool {
        let (Some(state), Some(secret)) = (self.verifier.state(), self.keys.epochs.get(&epoch))
        else {
            return false;
        };
        let Ok(id) = TaskId::normalize(title) else {
            return false;
        };
        EpochCipher::for_epoch(state, secret, epoch)
            .is_ok_and(|cipher| cipher.match_key().task(&id) == *task)
    }

    fn on_reserved(&mut self, m: TaskReserved) -> Vec<Effect> {
        let mut effects = vec![Effect::Changed];
        let same = self
            .holds
            .get(&m.task)
            .is_some_and(|h| h.holder == m.holder);
        if !same && !self.room_for(&m.holder) {
            return Vec::new();
        }
        if !same {
            // Another holder took the task: the note of the one before stood for a hold that
            // ended.
            if let Some(before) = self.holds.get(&m.task).map(|h| h.holder.clone()) {
                let key = note_key(RecordKind::ReservationNote, &m.task);
                self.note_marks_rose |= self.live.consume_note(&before, &key);
            }
        }
        let mut hold = match self.holds.get(&m.task).filter(|_| same) {
            Some(hold) => hold.clone(),
            None => Hold {
                holder: m.holder.clone(),
                epoch: m.epoch,
                since: m.since,
                expires_at: None,
                expiring: false,
                unopened: None,
                note: None,
                request: None,
            },
        };
        hold.epoch = m.epoch;
        hold.since = m.since;
        hold.expires_at = m.expires_at;
        // A new deadline (activity, the holder back online) ends a warning.
        hold.expiring = false;
        if let Some(sealed) = &m.note {
            match self.open_note(&m.task, &m.holder, sealed) {
                NoteOpen::Note(note) => {
                    hold.note = Some(note);
                    hold.unopened = None;
                }
                NoteOpen::Later => hold.unopened = Some(sealed.clone()),
                NoteOpen::Fork => effects.extend(self.diverged(&m.holder)),
                NoteOpen::Keep => {}
            }
        }
        let has_note = hold.note.is_some();
        let note = hold.note.clone();
        self.holds.insert(m.task, hold);
        if self.me.as_ref() == Some(&m.holder) {
            match self.attempt_key(&m.task) {
                Some(key) => {
                    let mut renote = false;
                    if let Some(attempt) = self.attempts.get_mut(&key) {
                        attempt.blocked = None;
                        attempt.asked = false;
                        attempt.verified = true;
                        // A handover carries no note of this device's: it goes out again, so
                        // the team reads the title.
                        renote = !has_note && !attempt.renoted;
                        attempt.renoted |= renote;
                    }
                    if renote {
                        effects.extend(self.send_reserve(&key));
                    }
                }
                // Held from before a restart: its note says what it is.
                None => {
                    if let Some(note) = note {
                        if let Ok(id) = TaskId::normalize(&note.title) {
                            if self.attempts.len() < MAX_RESERVATIONS_PER_MEMBER {
                                self.attempts.insert(
                                    id.as_str().to_owned(),
                                    Attempt {
                                        title: note.title,
                                        session: None,
                                        repo: note.repo,
                                        area: note.area,
                                        hash: Some(m.task),
                                        blocked: None,
                                        asked: false,
                                        renoted: true,
                                        verified: true,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        } else if let Some(key) = self.attempt_key(&m.task) {
            if let Some(attempt) = self.attempts.get_mut(&key) {
                if attempt.blocked.as_ref() != Some(&m.holder) {
                    attempt.asked = false;
                }
                attempt.blocked = Some(m.holder.clone());
                attempt.verified = true;
            }
        }
        effects
    }

    fn on_released(&mut self, m: TaskReleased) -> Vec<Effect> {
        let notice =
            (m.reason == ReleaseReason::ForceReleased).then_some(NoticeKind::ForceReleased);
        // A handover is followed by the requester's `task.reserved`: nobody else may try first.
        let retry = m.reason != ReleaseReason::HandedOver;
        self.hold_ended(m.task, &m.holder, notice, retry)
    }

    /// A hold ended: released, expired or handed over. Its note counts as shown once more, so
    /// the relay cannot bring the hold back with it. `notice` goes to this person when a hold of
    /// theirs ended; with `retry`, this device's attempt that waited for the task tries again
    /// (first wins once more).
    fn hold_ended(
        &mut self,
        task: KeyedHash,
        holder: &Holder,
        notice: Option<NoticeKind>,
        retry: bool,
    ) -> Vec<Effect> {
        let removed = match self.holds.get(&task) {
            Some(hold) if hold.holder == *holder => self.holds.remove(&task),
            _ => None,
        };
        let Some(removed) = removed else {
            // No such hold here: nothing ended that this device showed.
            return Vec::new();
        };
        self.mark_ended(&task, &removed);
        let mut effects = vec![Effect::Changed];
        let attempt_key = self.attempt_key(&task);
        let title = removed
            .note
            .as_ref()
            .map(|n| n.title.clone())
            .or_else(|| {
                attempt_key
                    .as_ref()
                    .and_then(|k| self.attempts.get(k))
                    .map(|a| a.title.clone())
            })
            .unwrap_or_default();
        let session = attempt_key
            .as_ref()
            .and_then(|k| self.attempts.get(k))
            .and_then(|a| a.session);
        if let Some(kind) = notice {
            if self.member.as_ref() == Some(&holder.member) {
                self.notice(kind, title, None, None, session);
            }
        }
        if self.me.as_ref() == Some(holder) {
            if let Some(key) = attempt_key {
                self.attempts.remove(&key);
            }
            return effects;
        }
        if let Some(key) = attempt_key {
            let waiting = self
                .attempts
                .get(&key)
                .is_some_and(|a| a.blocked.as_ref() == Some(holder));
            if waiting && retry {
                if let Some(attempt) = self.attempts.get_mut(&key) {
                    attempt.blocked = None;
                    attempt.asked = false;
                    attempt.verified = false;
                }
                effects.extend(self.send_reserve(&key));
            }
        }
        effects
    }

    /// The keys of `device`'s notes that stand for something still running here: the
    /// reservation note of each of its holds, and the request note of each open request it
    /// signed. [`LiveMarks::note`] never drops their marks.
    fn running_notes(&self, device: &Holder) -> BTreeSet<String> {
        let mut keys = BTreeSet::new();
        // During a resync the holds from before come back one by one: theirs count until then.
        for (task, hold) in self.holds.iter().chain(&self.before_resync) {
            if hold.holder == *device {
                keys.insert(note_key(RecordKind::ReservationNote, task));
            }
            if hold.request.as_ref().and_then(|r| r.signer.as_ref()) == Some(device) {
                keys.insert(note_key(RecordKind::ReleaseRequestNote, task));
            }
        }
        keys
    }

    /// A hold ended (or vanished from a snapshot): its note, and the note of its open request,
    /// count as shown once more. A note that never opened (its key or log entry had not come)
    /// gets a mark from its header, so it cannot come back as new once the key arrives; the
    /// header is unverified, so its counter counts at most [`HEADER_COUNTER_SLACK_MS`] ahead of
    /// this device's clock.
    fn mark_ended(&mut self, task: &KeyedHash, hold: &Hold) {
        let key = note_key(RecordKind::ReservationNote, task);
        if !self.live.consume_note(&hold.holder, &key) {
            if let Some(stamp) = hold
                .unopened
                .as_ref()
                .and_then(|sealed| self.header_stamp(&hold.holder, sealed))
            {
                let ceiling = crate::team::counter_floor(
                    (self.now)().saturating_add(HEADER_COUNTER_SLACK_MS),
                );
                let next = Stamp {
                    counter: stamp.counter.min(ceiling).saturating_add(1),
                    ..stamp
                };
                self.note_marks_rose |= self.live.note_at_least(&hold.holder, &key, next);
            }
        } else {
            self.note_marks_rose = true;
        }
        if let Some(signer) = hold.request.as_ref().and_then(|r| r.signer.as_ref()) {
            let key = note_key(RecordKind::ReleaseRequestNote, task);
            self.note_marks_rose |= self.live.consume_note(signer, &key);
        }
    }

    /// The stamp a sealed record's header names (unverified: used only to raise a mark), with
    /// the key period the log gives the sender at that head. `None` when no period of the
    /// sender covers it: a mark without a period would count against every period.
    fn header_stamp(&self, sender: &Holder, sealed: &Sealed) -> Option<Stamp> {
        let (head, counter) = sealed_header(sealed)?;
        let period = self
            .verifier
            .state()?
            .key_periods(sender)
            .iter()
            .find(|p| p.covers(head.index))?
            .added_at;
        Some(Stamp { period, counter })
    }

    fn on_expiring(&mut self, m: ReservationExpiring) -> Vec<Effect> {
        let Some(hold) = self.holds.get_mut(&m.task).filter(|h| h.holder == m.holder) else {
            return Vec::new();
        };
        hold.expiring = true;
        hold.expires_at = Some(m.expires_at);
        let title = hold
            .note
            .as_ref()
            .map(|n| n.title.clone())
            .unwrap_or_default();
        if self.member.as_ref() == Some(&m.holder.member) {
            let session = self
                .attempt_key(&m.task)
                .and_then(|k| self.attempts.get(&k))
                .and_then(|a| a.session);
            self.notice(
                NoticeKind::Expiring,
                title,
                None,
                Some(m.expires_at),
                session,
            );
        }
        vec![Effect::Changed]
    }

    fn on_requested(&mut self, m: TaskReleaseRequested) -> Vec<Effect> {
        let Some(state) = self.verifier.state() else {
            return Vec::new();
        };
        // The requester must be a member the log lists; its name, and the holder's card, come
        // only from a note one of its listed devices signed for this task, not shown before.
        if state.member(&m.requester).is_none() {
            return Vec::new();
        }
        if !self
            .holds
            .get(&m.task)
            .is_some_and(|h| h.holder == m.holder)
        {
            return Vec::new();
        }
        let devices: Vec<Holder> = state
            .devices()
            .filter(|(h, _)| h.member == m.requester)
            .map(|(h, _)| h)
            .collect();
        let record = record_task_id(&m.task);
        let opened = m.note.as_ref().and_then(|sealed| {
            devices.iter().find_map(|device| {
                match self.open_sealed(
                    RecordKind::ReleaseRequestNote,
                    record.as_bytes(),
                    device,
                    sealed,
                ) {
                    Open::Opened(opened) => Some((device.clone(), opened)),
                    _ => None,
                }
            })
        });
        let mut signer = None;
        let mut name = String::new();
        if let Some((device, opened)) = opened {
            let stamp = self.stamp(&device, &opened);
            let key = note_key(RecordKind::ReleaseRequestNote, &m.task);
            let protected = self.running_notes(&device);
            let mark = self.live.note(&device, &key, stamp, &protected);
            if mark == Mark::Raised {
                self.note_marks_rose = true;
            }
            let parsed = team_tasks::parse_request(&opened.plaintext)
                .filter(|parsed| Self::plain_name(parsed));
            if let (true, Some(parsed)) = (mark != Mark::Stale, parsed) {
                signer = Some(device);
                name = parsed;
            }
        }
        let previous = self.holds.get(&m.task).and_then(|h| h.request.clone());
        // The same request again (the relay repeats it): an answer already sent stays sent.
        let answered = previous
            .as_ref()
            .is_some_and(|p| p.requester == m.requester && p.signer == signer && p.answered);
        // Another request replaces an open one: the note of that one is old from now on.
        if let Some(old) = previous
            .as_ref()
            .filter(|p| p.requester != m.requester || p.signer != signer)
            .and_then(|p| p.signer.clone())
        {
            let key = note_key(RecordKind::ReleaseRequestNote, &m.task);
            self.note_marks_rose |= self.live.consume_note(&old, &key);
        }
        let Some(hold) = self.holds.get_mut(&m.task) else {
            return Vec::new();
        };
        hold.request = Some(HoldRequest {
            requester: m.requester.clone(),
            signer,
            name,
            answered,
        });
        if self.member.as_ref() == Some(&m.requester) {
            if let Some(key) = self.attempt_key(&m.task) {
                if let Some(attempt) = self.attempts.get_mut(&key) {
                    attempt.asked = true;
                }
            }
        }
        vec![Effect::Changed]
    }

    /// The open request of a task ended; `kind`: the notice for the requester, given only when
    /// this device's attempt had asked for the task.
    fn request_ended(
        &mut self,
        task: KeyedHash,
        holder: &Holder,
        requester: &MemberId,
        kind: Option<NoticeKind>,
    ) -> Vec<Effect> {
        let mut signer = None;
        let title = match self.holds.get_mut(&task).filter(|h| h.holder == *holder) {
            Some(hold) => {
                signer = hold.request.take().and_then(|r| r.signer);
                hold.note.as_ref().map(|n| n.title.clone())
            }
            None => None,
        };
        // The request note stood for this request: shown again, it is old.
        if let Some(signer) = signer {
            let key = note_key(RecordKind::ReleaseRequestNote, &task);
            self.note_marks_rose |= self.live.consume_note(&signer, &key);
        }
        if self.member.as_ref() != Some(requester) {
            return vec![Effect::Changed];
        }
        let key = self.attempt_key(&task);
        let attempt = key.as_ref().and_then(|k| self.attempts.get_mut(k));
        let Some(attempt) = attempt.filter(|a| a.asked) else {
            return vec![Effect::Changed];
        };
        attempt.asked = false;
        let title = title.unwrap_or_else(|| attempt.title.clone());
        let session = attempt.session;
        if let Some(kind) = kind {
            self.notice(kind, title, Some(holder), None, session);
        }
        vec![Effect::Changed]
    }

    fn on_answered(&mut self, m: TaskReleaseAnswered) -> Vec<Effect> {
        let kind = if m.accepted {
            NoticeKind::ReleaseAccepted
        } else {
            NoticeKind::ReleaseDeclined
        };
        self.request_ended(m.task, &m.holder, &m.requester, Some(kind))
    }

    fn on_request_ended(&mut self, m: TaskReleaseRequestEnded) -> Vec<Effect> {
        let kind = (m.reason == RequestEndReason::TimedOut).then_some(NoticeKind::ReleaseTimedOut);
        self.request_ended(m.task, &m.holder, &m.requester, kind)
    }

    fn task_views(&self) -> Vec<TaskView> {
        let mut list: Vec<TaskView> = self
            .holds
            .iter()
            .map(|(task, hold)| TaskView {
                task: task.to_hex(),
                title: hold
                    .note
                    .as_ref()
                    .map(|n| n.title.clone())
                    .unwrap_or_default(),
                area: hold.note.as_ref().and_then(|n| n.area.clone()),
                member: hold.holder.member.as_str().to_owned(),
                device: hold.holder.device.as_str().to_owned(),
                name: hold
                    .note
                    .as_ref()
                    .map(|n| n.name.clone())
                    .unwrap_or_default(),
                mine: self.me.as_ref() == Some(&hold.holder),
                own: self.me.as_ref() != Some(&hold.holder)
                    && self.member.as_ref() == Some(&hold.holder.member),
                since: hold.since.0,
                expires_at: hold.expires_at.map(|at| at.0),
                expiring: hold.expiring,
                // Answered here: gone until the relay's word, so nobody answers twice.
                request: hold
                    .request
                    .as_ref()
                    .filter(|r| !r.answered)
                    .map(|r| RequestView {
                        member: r.requester.as_str().to_owned(),
                        name: r.name.clone(),
                        mine: self.member.as_ref() == Some(&r.requester),
                        confirmed: r.signer.is_some(),
                    }),
            })
            .collect();
        list.sort_by(|a, b| (a.since, &a.task).cmp(&(b.since, &b.task)));
        list
    }

    fn attempt_views(&self) -> Vec<AttemptView> {
        let me = self.me.as_ref();
        self.attempts
            .values()
            .filter(|a| {
                // Held by this device: the list of reservations shows it.
                !a.hash
                    .and_then(|hash| self.holds.get(&hash))
                    .is_some_and(|h| Some(&h.holder) == me)
            })
            .map(|a| {
                // A holder the log does not list yet is not shown: the attempt still waits.
                let blocked = a.blocked.as_ref().filter(|_| a.verified);
                AttemptView {
                    task: a.hash.map(|h| h.to_hex()).unwrap_or_default(),
                    title: a.title.clone(),
                    state: if blocked.is_some() {
                        AttemptState::Blocked
                    } else {
                        AttemptState::Reserving
                    },
                    member: blocked.map(|h| h.member.as_str().to_owned()),
                    device: blocked.map(|h| h.device.as_str().to_owned()),
                    name: blocked.map(|h| self.name_of(h)).unwrap_or_default(),
                    asked: a.asked,
                    session_id: a.session,
                }
            })
            .collect()
    }

    /// The reservations for the agent gate and the agent's context: only those whose note the
    /// holder signed, and this device's attempts that a listed teammate's hold blocks (they
    /// pause their own linked session).
    pub fn team_tasks(&self) -> Vec<TeamTask> {
        let mut tasks = Vec::new();
        for (task, hold) in &self.holds {
            let Some(note) = &hold.note else {
                continue;
            };
            let mine = self.member.as_ref() == Some(&hold.holder.member);
            let session = (self.me.as_ref() == Some(&hold.holder))
                .then(|| self.attempt_key(task))
                .flatten()
                .and_then(|key| self.attempts.get(&key))
                .and_then(|a| a.session);
            tasks.push(TeamTask {
                title: note.title.clone(),
                who: if mine {
                    String::new()
                } else {
                    team_share::person_tag(&note.name, hold.holder.member.as_str())
                },
                mine,
                repo: note.repo.clone(),
                area: note.area.clone(),
                session,
                wanted: false,
            });
        }
        for attempt in self.attempts.values().filter(|a| a.verified) {
            let Some(holder) = &attempt.blocked else {
                continue;
            };
            // Only while the hold that blocks it is known here: a conflict without a hold, or a
            // hold that ended, pauses nothing.
            let blocking = attempt
                .hash
                .and_then(|hash| self.holds.get(&hash))
                .is_some_and(|h| h.holder == *holder);
            if !blocking {
                continue;
            }
            tasks.push(TeamTask {
                title: attempt.title.clone(),
                who: team_share::person_tag(&self.name_of(holder), holder.member.as_str()),
                mine: false,
                repo: attempt.repo.clone(),
                area: attempt.area.clone(),
                session: attempt.session,
                wanted: true,
            });
        }
        tasks
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
    use hangar_team_protocol::reservation::{Intent, Policy, ReservationBook, ReservationEvent};
    use hangar_team_protocol::wire::{
        encode_server, Ack, ErrorBody, KeyRotated, MemberLogAppended, MemberLogEntry,
        ReservationExpired, Welcome,
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
        /// Reservations, decided as the real relay decides them.
        book: ReservationBook,
        /// The note of the newest `task.reserve` per task, with the device that sent it.
        notes: HashMap<KeyedHash, (Holder, Option<Sealed>)>,
        /// The note of the open release request per task.
        request_notes: HashMap<KeyedHash, Option<Sealed>>,
        /// The relay's clock, Unix milliseconds.
        clock: u64,
        /// Logged task events sent, in order.
        task_events: Vec<String>,
        /// Every `task.reserve` received.
        reserves: Vec<TaskReserve>,
        /// Log entries after this index are never handed to this device (a relay holding back
        /// part of the history).
        withhold: Option<(Holder, u64)>,
        /// `task.reserve` frames are recorded but never reach the book: lost with a connection.
        lose_reserves: bool,
        /// The protocol version the relay negotiates (1: a relay from before the notes key
        /// ring, which refuses its kinds).
        proto: u16,
        /// The team's notes key ring as stored.
        notes_record: Option<hangar_team_protocol::wire::NotesKeysRecord>,
        /// `notes.keys.*` frames received.
        notes_frames: usize,
        /// The next `notes.keys.set` meets another device's write first (`conflict`).
        notes_race: bool,
        /// Every `notes.keys.set` is refused.
        refuse_notes: bool,
        /// Every `key.rotate` is refused (no owner's or admin's rotation gets through).
        refuse_rotate: bool,
        /// The challenge of each connection's newest `welcome` (protocol version 3), by holder
        /// and connection number.
        challenges: HashMap<(Holder, u64), hangar_team_protocol::e2e::Challenge>,
        /// The signing key each connection proved (protocol version 3).
        proven: HashMap<(Holder, u64), PublicKey>,
        /// A removed device in its grace period, with the index of the entry that removed it.
        departing: Option<(Holder, u64)>,
        /// The departing connection's `device.prove` is answered with `forbidden` (a relay that
        /// does not follow the specification).
        departing_prove_err: bool,
        /// Kinds the departing connection sent.
        departing_frames: Vec<&'static str>,
        /// `device.prove` frames that passed.
        proofs: usize,
        /// Every `device.prove` is refused.
        refuse_proof: bool,
        /// Kinds refused because the sender did not count as a device of the log
        /// (`possession::check_restricted`), in arrival order.
        restricted: Vec<&'static str>,
        /// Kinds of a newer protocol version than the relay negotiated.
        too_new: Vec<&'static str>,
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
                book: ReservationBook::new(Policy::default(), Epoch(0)),
                notes: HashMap::new(),
                request_notes: HashMap::new(),
                clock: 10_000_000,
                task_events: Vec::new(),
                reserves: Vec::new(),
                withhold: None,
                lose_reserves: false,
                proto: 1,
                notes_record: None,
                notes_frames: 0,
                notes_race: false,
                refuse_notes: false,
                refuse_rotate: false,
                challenges: HashMap::new(),
                proven: HashMap::new(),
                departing: None,
                departing_prove_err: false,
                departing_frames: Vec::new(),
                proofs: 0,
                refuse_proof: false,
                restricted: Vec::new(),
                too_new: Vec::new(),
            }
        }

        /// A book event as the relay logs and sends it.
        fn task_message(&self, task: KeyedHash, event: ReservationEvent) -> ServerMessage {
            match event {
                ReservationEvent::Reserved {
                    holder,
                    epoch,
                    since,
                    expires_at,
                } => {
                    // The note goes along only with the hold of the device that sent it.
                    let note = self
                        .notes
                        .get(&task)
                        .filter(|(by, _)| *by == holder)
                        .and_then(|(_, note)| note.clone());
                    ServerMessage::TaskReserved(TaskReserved {
                        task,
                        holder,
                        epoch,
                        since,
                        expires_at,
                        note,
                    })
                }
                ReservationEvent::Released { holder, reason } => {
                    ServerMessage::TaskReleased(TaskReleased {
                        task,
                        holder,
                        reason,
                    })
                }
                ReservationEvent::Expiring { holder, expires_at } => {
                    ServerMessage::ReservationExpiring(ReservationExpiring {
                        task,
                        holder,
                        expires_at,
                    })
                }
                ReservationEvent::Expired { holder } => {
                    ServerMessage::ReservationExpired(ReservationExpired { task, holder })
                }
                ReservationEvent::ReleaseRequested { holder, requester } => {
                    ServerMessage::TaskReleaseRequested(TaskReleaseRequested {
                        task,
                        holder,
                        requester: requester.member,
                        note: self.request_notes.get(&task).cloned().flatten(),
                    })
                }
                ReservationEvent::ReleaseAnswered {
                    holder,
                    requester,
                    accepted,
                } => ServerMessage::TaskReleaseAnswered(TaskReleaseAnswered {
                    task,
                    holder,
                    requester: requester.member,
                    accepted,
                }),
                ReservationEvent::ReleaseRequestEnded {
                    holder,
                    requester,
                    reason,
                } => ServerMessage::TaskReleaseRequestEnded(TaskReleaseRequestEnded {
                    task,
                    holder,
                    requester: requester.member,
                    reason,
                }),
            }
        }

        /// Logs book events with the next sequence numbers.
        fn log_tasks(&mut self, events: Vec<(KeyedHash, ReservationEvent)>) -> Vec<String> {
            let mut texts = Vec::new();
            for (task, event) in events {
                let message = self.task_message(task, event);
                self.seq += 1;
                let text = self.text(None, Some(self.seq), message);
                self.task_events.push(text.clone());
                texts.push(text);
            }
            texts
        }

        /// The relay's clock moves on: warnings and expiry, for everyone.
        fn tick(&mut self, ms: u64) -> Vec<String> {
            self.clock += ms;
            let events = self.book.tick(UnixMillis(self.clock));
            self.log_tasks(events)
        }

        /// A device's last connection closed (its holds start to run) or opened again.
        fn presence_of(&mut self, holder: &Holder, online: bool) -> Vec<String> {
            let events = self.book.presence(holder, online, UnixMillis(self.clock));
            self.log_tasks(events)
        }

        fn apply_intent(
            &mut self,
            id: OpId,
            sender: &Holder,
            intent: Intent,
        ) -> (Vec<String>, Vec<String>) {
            let applied = self.book.apply(sender, intent, UnixMillis(self.clock));
            let (events, rejection) = applied.into_events();
            let all = self.log_tasks(events);
            let own = match rejection {
                None => vec![self.ack(id, None)],
                Some(rejection) => vec![self.text(
                    Some(id),
                    None,
                    ServerMessage::Err(ErrorBody {
                        code: rejection.code(),
                        retry_after_ms: rejection.retry_after_ms(UnixMillis(self.clock)),
                        holder: rejection.holder().cloned(),
                    }),
                )],
            };
            (own, all)
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
            self.handle_on(sender, role, 0, frame)
        }

        /// The connection of a removed device in its grace period, as the relay serves it: a
        /// `welcome` (with a challenge from version 3 on), the log up to the removing entry,
        /// `device.keys` and `device.prove` acknowledged without effect, everything else
        /// `forbidden`.
        fn handle_departing(&mut self, until: u64, frame: ClientFrame) -> Vec<String> {
            let id = frame.id;
            self.departing_frames.push(frame.message.kind());
            match frame.message {
                ClientMessage::Hello(_) => {
                    let challenge = (self.proto >= 3)
                        .then(|| hangar_team_protocol::e2e::Challenge::from_bytes([3; 32]));
                    let member = self.departing.as_ref().unwrap().0.member.clone();
                    vec![self.text(
                        Some(id),
                        None,
                        ServerMessage::Welcome(Welcome {
                            proto: self.proto,
                            stream: StreamId::new("s").unwrap(),
                            head: Seq(self.seq),
                            member,
                            epoch: Epoch(0),
                            challenge,
                        }),
                    )]
                }
                ClientMessage::DeviceProve(_) if self.departing_prove_err => {
                    vec![self.err(id, ErrorCode::Forbidden)]
                }
                ClientMessage::DeviceKeys(_) | ClientMessage::DeviceProve(_) => {
                    vec![self.ack(id, None)]
                }
                ClientMessage::MemberLogGet(get) => {
                    let entries = self
                        .entries
                        .iter()
                        .filter(|e| e.index > get.after && e.index <= until)
                        .cloned()
                        .collect::<Vec<_>>();
                    vec![self.text(
                        Some(id),
                        None,
                        ServerMessage::MemberLogEntries(MemberLogEntries::page(entries)),
                    )]
                }
                _ => vec![self.err(id, ErrorCode::Forbidden)],
            }
        }

        /// [`Relay::handle`] for connection `conn` of `sender`: challenges and proofs belong
        /// to one connection, never to the holder.
        fn handle_on(
            &mut self,
            sender: &Holder,
            role: Role,
            conn: u64,
            frame: ClientFrame,
        ) -> (Vec<String>, Vec<String>, Vec<(Holder, String)>) {
            if let Some((_, until)) = self.departing.as_ref().filter(|(h, _)| h == sender) {
                let until = *until;
                return (self.handle_departing(until, frame), Vec::new(), Vec::new());
            }
            let id = frame.id;
            let mut own = Vec::new();
            let mut all = Vec::new();
            let mut targeted = Vec::new();
            let connection = (sender.clone(), conn);
            // A kind of a newer version than the connection's is refused, as the relay does.
            if frame.message.since() > self.proto && frame.message.since() >= 3 {
                self.too_new.push(frame.message.kind());
                own.push(self.err(id, ErrorCode::BadRequest));
                return (own, all, targeted);
            }
            // The relay's rule before every intent (also for versions 1 and 2, where a device
            // the log lists counts without a proof).
            if !matches!(frame.message, ClientMessage::Hello(_)) {
                let possession = hangar_team_crypto::possession::Possession::of_connection(
                    self.proto,
                    self.proven.get(&connection),
                );
                let restricted = hangar_team_crypto::possession::check_restricted(
                    self.verifier.state(),
                    sender,
                    possession,
                    &frame.message,
                );
                if restricted.is_err() {
                    self.restricted.push(frame.message.kind());
                    own.push(self.err(id, ErrorCode::Forbidden));
                    return (own, all, targeted);
                }
            }
            match frame.message {
                ClientMessage::Hello(_) => {
                    // A new challenge per welcome; the connection starts unproven.
                    self.proven.remove(&connection);
                    let challenge = (self.proto >= 3).then(|| {
                        let mut bytes = [0u8; 32];
                        OsEntropy.fill(&mut bytes).unwrap();
                        let challenge = hangar_team_protocol::e2e::Challenge::from_bytes(bytes);
                        self.challenges.insert(connection.clone(), challenge);
                        challenge
                    });
                    own.push(self.text(
                        Some(id),
                        None,
                        ServerMessage::Welcome(Welcome {
                            proto: self.proto,
                            stream: StreamId::new("s").unwrap(),
                            head: Seq(self.seq),
                            member: sender.member.clone(),
                            epoch: Epoch(0),
                            challenge,
                        }),
                    ));
                    all.extend(self.presence_of(sender, true));
                }
                ClientMessage::DeviceProve(prove) => {
                    let checked = self.challenges.get(&connection).and_then(|challenge| {
                        hangar_team_crypto::possession::verify_connection_proof(
                            &self.team, sender, self.proto, challenge, &prove,
                        )
                        .ok()
                    });
                    match checked {
                        Some(key) if !self.refuse_proof => {
                            self.proofs += 1;
                            self.proven.insert(connection.clone(), key);
                            own.push(self.ack(id, None));
                        }
                        _ => own.push(self.err(id, ErrorCode::Forbidden)),
                    }
                }
                ClientMessage::NotesKeysGet | ClientMessage::NotesKeysSet(_) if self.proto < 2 => {
                    self.notes_frames += 1;
                    own.push(self.err(id, ErrorCode::BadRequest));
                }
                ClientMessage::NotesKeysGet => {
                    self.notes_frames += 1;
                    own.push(self.text(
                        Some(id),
                        None,
                        ServerMessage::NotesKeys(hangar_team_protocol::wire::NotesKeys {
                            record: self.notes_record.clone(),
                        }),
                    ));
                }
                ClientMessage::NotesKeysSet(set) => {
                    self.notes_frames += 1;
                    let state = self.verifier.state().unwrap();
                    let manager = state
                        .effective_role(&sender.member, role)
                        .is_some_and(|r| matches!(r, Role::Owner | Role::Admin));
                    let current = state.current_epoch().map(|e| e.epoch);
                    let version = self.notes_record.as_ref().map_or(0, |r| r.version);
                    if std::mem::take(&mut self.notes_race) {
                        if let Some(record) = &mut self.notes_record {
                            record.version += 1;
                        }
                        own.push(self.err(id, ErrorCode::Conflict));
                    } else if self.refuse_notes {
                        own.push(self.err(id, ErrorCode::BadRequest));
                    } else if !manager {
                        own.push(self.err(id, ErrorCode::Forbidden));
                    } else if Some(set.content.epoch) != current {
                        own.push(self.err(id, ErrorCode::StaleEpoch));
                    } else if set.base != version {
                        own.push(self.err(id, ErrorCode::Conflict));
                    } else {
                        self.notes_record = Some(hangar_team_protocol::wire::NotesKeysRecord {
                            version: version + 1,
                            by: sender.clone(),
                            content: set.content,
                        });
                        own.push(self.ack(id, None));
                        all.push(self.text(
                            None,
                            None,
                            ServerMessage::NotesKeysUpdated(
                                hangar_team_protocol::wire::NotesKeysUpdated {
                                    version: version + 1,
                                    by: sender.clone(),
                                },
                            ),
                        ));
                    }
                }
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
                    let hidden = self
                        .withhold
                        .as_ref()
                        .filter(|(from, _)| from == sender)
                        .map_or(u64::MAX, |(_, after)| *after);
                    let entries = self
                        .entries
                        .iter()
                        .filter(|e| e.index > get.after && e.index <= hidden)
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
                ClientMessage::DeviceRemove(remove) => {
                    match self.verifier.admit_device_remove(&remove, sender, role) {
                        Ok(admitted) => {
                            self.store(&remove.log, admitted.head, sender);
                            own.push(self.ack(id, None));
                            all.push(self.appended(admitted.head, sender));
                        }
                        Err(error) => own.push(self.err(id, error.code())),
                    }
                }
                ClientMessage::KeyRotate(_) if self.refuse_rotate => {
                    own.push(self.err(id, ErrorCode::BadRequest));
                }
                ClientMessage::KeyRotate(rotate) => {
                    match self.verifier.admit_rotation(&rotate, sender, role) {
                        Ok(admitted) => {
                            self.store(&rotate.log, admitted.head, sender);
                            let events = self
                                .book
                                .rotate(rotate.epoch, UnixMillis(self.clock))
                                .unwrap();
                            let rotated = self.log_tasks(events);
                            for wrapped in &rotate.wrapped {
                                self.wrapped.push((
                                    wrapped.holder.clone(),
                                    rotate.epoch,
                                    sender.clone(),
                                    wrapped.data.clone(),
                                ));
                            }
                            own.push(self.ack(id, None));
                            all.extend(rotated);
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
                ClientMessage::TaskReserve(_) if self.lose_reserves => {}
                ClientMessage::TaskReserve(reserve) => {
                    self.reserves.push(reserve.clone());
                    let note = (sender.clone(), reserve.note.clone());
                    let intent = Intent::Reserve {
                        task: reserve.task,
                        epoch: reserve.epoch,
                        prev: reserve.prev,
                    };
                    // The note goes with the hold only when the reserve is applied.
                    let before = self.notes.insert(reserve.task, note);
                    let (mine, events) = self.apply_intent(id, sender, intent);
                    let applied = mine.iter().any(|t| t.contains("\"ack\""));
                    if !applied {
                        match before {
                            Some(before) => self.notes.insert(reserve.task, before),
                            None => self.notes.remove(&reserve.task),
                        };
                    }
                    own.extend(mine);
                    all.extend(events);
                }
                ClientMessage::TaskRelease(task) => {
                    let (mine, events) =
                        self.apply_intent(id, sender, Intent::Release { task: task.task });
                    own.extend(mine);
                    all.extend(events);
                }
                ClientMessage::Heartbeat(beat) => {
                    let (mine, events) =
                        self.apply_intent(id, sender, Intent::Heartbeat { tasks: beat.tasks });
                    own.extend(mine);
                    all.extend(events);
                }
                ClientMessage::TaskReleaseRequest(request) => {
                    let before = self.request_notes.insert(request.task, request.note);
                    let (mine, events) = self.apply_intent(
                        id,
                        sender,
                        Intent::RequestRelease { task: request.task },
                    );
                    if !mine.iter().any(|t| t.contains("\"ack\"")) {
                        match before {
                            Some(before) => self.request_notes.insert(request.task, before),
                            None => self.request_notes.remove(&request.task),
                        };
                    }
                    own.extend(mine);
                    all.extend(events);
                }
                ClientMessage::TaskReleaseWithdraw(task) => {
                    let (mine, events) =
                        self.apply_intent(id, sender, Intent::WithdrawRelease { task: task.task });
                    own.extend(mine);
                    all.extend(events);
                }
                ClientMessage::TaskReleaseAnswer(answer) => {
                    let (mine, events) = self.apply_intent(
                        id,
                        sender,
                        Intent::AnswerRelease {
                            task: answer.task,
                            requester: answer.requester,
                            accept: answer.accept,
                        },
                    );
                    own.extend(mine);
                    all.extend(events);
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
        /// Every end in order (`ended` keeps only the last).
        ends: Vec<SessionEnd>,
        /// Fails the engine's draw after this many more, once.
        fail_draw: Arc<Mutex<Option<usize>>>,
        /// Confirms its admission once the log lists it, as the person would after comparing
        /// the safety numbers ([`run`]).
        admit: bool,
        /// The website checks it asked for (`removal`, `members`).
        checks: Vec<(bool, bool)>,
        /// The engine's clock (Unix milliseconds).
        clock: Arc<std::sync::atomic::AtomicU64>,
    }

    fn client(member: &str, device: &str, role: Role) -> Client {
        let identity = DeviceIdentity::generate(&mut OsEntropy).unwrap();
        let fail_draw = Arc::new(Mutex::new(None));
        let clock = Arc::new(std::sync::atomic::AtomicU64::new(1_000));
        let read = Arc::clone(&clock);
        let mut engine = Engine::new(EngineSetup {
            project: "p1".to_owned(),
            team: team(),
            device: DeviceId::new(device).unwrap(),
            identity,
            member: None,
            account: Some(MemberId::new(member).unwrap()),
            website_role: role,
            // Every test team is alice's.
            owner: Some(MemberId::new("alice").unwrap()),
            keys: StoredKeys::default(),
            local: LocalState::default(),
            now: Box::new(move || read.load(std::sync::atomic::Ordering::SeqCst)),
            entropy: Box::new(Scripted(Arc::clone(&fail_draw))),
            website_every_ms: WEBSITE_CHECK_EVERY_MS,
            members_every_ms: MEMBERS_CHECK_EVERY_MS,
        });
        // The website lists every test member, as read before the team began (`at: 0`): new
        // members can be admitted, and the list is too old to remove anyone. Tests about the
        // list replace it with `set_website` or clear it.
        engine.roster = Some(Roster {
            members: ["alice", "bob", "carol", "mallory"]
                .into_iter()
                .map(|m| {
                    let role = if m == "alice" {
                        Role::Owner
                    } else {
                        Role::Member
                    };
                    (MemberId::new(m).unwrap(), Some(role))
                })
                .collect(),
            at: 0,
        });
        Client {
            engine,
            holder: holder(member, device),
            role,
            inbox: Vec::new(),
            stored_keys: 0,
            ended: None,
            ends: Vec::new(),
            fail_draw,
            admit: true,
            checks: Vec::new(),
            clock,
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
                        Effect::End(end) => {
                            clients[at].ended = Some(end);
                            clients[at].ends.push(end);
                        }
                        Effect::CheckWebsite { removal, members } => {
                            clients[at].checks.push((removal, members));
                        }
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
                for (at, client) in clients.iter_mut().enumerate() {
                    if let TeamStatus::ConfirmAdmission {
                        log_index,
                        safety_number,
                        ..
                    } = client.engine.status(false)
                    {
                        if client.admit {
                            let effects = client
                                .engine
                                .confirm_admission(log_index, &safety_number)
                                .unwrap();
                            start.push((at, effects));
                        }
                    }
                }
                if start.is_empty() {
                    return;
                }
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
                account: None,
                website_role: Role::Owner,
                owner: None,
                keys: StoredKeys::default(),
                local: LocalState {
                    counter: stored,
                    ..LocalState::default()
                },
                now: Box::new(|| 1_000),
                entropy: Box::new(OsEntropy),
                website_every_ms: WEBSITE_CHECK_EVERY_MS,
                members_every_ms: MEMBERS_CHECK_EVERY_MS,
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
        team_of_two_on(1)
    }

    /// [`team_of_two`] on a relay that negotiates `proto`.
    fn team_of_two_on(proto: u16) -> (Relay, Client, Client) {
        let mut relay = Relay::new();
        relay.proto = proto;
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

    fn sends(effects: &[Effect]) -> Vec<&'static str> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Send(frame) => Some(frame.message.kind()),
                _ => None,
            })
            .collect()
    }

    fn website_checks(effects: &[Effect]) -> Vec<(bool, bool)> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::CheckWebsite { removal, members } => Some((*removal, *members)),
                _ => None,
            })
            .collect()
    }

    /// The relay's `welcome` of `proto` for `client`, with a challenge from version 3 on.
    fn welcome_text(
        client: &Client,
        proto: u16,
        challenge: Option<hangar_team_protocol::e2e::Challenge>,
        op: &OpId,
    ) -> String {
        encode_server(&ServerFrame {
            id: Some(op.clone()),
            seq: None,
            team: Some(team()),
            message: ServerMessage::Welcome(Welcome {
                proto,
                stream: StreamId::new("s").unwrap(),
                head: Seq(0),
                member: client.holder.member.clone(),
                epoch: Epoch(0),
                challenge,
            }),
        })
        .unwrap()
    }

    fn hello_op(effects: &[Effect]) -> OpId {
        effects
            .iter()
            .find_map(|e| match e {
                Effect::Send(frame) if matches!(frame.message, ClientMessage::Hello(_)) => {
                    Some(frame.id.clone())
                }
                _ => None,
            })
            .unwrap()
    }

    /// Protocol version 3: the device answers the `welcome`'s challenge right after
    /// registering its keys and before it reads the log, so nothing it sends later meets a
    /// relay that still counts it as unproven. The proof answers exactly this `hello`.
    #[test]
    fn a_welcome_of_version_three_is_answered_with_the_proof_before_the_log_fetch() {
        let mut a1 = client("alice", "a1", Role::Owner);
        let hello = a1.engine.hello(ticket());
        let op = hello_op(&hello);
        assert!(matches!(
            &hello[..],
            [.., Effect::Send(ClientFrame { message: ClientMessage::Hello(h), .. }), _]
                if h.proto == ProtoRange::CURRENT && h.proto.max() == 3
        ));
        let challenge = hangar_team_protocol::e2e::Challenge::from_bytes([7; 32]);
        let effects = a1
            .engine
            .on_text(&welcome_text(&a1, 3, Some(challenge), &op));
        assert_eq!(
            sends(&effects),
            ["device.keys", "device.prove", "memberlog.get"]
        );
        let prove = effects
            .iter()
            .find_map(|e| match e {
                Effect::Send(ClientFrame {
                    message: ClientMessage::DeviceProve(prove),
                    ..
                }) => Some(prove.clone()),
                _ => None,
            })
            .unwrap();
        let key = hangar_team_crypto::possession::verify_connection_proof(
            &team(),
            &a1.holder,
            3,
            &challenge,
            &prove,
        );
        assert_eq!(key, Ok(a1.engine.identity.public_keys().sign));
        // Another challenge (a replay on the next connection) or version does not verify.
        let other = hangar_team_protocol::e2e::Challenge::from_bytes([8; 32]);
        assert!(hangar_team_crypto::possession::verify_connection_proof(
            &team(),
            &a1.holder,
            3,
            &other,
            &prove
        )
        .is_err());
    }

    /// A relay of version 2 (or 1) sends no challenge and gets no proof: the device falls back
    /// to what it sent before version 3.
    #[test]
    fn a_welcome_before_version_three_gets_no_proof() {
        for proto in [1, 2] {
            let mut a1 = client("alice", "a1", Role::Owner);
            let hello = a1.engine.hello(ticket());
            let op = hello_op(&hello);
            let effects = a1.engine.on_text(&welcome_text(&a1, proto, None, &op));
            assert_eq!(sends(&effects), ["device.keys", "memberlog.get"], "{proto}");
        }
    }

    /// End to end on a relay of version 3 that applies the restriction: both devices prove
    /// themselves after every `welcome`, the team forms and shares as on version 1, and the
    /// relay refuses nothing. On versions 1 and 2 the relay restricts only devices the log does
    /// not list, and the newcomer waiting for its admission never sends anything else.
    #[test]
    fn devices_prove_themselves_after_every_welcome_and_the_relay_refuses_nothing() {
        for proto in [1, 2, 3] {
            let (mut relay, mut a1, mut b1) = team_of_two_on(proto);
            assert!(
                relay.restricted.is_empty(),
                "{proto}: {:?}",
                relay.restricted
            );
            assert!(relay.too_new.is_empty(), "{proto}: {:?}", relay.too_new);
            let proofs = if proto >= 3 { 2 } else { 0 };
            assert_eq!(relay.proofs, proofs, "{proto}");
            // A new connection proves again against its new challenge.
            let hello = b1.engine.hello(ticket());
            run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, hello)]);
            assert_eq!(connected(&b1), (1, 2, 2));
            assert_eq!(relay.proofs, if proto >= 3 { 3 } else { 0 }, "{proto}");
            assert!(
                relay.restricted.is_empty(),
                "{proto}: {:?}",
                relay.restricted
            );
        }
    }

    /// The relay refusing the proof leaves the connection unable to act; the device ends it
    /// (and reconnects with backoff) instead of sending intents the relay refuses one by one.
    #[test]
    fn a_refused_proof_ends_the_connection() {
        let mut relay = Relay::new();
        relay.proto = 3;
        relay.refuse_proof = true;
        let mut a1 = client("alice", "a1", Role::Owner);
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1], vec![(0, hello)]);
        assert_eq!(a1.ends, [SessionEnd::Lost]);
        assert_eq!(relay.proofs, 0);
        // The connection ends once the log fetch answered, before anything acts on it: no
        // team is created on a connection the relay does not count.
        assert!(relay.entries.is_empty());
    }

    /// Bob's device is removed while it is offline, then reconnects to a relay of version 3,
    /// which serves it as a departing device: a `welcome` with a challenge, then only the log
    /// up to the removal. The device learns from the log that it was removed, also when the
    /// relay answers its proof with `forbidden` instead of the acknowledgement the
    /// specification asks for.
    fn removed_device_reconnects(prove_err: bool) -> (Relay, Client) {
        let (mut relay, mut a1, mut b1) = team_of_two_on(3);
        let before = relay.entries.len() as u64;
        let effects = a1.engine.remove_member(&b1.holder.member).unwrap();
        run(&mut relay, &mut [&mut a1], vec![(0, effects)]);
        assert!(relay.entries.len() as u64 > before);
        relay.departing = Some((b1.holder.clone(), before + 1));
        relay.departing_prove_err = prove_err;
        // The setup delivered the team's events to Bob before his first welcome too.
        b1.ends.clear();
        let hello = b1.engine.hello(ticket());
        run(&mut relay, &mut [&mut b1], vec![(0, hello)]);
        (relay, b1)
    }

    #[test]
    fn a_removed_device_on_version_three_reads_its_removal_from_the_departing_log() {
        for prove_err in [false, true] {
            let (relay, b1) = removed_device_reconnects(prove_err);
            assert_eq!(
                b1.ends.first(),
                Some(&SessionEnd::Stop(StopReason::Removed)),
                "{prove_err}: {:?}",
                b1.ends
            );
            assert_eq!(
                relay.departing_frames,
                ["hello", "device.keys", "device.prove", "memberlog.get"],
                "{prove_err}"
            );
        }
    }

    /// A stolen ticket with Bob's device id opens a second connection: it proves with a key of
    /// its own, which the log does not list, and gets nothing; Bob's proven connection keeps
    /// its proof. Challenges and proofs belong to a connection, not to the holder.
    #[test]
    fn a_second_connection_of_the_same_holder_does_not_inherit_a_proof() {
        let (mut relay, _a1, b1) = team_of_two_on(3);
        let frame = |message| ClientFrame {
            id: op_id(),
            team: Some(team()),
            message,
        };
        let presence = || {
            ClientMessage::PresenceSet(PresenceSet {
                state: PresenceState::Active,
                status: None,
            })
        };
        let hello = ClientMessage::Hello(Hello {
            ticket: ticket(),
            device: b1.holder.device.clone(),
            proto: ProtoRange::CURRENT,
            last_seq: None,
        });
        let (own, _, _) = relay.handle_on(&b1.holder, Role::Member, 1, frame(hello));
        assert!(own[0].contains("\"challenge\""), "{}", own[0]);
        let (own, _, _) = relay.handle_on(&b1.holder, Role::Member, 1, frame(presence()));
        assert!(own[0].contains("\"forbidden\""), "{}", own[0]);
        let impostor = DeviceIdentity::generate(&mut OsEntropy).unwrap();
        let challenge = relay.challenges[&(b1.holder.clone(), 1)];
        let prove = impostor.prove_connection(&team(), &b1.holder, 3, &challenge);
        let (own, _, _) = relay.handle_on(
            &b1.holder,
            Role::Member,
            1,
            frame(ClientMessage::DeviceProve(prove)),
        );
        assert!(own[0].contains("\"ack\""), "a valid signature: {}", own[0]);
        let (own, _, _) = relay.handle_on(&b1.holder, Role::Member, 1, frame(presence()));
        assert!(own[0].contains("\"forbidden\""), "{}", own[0]);
        let (own, _, _) = relay.handle_on(&b1.holder, Role::Member, 0, frame(presence()));
        assert!(own[0].contains("\"ack\""), "{}", own[0]);
    }

    #[test]
    fn a_newcomer_shares_nothing_before_the_person_confirms_who_admitted_it() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let mut b1 = client("bob", "b1", Role::Member);
        b1.admit = false;
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, hello)]);
        assert!(
            a1.engine.keys().admitted,
            "the team's creator admitted itself"
        );
        let hello = b1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, hello)]);
        let poll = a1.engine.poll_devices();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, poll)]);
        let bob = a1.engine.pending().into_iter().next().unwrap();
        let confirm = a1.engine.confirm(&bob.id).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, confirm)]);
        let TeamStatus::ConfirmAdmission {
            member,
            device,
            safety_number,
            log_index,
            log_safety_number,
        } = b1.engine.status(false)
        else {
            panic!("not asking: {:?}", b1.engine.status(false));
        };
        // The admitting device shows the same numbers for itself and the log.
        assert_eq!((member.as_str(), device.as_str()), ("alice", "a1"));
        let view = a1.engine.activity();
        let own = view.devices.iter().find(|d| d.this).unwrap();
        assert_eq!(own.safety_number, safety_number);
        let log = view.log.unwrap();
        assert_eq!(
            (log.index, log.safety_number),
            (log_index, log_safety_number)
        );
        assert!(!b1.engine.keys().admitted);
        // Nothing of bob's goes out: no presence, no claims, no reservation, no removal.
        let shared = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("src/a.rs", 1, 2)]));
        assert!(sends(&shared).is_empty(), "{:?}", sends(&shared));
        assert!(matches!(
            b1.engine.reserve_task("Fix login", None, None),
            Err(TeamError::Busy)
        ));
        assert!(matches!(
            b1.engine.remove_own_device(&DeviceId::new("b9").unwrap()),
            Err(TeamError::UnknownRequest)
        ));
        assert!(a1.engine.activity().peers.is_empty());
        // A confirmation of numbers the window showed before the team moved on changes nothing.
        assert_eq!(
            b1.engine
                .confirm_admission(log_index + 1, &safety_number)
                .err(),
            Some(TeamError::Changed)
        );
        let mut other = safety_number.clone();
        other[0] = "00000".to_owned();
        assert_eq!(
            b1.engine.confirm_admission(log_index, &other).err(),
            Some(TeamError::Changed)
        );
        assert!(!b1.engine.keys().admitted);
        // Confirmed: stored with the head first, then bob's device takes the key and shares.
        let confirmed = b1
            .engine
            .confirm_admission(log_index, &safety_number)
            .unwrap();
        assert!(matches!(confirmed[0], Effect::StoreKeys));
        assert!(b1.engine.keys().admitted);
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, confirmed)]);
        assert_eq!(connected(&b1), (1, 2, 2));
        assert!(a1.engine.activity().peers.iter().any(|p| p.member == "bob"));
        assert!(matches!(
            b1.engine.confirm_admission(log_index, &safety_number),
            Err(TeamError::UnknownRequest)
        ));
    }

    #[test]
    fn a_log_whose_owner_is_not_the_websites_stops_the_team() {
        let mut relay = Relay::new();
        // The relay lets someone else create the project's team.
        let mut mallory = client("mallory", "m1", Role::Owner);
        let hello = mallory.engine.hello(ticket());
        run(&mut relay, &mut [&mut mallory], vec![(0, hello)]);
        let mut b1 = client("bob", "b1", Role::Member);
        let _ = b1.engine.set_website(Website {
            role: Role::Member,
            owner: Some(MemberId::new("alice").unwrap()),
            members: None,
        });
        let hello = b1.engine.hello(ticket());
        run(&mut relay, &mut [&mut mallory, &mut b1], vec![(1, hello)]);
        assert_eq!(
            b1.ended,
            Some(SessionEnd::Stop(StopReason::Attention(
                AttentionReason::OwnerMismatch
            )))
        );
    }

    #[test]
    fn the_first_welcome_must_name_the_signed_in_account() {
        let mut relay = Relay::new();
        let mut b1 = client("bob", "b1", Role::Member);
        // The website says carol is signed in; the relay's welcome names bob.
        b1.engine.set_account(Some(MemberId::new("carol").unwrap()));
        let hello = b1.engine.hello(ticket());
        run(&mut relay, &mut [&mut b1], vec![(0, hello)]);
        assert_eq!(
            b1.ended,
            Some(SessionEnd::Stop(StopReason::Attention(
                AttentionReason::OtherAccount
            )))
        );
        assert!(
            b1.engine.member().is_none(),
            "not bound to the relay's choice"
        );
        assert!(relay.devices.is_empty(), "no keys registered");
    }

    #[test]
    fn relay_events_ask_the_website_at_most_once_per_interval() {
        let (relay, _a1, mut b1) = team_of_two();
        let removed = relay.text(
            None,
            None,
            ServerMessage::MemberRemoved(hangar_team_protocol::wire::MemberRemoved {
                member: MemberId::new("bob").unwrap(),
            }),
        );
        let mut checks = Vec::new();
        for _ in 0..100 {
            checks.extend(website_checks(&b1.engine.on_text(&removed)));
        }
        assert_eq!(checks, vec![(true, true)]);
        // The next one waits for a ping after the interval, with the removal remembered.
        assert!(website_checks(&b1.engine.ping()).is_empty());
        b1.clock
            .fetch_add(WEBSITE_CHECK_EVERY_MS, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(website_checks(&b1.engine.ping()), vec![(true, true)]);
        assert!(website_checks(&b1.engine.ping()).is_empty());
    }

    #[test]
    fn a_member_list_without_this_member_or_the_owner_removes_nobody() {
        let (_relay, mut a1, _b1) = team_of_two();
        let alice = MemberId::new("alice").unwrap();
        let lists = [
            Vec::new(),
            vec![WebsiteMember {
                member: MemberId::new("carol").unwrap(),
                role: Some(Role::Member),
            }],
        ];
        for list in lists {
            a1.clock
                .fetch_add(MEMBERS_CHECK_EVERY_MS, std::sync::atomic::Ordering::SeqCst);
            assert_eq!(website_checks(&a1.engine.ping()), vec![(false, true)]);
            let effects = a1.engine.set_website(Website {
                role: Role::Owner,
                owner: Some(alice.clone()),
                members: Some(list),
            });
            assert!(sends(&effects).is_empty(), "{:?}", sends(&effects));
        }
        assert_eq!(connected(&a1), (1, 2, 2));
    }

    #[test]
    fn a_member_the_website_no_longer_lists_leaves_the_log_and_the_next_key() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        // The owner's device asks for the member list (here at its ping).
        let ping = a1.engine.ping();
        assert_eq!(website_checks(&ping), vec![(false, true)]);
        let alice = MemberId::new("alice").unwrap();
        let bob = MemberId::new("bob").unwrap();
        let effects = a1.engine.set_website(Website {
            role: Role::Owner,
            owner: Some(alice.clone()),
            members: Some(vec![WebsiteMember {
                member: alice,
                role: Some(Role::Owner),
            }]),
        });
        let append = effects
            .iter()
            .find_map(|e| match e {
                Effect::Send(frame) => match &frame.message {
                    ClientMessage::MemberLogAppend(append) => Some(append.clone()),
                    _ => None,
                },
                _ => None,
            })
            .expect("a member log entry");
        let body = EntryBody::decode_unverified(&append.entry).unwrap();
        assert_eq!(
            body.kind,
            EntryKind::RemoveMember {
                member: bob.clone()
            }
        );
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        // After the relay took it and the log was fetched: a rotation that leaves bob out.
        assert_eq!(connected(&a1), (2, 1, 1));
        let wrapped: Vec<&Holder> = relay
            .wrapped
            .iter()
            .filter(|w| w.1 == Epoch(2))
            .map(|w| &w.0)
            .collect();
        assert!(!wrapped.is_empty());
        assert!(wrapped.iter().all(|h| h.member != bob), "{wrapped:?}");
        assert_eq!(b1.ended, Some(SessionEnd::Stop(StopReason::Removed)));
    }

    #[test]
    fn a_member_added_after_the_list_was_asked_for_stays_and_roles_follow_the_website() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        // The list was asked for before bob's device was confirmed: it does not name bob yet.
        a1.engine.roster_asked_at = Some(2);
        let alice = MemberId::new("alice").unwrap();
        let effects = a1.engine.set_website(Website {
            role: Role::Owner,
            owner: Some(alice.clone()),
            members: Some(vec![WebsiteMember {
                member: alice.clone(),
                role: Some(Role::Owner),
            }]),
        });
        assert!(sends(&effects).is_empty(), "{:?}", sends(&effects));
        // The website makes bob an admin: the owner's device asks the owner first.
        let bob_id = MemberId::new("bob").unwrap();
        let list = |bob_role: Role| Website {
            role: Role::Owner,
            owner: Some(alice.clone()),
            members: Some(vec![
                WebsiteMember {
                    member: alice.clone(),
                    role: Some(Role::Owner),
                },
                WebsiteMember {
                    member: bob_id.clone(),
                    role: Some(bob_role),
                },
            ]),
        };
        let bob_role = |a1: &Client| a1.engine.state().unwrap().member(&bob_id).unwrap().role;
        assert_eq!(website_checks(&a1.engine.ping()), vec![(false, true)]);
        a1.engine.roster_asked_at = Some(3);
        let effects = a1.engine.set_website(list(Role::Admin));
        assert!(sends(&effects).is_empty(), "{:?}", sends(&effects));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        assert_eq!(bob_role(&a1), Role::Member);
        let pending = a1.engine.pending();
        assert_eq!(pending.len(), 1);
        let promotion = pending[0].clone();
        assert_eq!(promotion.kind, PendingKind::RoleChange);
        assert_eq!(promotion.member, "bob");
        // Confirmed: the change role entry goes into the log.
        let confirm = a1.engine.confirm(&promotion.id).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, confirm)]);
        assert_eq!(bob_role(&a1), Role::Admin);
        assert!(a1.engine.pending().is_empty());
        assert_eq!(
            connected(&a1),
            (1, 2, 2),
            "a role change makes no rotation due"
        );
        // The website takes the admin role back: fewer rights follow at once, unasked.
        a1.engine.roster_asked_at = Some(4);
        let effects = a1.engine.set_website(list(Role::Member));
        assert_eq!(sends(&effects), vec!["memberlog.append"]);
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        assert_eq!(bob_role(&a1), Role::Member);
        assert!(a1.engine.pending().is_empty());
        // Made an admin again and rejected: the request stays away, the log keeps the role.
        a1.engine.roster_asked_at = Some(5);
        let effects = a1.engine.set_website(list(Role::Admin));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        let again = a1.engine.pending();
        assert_eq!(again.len(), 1);
        let rejected = a1.engine.reject(&again[0].id).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, rejected)]);
        a1.engine.roster_asked_at = Some(5);
        let effects = a1.engine.set_website(list(Role::Admin));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        assert!(a1.engine.pending().is_empty());
        assert_eq!(bob_role(&a1), Role::Member);
    }

    #[test]
    fn after_a_removal_a_member_device_seals_nothing_until_the_key_changes() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let mut b1 = client("bob", "b1", Role::Member);
        let mut b2 = client("bob", "b2", Role::Member);
        let hello = a1.engine.hello(ticket());
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut b2],
            vec![(0, hello)],
        );
        let hello = b1.engine.hello(ticket());
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut b2],
            vec![(1, hello)],
        );
        let poll = a1.engine.poll_devices();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut b2],
            vec![(0, poll)],
        );
        let pending = a1.engine.pending().into_iter().next().unwrap();
        let confirm = a1.engine.confirm(&pending.id).unwrap();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut b2],
            vec![(0, confirm)],
        );
        let hello = b2.engine.hello(ticket());
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut b2],
            vec![(2, hello)],
        );
        let poll = b1.engine.poll_devices();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut b2],
            vec![(1, poll)],
        );
        let pending = b1.engine.pending().into_iter().next().unwrap();
        let confirm = b1.engine.confirm(&pending.id).unwrap();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut b2],
            vec![(1, confirm)],
        );
        assert_eq!(connected(&b2), (1, 2, 3));
        // A key change after which bob's second device holds the epoch's key from the rotation.
        let me = a1.holder.clone();
        let rotate = a1.engine.rotate(&me);
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut b2],
            vec![(0, rotate)],
        );
        assert_eq!(connected(&b2), (2, 2, 3));
        // Bob's second device is lost; his first removes it while no owner or admin is around.
        let own = b1.engine.activity().devices;
        assert_eq!(own.len(), 2);
        assert!(own[0].this && !own[1].this);
        let remove = b1
            .engine
            .remove_own_device(&DeviceId::new("b2").unwrap())
            .unwrap();
        run(&mut relay, &mut [&mut b1, &mut b2], vec![(0, remove)]);
        assert_eq!(b2.ended, Some(SessionEnd::Stop(StopReason::Removed)));
        assert_eq!(
            b1.engine.status(false),
            TeamStatus::WaitingForKeyChange {
                reason: KeyChangeWait::Removal
            }
        );
        let shared = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("src/a.rs", 1, 2)]));
        assert!(sends(&shared).is_empty(), "{:?}", sends(&shared));
        assert!(matches!(
            b1.engine.reserve_task("Fix login", None, None),
            Err(TeamError::Busy)
        ));
        // The owner's device comes back and rotates: bob's device shares again.
        let claims = relay.claim_sets;
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, hello)]);
        assert_eq!(connected(&b1), (3, 2, 2));
        assert!(relay.claim_sets > claims);
    }

    fn listing(members: &[(&str, Role)]) -> Vec<WebsiteMember> {
        members
            .iter()
            .map(|(member, role)| WebsiteMember {
                member: MemberId::new(*member).unwrap(),
                role: Some(*role),
            })
            .collect()
    }

    fn website(role: Role, members: Option<Vec<WebsiteMember>>) -> Website {
        Website {
            role,
            owner: Some(MemberId::new("alice").unwrap()),
            members,
        }
    }

    fn advance(client: &Client, ms: u64) {
        client
            .clock
            .fetch_add(ms, std::sync::atomic::Ordering::SeqCst);
    }

    fn appended_kind(effects: &[Effect]) -> EntryKind {
        effects
            .iter()
            .find_map(|e| match e {
                Effect::Send(ClientFrame {
                    message: ClientMessage::MemberLogAppend(append),
                    ..
                }) => Some(EntryBody::decode_unverified(&append.entry).unwrap().kind),
                _ => None,
            })
            .expect("a member log entry")
    }

    #[test]
    fn a_new_member_the_website_lists_as_admin_joins_as_member_and_waits_for_the_promotion() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let mut b1 = client("bob", "b1", Role::Member);
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, hello)]);
        // The website lists bob as an admin before his first device asks to join.
        a1.engine.roster_asked_at = Some(a1.engine.state().unwrap().head().index);
        let list = listing(&[("alice", Role::Owner), ("bob", Role::Admin)]);
        let effects = a1.engine.set_website(website(Role::Owner, Some(list)));
        assert!(sends(&effects).is_empty(), "{:?}", sends(&effects));
        let hello = b1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, hello)]);
        let poll = a1.engine.poll_devices();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, poll)]);
        let pending = a1.engine.pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, PendingKind::NewMember);
        // Confirming the device admits bob as a member, whatever the website says.
        let confirm = a1.engine.confirm(&pending[0].id).unwrap();
        assert!(matches!(
            appended_kind(&confirm),
            EntryKind::AddMember {
                role: Role::Member,
                ..
            }
        ));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, confirm)]);
        let bob = MemberId::new("bob").unwrap();
        let bob_role = |c: &Client| c.engine.state().unwrap().member(&bob).unwrap().role;
        assert_eq!(bob_role(&a1), Role::Member);
        assert_eq!(bob_role(&b1), Role::Member);
        // The admin role is a confirmation of its own.
        let pending = a1.engine.pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, PendingKind::RoleChange);
        assert_eq!(pending[0].member, "bob");
        let confirm = a1.engine.confirm(&pending[0].id).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, confirm)]);
        assert_eq!(bob_role(&a1), Role::Admin);
        assert!(a1.engine.pending().is_empty());
    }

    #[test]
    fn only_people_the_website_lists_are_offered_for_admission() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let mut b1 = client("bob", "b1", Role::Member);
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, hello)]);
        a1.engine.roster_asked_at = Some(a1.engine.state().unwrap().head().index);
        let alone = listing(&[("alice", Role::Owner)]);
        let effects = a1.engine.set_website(website(Role::Owner, Some(alone)));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        let hello = b1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, hello)]);
        // The list was just read, for an event and regularly.
        let now = a1.clock.load(std::sync::atomic::Ordering::SeqCst);
        a1.engine.website_checked_at = Some(now);
        a1.engine.members_checked_at = Some(now);
        let poll = a1.engine.poll_devices();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, poll)]);
        // The relay names bob's device; the website does not name bob.
        assert!(a1.engine.pending().is_empty());
        // That makes the owner's device read the list again at the next ping after the
        // interval, not only with the regular member check.
        advance(&a1, WEBSITE_CHECK_EVERY_MS);
        assert_eq!(website_checks(&a1.engine.ping()), vec![(false, true)]);
        let both = listing(&[("alice", Role::Owner), ("bob", Role::Member)]);
        let effects = a1.engine.set_website(website(Role::Owner, Some(both)));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        let poll = a1.engine.poll_devices();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, poll)]);
        let pending = a1.engine.pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, PendingKind::NewMember);
        // The website drops bob again before anyone confirmed him: the request goes with the
        // new list, not only with the next device poll.
        a1.engine.roster_asked_at = Some(a1.engine.state().unwrap().head().index);
        let alone = listing(&[("alice", Role::Owner)]);
        let effects = a1.engine.set_website(website(Role::Owner, Some(alone)));
        assert!(a1.engine.pending().is_empty());
        assert_eq!(
            a1.engine.confirm(&pending[0].id).err(),
            Some(TeamError::UnknownRequest)
        );
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        assert!(a1.engine.pending().is_empty());
    }

    #[test]
    fn before_the_first_member_list_nobody_new_is_offered_for_admission() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let mut b1 = client("bob", "b1", Role::Member);
        // Just started: the website's list was not read yet.
        a1.engine.roster = None;
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, hello)]);
        let hello = b1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, hello)]);
        let checks = a1.checks.len();
        let poll = a1.engine.poll_devices();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, poll)]);
        // The relay alone names bob's device: nobody is offered, and the list is read now.
        assert!(a1.engine.pending().is_empty());
        assert_eq!(a1.checks[checks..], [(false, true)]);
        // The list arrives and names bob: his device is offered with the next poll.
        a1.engine.roster_asked_at = Some(a1.engine.state().unwrap().head().index);
        let both = listing(&[("alice", Role::Owner), ("bob", Role::Member)]);
        let effects = a1.engine.set_website(website(Role::Owner, Some(both)));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        let poll = a1.engine.poll_devices();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, poll)]);
        let pending = a1.engine.pending();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].kind, PendingKind::NewMember);
        assert_eq!(pending[0].member, "bob");
    }

    #[test]
    fn a_refused_member_entry_keeps_the_list_for_admissions() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let mut c1 = client("carol", "c1", Role::Member);
        let mut m1 = client("mallory", "m1", Role::Member);
        for at in [2, 3] {
            let mut clients = [&mut a1, &mut b1, &mut c1, &mut m1];
            let hello = clients[at].engine.hello(ticket());
            run(&mut relay, &mut clients, vec![(at, hello)]);
        }
        // The website removed bob and lists carol; mallory is nobody's teammate there.
        let bob = MemberId::new("bob").unwrap();
        a1.engine.roster_asked_at = Some(a1.engine.state().unwrap().head().index);
        let list = listing(&[("alice", Role::Owner), ("carol", Role::Member)]);
        let effects = a1.engine.set_website(website(Role::Owner, Some(list)));
        assert_eq!(
            appended_kind(&effects),
            EntryKind::RemoveMember {
                member: bob.clone()
            }
        );
        let id = effects
            .iter()
            .find_map(|e| match e {
                Effect::Send(ClientFrame {
                    id,
                    message: ClientMessage::MemberLogAppend(_),
                    ..
                }) => Some(id.clone()),
                _ => None,
            })
            .unwrap();
        // The relay refuses the removal.
        let refused = relay.text(
            Some(id),
            None,
            ServerMessage::Err(ErrorBody {
                code: ErrorCode::Forbidden,
                retry_after_ms: None,
                holder: None,
            }),
        );
        let after = a1.engine.on_text(&refused);
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1, &mut m1],
            vec![(0, after)],
        );
        // Not sent again before the next read of the list.
        assert!(a1.engine.state().unwrap().member(&bob).is_some());
        // The list read last still decides who is offered: carol, never mallory.
        let poll = a1.engine.poll_devices();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1, &mut m1],
            vec![(0, poll)],
        );
        let pending = a1.engine.pending();
        assert_eq!(pending.len(), 1, "{pending:?}");
        assert_eq!(pending[0].kind, PendingKind::NewMember);
        assert_eq!(pending[0].member, "carol");
        // The next read of the list tries the removal once more.
        a1.engine.roster_asked_at = Some(a1.engine.state().unwrap().head().index);
        let list = listing(&[("alice", Role::Owner), ("carol", Role::Member)]);
        let effects = a1.engine.set_website(website(Role::Owner, Some(list)));
        assert_eq!(
            appended_kind(&effects),
            EntryKind::RemoveMember { member: bob }
        );
    }

    /// Alice, bob and carol; the website removes carol, alice's device writes the removal and
    /// the rotation, and the relay hands neither to bob's device. Returns bob's log head.
    fn withheld_removal() -> (Relay, Client, Client, u64) {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let mut c1 = carol_joins(&mut relay, &mut a1, &mut b1);
        assert_eq!(connected(&b1), (1, 3, 3));
        let head = b1.engine.state().unwrap().head().index;
        relay.withhold = Some((b1.holder.clone(), head));
        a1.engine.roster_asked_at = Some(head);
        let list = listing(&[("alice", Role::Owner), ("bob", Role::Member)]);
        let effects = a1.engine.set_website(website(Role::Owner, Some(list)));
        assert!(matches!(
            appended_kind(&effects),
            EntryKind::RemoveMember { .. }
        ));
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(0, effects)],
        );
        assert_eq!(connected(&a1), (2, 2, 2));
        assert_eq!(connected(&b1), (1, 3, 3), "bob's device never saw it");
        (relay, a1, b1, head)
    }

    #[test]
    fn an_admission_without_safety_numbers_cannot_be_confirmed() {
        let (_relay, _a1, mut b1) = team_of_two();
        let head = b1.engine.state().unwrap().head().index;
        b1.engine.keys.admitted = false;
        for (number, log) in [
            (Vec::new(), vec!["00001".to_owned()]),
            (vec!["00001".to_owned()], Vec::new()),
        ] {
            b1.engine.status = TeamStatus::ConfirmAdmission {
                member: String::new(),
                device: String::new(),
                safety_number: number.clone(),
                log_index: head,
                log_safety_number: log,
            };
            assert_eq!(
                b1.engine.confirm_admission(head, &number).err(),
                Some(TeamError::UnknownRequest)
            );
            assert!(!b1.engine.keys.admitted);
        }
    }

    #[test]
    fn a_member_who_left_a_log_without_room_for_the_removal_shows_the_full_log() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let mut c1 = carol_joins(&mut relay, &mut a1, &mut b1);
        use hangar_team_crypto::memberlog::RESERVED_LOG_ENTRIES;
        let max = b1.engine.state().unwrap().head().index + 1 + RESERVED_LOG_ENTRIES;
        relay.verifier = LogVerifier::with_max_entries(team(), max);
        relay.verifier.apply_all(&relay.entries).unwrap();
        for client in [&mut a1, &mut b1, &mut c1] {
            client.engine.max_log_entries = max;
            let mut verifier = LogVerifier::with_max_entries(team(), max);
            verifier.apply_all(&relay.entries).unwrap();
            client.engine.verifier = verifier;
        }
        // Key changes fill the log up to one place before the end: a rotation fits there, a
        // removal and its rotation do not.
        let me = a1.holder.clone();
        while a1.engine.state().unwrap().head().index < max - 1 {
            let rotate = a1.engine.rotate(&me);
            run(
                &mut relay,
                &mut [&mut a1, &mut b1, &mut c1],
                vec![(0, rotate)],
            );
        }
        let head = b1.engine.state().unwrap().head().index;
        assert_eq!(head, max - 1);
        // Carol leaves the project on the website; nobody can take her out of the team.
        let list = listing(&[("alice", Role::Owner), ("bob", Role::Member)]);
        for client in [&mut a1, &mut b1] {
            client.engine.roster_asked_at = Some(head);
            let role = client.role;
            let effects = client.engine.set_website(website(role, Some(list.clone())));
            assert!(
                !sends(&effects).contains(&"memberlog.append"),
                "{:?}",
                sends(&effects)
            );
        }
        advance(&b1, DEPARTURE_GRACE_MS - 1);
        b1.engine.ping();
        connected(&b1);
        // After the grace period both devices say the history is full instead of waiting for a
        // removal that cannot come.
        for client in [&mut a1, &mut b1] {
            advance(client, DEPARTURE_GRACE_MS);
            client.engine.ping();
            assert_eq!(
                client.engine.status(false),
                TeamStatus::WaitingForKeyChange {
                    reason: KeyChangeWait::LogFull
                },
                "{}",
                client.holder.device.as_str()
            );
        }
    }

    #[test]
    fn a_removal_the_relay_withholds_stops_member_devices_after_the_grace_period() {
        let (mut relay, _a1, mut b1, head) = withheld_removal();
        // Bob's device reads the website's list as well, on connecting and every interval.
        b1.engine.roster_asked_at = Some(head);
        let list = listing(&[("alice", Role::Owner), ("bob", Role::Member)]);
        let effects = b1.engine.set_website(website(Role::Member, Some(list)));
        assert!(sends(&effects).is_empty(), "{:?}", sends(&effects));
        assert_eq!(connected(&b1).0, 1);
        advance(&b1, DEPARTURE_GRACE_MS - 1);
        b1.engine.ping();
        assert_eq!(connected(&b1).0, 1, "within the grace period");
        advance(&b1, 1);
        b1.engine.ping();
        assert_eq!(
            b1.engine.status(false),
            TeamStatus::WaitingForKeyChange {
                reason: KeyChangeWait::MemberLeft
            }
        );
        let shared = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("src/a.rs", 1, 2)]));
        assert!(sends(&shared).is_empty(), "{:?}", sends(&shared));
        // The relay hands the history over: the new key opens and sharing goes on.
        relay.withhold = None;
        let sync = b1.engine.sync_log();
        run(&mut relay, &mut [&mut b1], vec![(0, sync)]);
        assert_eq!(connected(&b1), (2, 2, 2));
    }

    #[test]
    fn without_the_websites_member_list_a_member_device_goes_on() {
        let (_relay, _a1, mut b1, _head) = withheld_removal();
        // The website answers without a member list (an older server answers 404): absence
        // is no removal.
        let effects = b1.engine.set_website(website(Role::Member, None));
        assert!(sends(&effects).is_empty(), "{:?}", sends(&effects));
        for _ in 0..3 {
            advance(&b1, DEPARTURE_GRACE_MS);
            let checks = website_checks(&b1.engine.ping());
            assert_eq!(checks, vec![(false, true)], "it keeps asking");
            b1.engine.set_website(website(Role::Member, None));
        }
        assert_eq!(connected(&b1), (1, 3, 3));
    }

    #[test]
    fn a_deferred_rotation_no_owner_or_admin_makes_stops_member_devices_after_its_interval() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        let mut b1 = client("bob", "b1", Role::Member);
        let mut b2 = client("bob", "b2", Role::Member);
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, hello)]);
        let hello = b1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, hello)]);
        let poll = a1.engine.poll_devices();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, poll)]);
        let pending = a1.engine.pending().into_iter().next().unwrap();
        let confirm = a1.engine.confirm(&pending.id).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, confirm)]);
        // Bob adds a second device after the key was handed out, then removes it while no
        // owner or admin is online: due, not urgent.
        let hello = b2.engine.hello(ticket());
        run(&mut relay, &mut [&mut b1, &mut b2], vec![(1, hello)]);
        let poll = b1.engine.poll_devices();
        run(&mut relay, &mut [&mut b1, &mut b2], vec![(0, poll)]);
        let pending = b1.engine.pending().into_iter().next().unwrap();
        let confirm = b1.engine.confirm(&pending.id).unwrap();
        run(&mut relay, &mut [&mut b1, &mut b2], vec![(0, confirm)]);
        assert_eq!(connected(&b2), (1, 2, 3));
        let remove = b1
            .engine
            .remove_own_device(&DeviceId::new("b2").unwrap())
            .unwrap();
        run(&mut relay, &mut [&mut b1, &mut b2], vec![(0, remove)]);
        let state = b1.engine.state().unwrap();
        assert!(state.rotation_due() && !state.rotation_urgent());
        assert_eq!(connected(&b1).0, 1);
        advance(&b1, DEFERRED_ROTATION_EVERY_MS - 1);
        b1.engine.ping();
        assert_eq!(connected(&b1).0, 1, "within the interval sharing goes on");
        advance(&b1, 1);
        b1.engine.ping();
        assert_eq!(
            b1.engine.status(false),
            TeamStatus::WaitingForKeyChange {
                reason: KeyChangeWait::Overdue
            }
        );
        let shared = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("src/a.rs", 1, 2)]));
        assert!(sends(&shared).is_empty(), "{:?}", sends(&shared));
        // The owner's device comes back and changes the key: bob's device shares again.
        advance(&a1, DEFERRED_ROTATION_EVERY_MS);
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, hello)]);
        assert_eq!(connected(&b1), (2, 2, 2));
    }

    #[test]
    fn another_members_own_device_churn_never_makes_a_member_device_overdue() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let mut c1 = carol_joins(&mut relay, &mut a1, &mut b1);
        let epoch = connected(&c1).0;
        // With the owner's device offline, bob adds a device of his after the key was handed
        // out and removes it again: due, not urgent.
        let mut b2 = client("bob", "b2", Role::Member);
        let hello = b2.engine.hello(ticket());
        run(
            &mut relay,
            &mut [&mut b1, &mut c1, &mut b2],
            vec![(2, hello)],
        );
        let poll = b1.engine.poll_devices();
        run(
            &mut relay,
            &mut [&mut b1, &mut c1, &mut b2],
            vec![(0, poll)],
        );
        let pending = b1.engine.pending().into_iter().next().unwrap();
        let confirm = b1.engine.confirm(&pending.id).unwrap();
        run(
            &mut relay,
            &mut [&mut b1, &mut c1, &mut b2],
            vec![(0, confirm)],
        );
        let remove = b1
            .engine
            .remove_own_device(&DeviceId::new("b2").unwrap())
            .unwrap();
        run(&mut relay, &mut [&mut b1, &mut c1], vec![(0, remove)]);
        let state = c1.engine.state().unwrap();
        assert!(state.rotation_due() && !state.rotation_urgent());
        for _ in 0..3 {
            advance(&c1, DEFERRED_ROTATION_EVERY_MS);
            c1.engine.ping();
            assert_eq!(connected(&c1).0, epoch, "carol's device keeps sharing");
        }
        // Bob's own device waits for the key change: the removed device was his.
        advance(&b1, DEFERRED_ROTATION_EVERY_MS);
        b1.engine.ping();
        assert_eq!(
            b1.engine.status(false),
            TeamStatus::WaitingForKeyChange {
                reason: KeyChangeWait::Overdue
            }
        );
    }

    #[test]
    fn a_signed_newer_head_the_relay_does_not_hand_over_shows_held_back_history() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let head = b1.engine.state().unwrap().head().index;
        relay.withhold = Some((b1.holder.clone(), head));
        let _c1 = carol_joins(&mut relay, &mut a1, &mut b1);
        assert!(a1.engine.state().unwrap().head().index > head);
        // Fetches without a signed newer head say nothing.
        assert!(!b1.engine.activity().history_withheld);
        let effects = b1.engine.on_text(&honest_status(&relay, &mut a1));
        assert!(log_get(&effects).is_some(), "{effects:?}");
        run(&mut relay, &mut [&mut b1], vec![(0, effects)]);
        assert!(b1.engine.activity().history_withheld);
        assert_eq!(connected(&b1).0, 1, "a notice, no stop");
        // Once the relay hands the entries over, the notice goes.
        relay.withhold = None;
        let sync = b1.engine.sync_log();
        run(&mut relay, &mut [&mut b1], vec![(0, sync)]);
        assert!(!b1.engine.activity().history_withheld);
    }

    #[test]
    fn a_full_log_is_reported_and_its_requests_stay_visible_as_full() {
        use hangar_team_crypto::memberlog::RESERVED_LOG_ENTRIES;
        let (mut relay, mut a1, mut b1) = team_of_two();
        // `team_of_two` hands bob's device alice's first broadcasts before its own hello, which
        // ends that not yet started connection with `Gap` (the test relay sends to every client;
        // a real one sends nothing before `welcome`). Cleared, so every end below is this test's.
        assert_eq!(b1.ended, Some(SessionEnd::Gap));
        a1.ended = None;
        b1.ended = None;
        assert!(!a1.engine.activity().log_full);
        // Room for one more device entry: then only removals and key changes fit.
        let max = relay.entries.len() as u64 + 1 + RESERVED_LOG_ENTRIES;
        relay.verifier = LogVerifier::with_max_entries(team(), max);
        relay.verifier.apply_all(&relay.entries).unwrap();
        // The same limit on both devices, as if their connections had started with it.
        for client in [&mut a1, &mut b1] {
            client.engine.max_log_entries = max;
            let mut verifier = LogVerifier::with_max_entries(team(), max);
            verifier.apply_all(&relay.entries).unwrap();
            client.engine.verifier = verifier;
        }
        assert!(!b1.engine.activity().log_full);
        // Bob adds a device of his: the last place goes.
        let device = holder("bob", "x0");
        let identity = DeviceIdentity::generate(&mut OsEntropy).unwrap();
        let keys = identity.device_keys(&team(), &device);
        relay.devices.push(DeviceInfo {
            holder: device,
            sign: keys.sign,
            kem: keys.kem,
            proof: keys.proof,
            at: UnixMillis(1),
        });
        let poll = b1.engine.poll_devices();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, poll)]);
        let request = b1.engine.pending().into_iter().next().unwrap();
        assert!(!request.log_full);
        let confirm = b1.engine.confirm(&request.id).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, confirm)]);
        // Sharing goes on; every device reports the full log, without stopping.
        for client in [&a1, &b1] {
            assert!(client.engine.activity().log_full);
            assert_eq!(client.ended, None);
        }
        assert_eq!(connected(&b1).0, connected(&a1).0);
        // A new device asks to join: its request shows as full instead of vanishing, and
        // confirming it says why.
        let extra = holder("bob", "x1");
        let identity = DeviceIdentity::generate(&mut OsEntropy).unwrap();
        let keys = identity.device_keys(&team(), &extra);
        relay.devices.push(DeviceInfo {
            holder: extra,
            sign: keys.sign,
            kem: keys.kem,
            proof: keys.proof,
            at: UnixMillis(1),
        });
        let poll = b1.engine.poll_devices();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, poll)]);
        let pending = b1.engine.pending();
        assert_eq!(pending.len(), 1, "{pending:?}");
        assert!(pending[0].log_full);
        assert_eq!(
            b1.engine.confirm(&pending[0].id).err(),
            Some(TeamError::LogFull)
        );
        // A stolen device of bob's still goes: removals take the reserved places.
        let stolen = DeviceId::new("x0").unwrap();
        let remove = b1
            .engine
            .remove_own_device(&stolen)
            .expect("a full log still takes a device removal");
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, remove)]);
        assert!(b1
            .engine
            .state()
            .unwrap()
            .member(&MemberId::new("bob").unwrap())
            .is_some_and(|bob| !bob.devices.contains_key(&stolen)));
        assert_eq!(b1.ended, None);
        // Removing a member and the key change still fit.
        let remove = a1
            .engine
            .remove_member(&MemberId::new("bob").unwrap())
            .unwrap();
        let epoch = connected(&a1).0;
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, remove)]);
        assert_eq!(connected(&a1).0, epoch + 1);
        assert_eq!(a1.ended, None);
        assert_eq!(b1.ended, Some(SessionEnd::Stop(StopReason::Removed)));
    }

    /// Bob's first device admits another device of his (`id`, registered with the relay but
    /// never connecting, which keeps the test fast), wraps it the key and removes it again.
    fn churn(relay: &mut Relay, a1: &mut Client, b1: &mut Client, id: &str) {
        let extra = holder("bob", id);
        let identity = DeviceIdentity::generate(&mut OsEntropy).unwrap();
        let keys = identity.device_keys(&team(), &extra);
        relay.devices.push(DeviceInfo {
            holder: extra.clone(),
            sign: keys.sign,
            kem: keys.kem,
            proof: keys.proof,
            at: UnixMillis(1),
        });
        let wraps = relay.wraps;
        let poll = b1.engine.poll_devices();
        run(relay, &mut [&mut *a1, &mut *b1], vec![(1, poll)]);
        let request = b1.engine.pending().into_iter().next().unwrap();
        let confirm = b1.engine.confirm(&request.id).unwrap();
        run(relay, &mut [&mut *a1, &mut *b1], vec![(1, confirm)]);
        assert_eq!(relay.wraps, wraps + 1, "the new device got the key");
        let remove = b1.engine.remove_own_device(&extra.device).unwrap();
        run(relay, &mut [a1, b1], vec![(1, remove)]);
    }

    fn epoch_of(client: &Client) -> u32 {
        client
            .engine
            .state()
            .and_then(TeamState::current_epoch)
            .map_or(0, |e| e.epoch.0)
    }

    #[test]
    fn a_member_adding_and_removing_own_devices_forces_at_most_one_rotation_per_interval() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        assert_eq!(epoch_of(&a1), 1);
        for i in 0..50 {
            churn(&mut relay, &mut a1, &mut b1, &format!("x{i}"));
            // Sharing goes on: the removed devices came after the epoch, from bob himself.
            assert_eq!(connected(&b1).0, 1);
            assert_eq!(connected(&a1).0, 1);
        }
        assert!(a1.engine.state().unwrap().rotation_due());
        assert_eq!(epoch_of(&a1), 1, "no rotation within the interval");
        // The interval passed: the next ping rotates once, and more churn waits again.
        a1.clock.fetch_add(
            DEFERRED_ROTATION_EVERY_MS,
            std::sync::atomic::Ordering::SeqCst,
        );
        let ping = a1.engine.ping();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, ping)]);
        assert_eq!(epoch_of(&a1), 2);
        assert_eq!(connected(&b1).0, 2);
        for i in 50..55 {
            churn(&mut relay, &mut a1, &mut b1, &format!("x{i}"));
            let ping = a1.engine.ping();
            run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, ping)]);
        }
        assert_eq!(epoch_of(&a1), 2);
        // Removing a member still rotates at once.
        let remove = a1
            .engine
            .remove_member(&MemberId::new("bob").unwrap())
            .unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, remove)]);
        assert_eq!(epoch_of(&a1), 3);
    }

    #[test]
    fn a_relay_refusing_every_rotation_gets_at_most_two_per_connection() {
        let (mut relay, mut a1, b1) = team_of_two();
        let remove = a1
            .engine
            .remove_member(&MemberId::new("bob").unwrap())
            .unwrap();
        // The relay takes the removal but answers every `key.rotate` with `forbidden`.
        let mut rotations = 0;
        let mut queue = vec![remove];
        for round in 0..6 {
            while let Some(effects) = queue.pop() {
                for effect in effects {
                    let Effect::Send(frame) = effect else {
                        continue;
                    };
                    let answers = if matches!(frame.message, ClientMessage::KeyRotate(_)) {
                        rotations += 1;
                        vec![relay.err(frame.id, ErrorCode::Forbidden)]
                    } else {
                        let (mut own, all, _) = relay.handle(&a1.holder, Role::Owner, frame);
                        own.extend(all);
                        own
                    };
                    for text in answers {
                        queue.push(a1.engine.on_text(&text));
                    }
                }
            }
            assert!(rotations <= 2, "round {round}: {rotations} rotations");
            queue.push(a1.engine.ping());
        }
        assert_eq!(rotations, 2);
        assert_eq!(
            a1.engine.status(false),
            TeamStatus::WaitingForKeyChange {
                reason: KeyChangeWait::Removal
            }
        );
        drop(b1);
    }

    #[test]
    fn a_device_binds_to_no_account_before_the_website_names_the_signed_in_one() {
        let mut relay = Relay::new();
        let mut a1 = client("alice", "a1", Role::Owner);
        a1.engine.set_account(None);
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1], vec![(0, hello)]);
        assert_eq!(a1.ended, Some(SessionEnd::Lost));
        assert!(a1.engine.member().is_none());
        assert!(relay.entries.is_empty(), "nothing was created");
        // Once the account is known, the next connection binds and creates the team.
        a1.ended = None;
        a1.engine.set_account(Some(MemberId::new("alice").unwrap()));
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1], vec![(0, hello)]);
        assert_eq!(a1.engine.member().map(MemberId::as_str), Some("alice"));
        assert_eq!(connected(&a1).0, 1);
    }

    #[test]
    fn without_a_website_owner_only_devices_admitted_before_go_on() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        // An older server names no owner: bob's admitted device and alice's go on.
        for client in [&mut a1, &mut b1] {
            client.engine.owner = None;
        }
        let hello = b1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, hello)]);
        assert_eq!(connected(&b1).0, 1);
        let hello = a1.engine.hello(ticket());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, hello)]);
        assert_eq!(connected(&a1).0, 1);
        // A new device of carol's does not start trusting a log nothing vouches for.
        let mut c1 = client("carol", "c1", Role::Member);
        c1.engine.owner = None;
        let hello = c1.engine.hello(ticket());
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(2, hello)],
        );
        assert_eq!(
            c1.ended,
            Some(SessionEnd::Stop(StopReason::Attention(
                AttentionReason::NoWebsiteOwner
            )))
        );
    }

    #[test]
    fn a_release_request_without_the_key_leaves_no_attempt_behind() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        let task = b1.engine.activity().tasks[0].task.clone();
        assert!(b1.engine.attempts.is_empty());
        b1.engine.status = TeamStatus::WaitingForKeyChange {
            reason: KeyChangeWait::Removal,
        };
        assert_eq!(b1.engine.ask_release(&task).err(), Some(TeamError::Busy));
        assert!(
            b1.engine.attempts.is_empty(),
            "no attempt for a request never sent"
        );
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
        // The window matches a teammate's session to its own by the repository's root commit.
        assert_eq!(seen.peers[0].sessions[0].repo, "a".repeat(40));
        assert_eq!(
            seen.session_repos,
            [SessionRepo {
                session_id: 7,
                repo: "a".repeat(40)
            }]
        );
        let json = serde_json::to_value(&seen).unwrap();
        assert_eq!(json["peers"][0]["sessions"][0]["repo"], "a".repeat(40));
        assert_eq!(json["sessionRepos"][0]["sessionId"], 7);

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
    fn overlaps_per_session_are_bounded_per_teammate_device_and_need_a_status_for_details() {
        let (relay, mut a1, mut b1) = team_of_two();
        let a = a1
            .engine
            .set_share(share("Alice", 1, "Claude Code", &[("src/auth.rs", 10, 12)]));
        let repo = claim_set(&a).repo;
        let mine = a1.engine.session_keys.get(&1).unwrap().clone();
        // Bob's real session with its signed detail, but no status of his device reached alice.
        let b = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("src/auth.rs", 10, 12)]));
        let theirs = claim_set(&b);
        assert!(theirs.detail.is_some());
        a1.engine.peers.clear();
        let overlap = |session: SessionKey, detail: Option<Sealed>| {
            relay.text(
                None,
                None,
                ServerMessage::ClaimOverlap(ClaimOverlap {
                    repo,
                    session: mine.clone(),
                    other: SessionRef {
                        holder: b1.holder.clone(),
                        session,
                    },
                    level: MatchLevel::SameFile,
                    file: None,
                    files: 1,
                    detail,
                }),
            )
        };
        let _ = a1
            .engine
            .on_text(&overlap(theirs.session.clone(), theirs.detail.clone()));
        for n in 0..200 {
            let text = overlap(SessionKey::new(format!("s{n}")).unwrap(), None);
            let _ = a1.engine.on_text(&text);
        }
        assert_eq!(a1.engine.overlaps.len(), 1);
        // One device fills at most its own share, never the room of the other devices.
        assert_eq!(a1.engine.overlaps[&1].len(), MAX_OVERLAPS_PER_DEVICE);
        // Without a signed status listing the session only the level is kept, no detail.
        assert!(a1.engine.overlaps[&1].values().all(|e| e.detail.is_none()));
        assert!(a1
            .engine
            .activity()
            .overlaps
            .iter()
            .all(|o| o.level == OverlapLevel::SameArea && o.path.is_none()));
        assert!(a1
            .engine
            .teammate_files()
            .iter()
            .all(|f| f.lines.is_empty()));
    }

    /// A record stamp of the key period added at log index 3.
    fn at(counter: u64) -> Stamp {
        Stamp { period: 3, counter }
    }

    #[test]
    fn detail_marks_stay_bounded_per_device_and_refuse_dropped_sessions() {
        let mut marks = LiveMarks::default();
        let bob = holder("bob", "b1");
        for n in 0..MAX_DETAIL_SESSIONS as u64 {
            assert_eq!(
                marks.detail(&bob, &format!("s{n}"), at(100 + n)),
                Mark::Raised
            );
        }
        // One more: the oldest session's mark goes, and the floor rises to it.
        assert_eq!(marks.detail(&bob, "late", at(500)), Mark::Raised);
        assert_eq!(marks.detail.len(), MAX_DETAIL_SESSIONS);
        assert_eq!(marks.detail(&bob, "s0", at(100)), Mark::Stale);
        // A newer session (or record) of the device still passes; an older unknown one does not.
        assert_eq!(marks.detail(&bob, "later", at(600)), Mark::Raised);
        assert_eq!(marks.detail(&bob, "ancient", at(50)), Mark::Stale);
        assert_eq!(marks.detail(&bob, "late", at(500)), Mark::Seen);
        assert_eq!(marks.detail(&bob, "late", at(499)), Mark::Stale);
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
    fn carol_joins(relay: &mut Relay, a1: &mut Client, b1: &mut Client) -> Client {
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
        c1
    }

    /// A presence status `client` signs over its verified head index with another hash.
    fn forked_status(relay: &Relay, client: &mut Client) -> String {
        let (cipher, head, _) = client.engine.current_cipher().unwrap();
        let forked = ChainHead {
            index: head.index,
            hash: hangar_team_protocol::e2e::LogHash::from_bytes([7; 32]),
        };
        let status = br#"{"v":1,"name":"","sessions":[]}"#;
        let sealed = client
            .engine
            .seal(&cipher, &forked, RecordKind::PresenceStatus, b"", status)
            .unwrap();
        presence_text(relay, &client.holder, Some(sealed))
    }

    #[test]
    fn devices_of_two_members_reporting_another_history_only_make_the_log_fetch_decide() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let mut c1 = carol_joins(&mut relay, &mut a1, &mut b1);
        assert_eq!(connected(&c1), (1, 3, 3));
        // One device: set aside, and the log is fetched again in case the fork is this one's.
        let effects = a1.engine.on_text(&forked_status(&relay, &mut b1));
        assert!(log_get(&effects).is_some(), "{effects:?}");
        assert!(!effects.iter().any(|e| matches!(e, Effect::End(_))));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        assert!(a1.ended.is_none());
        assert_eq!(a1.engine.activity().divergent.len(), 1);
        // A second member's device too: two members working together must not stop everyone.
        // Both stay set aside and named in the Team view, and the log is fetched again; while
        // that fetch agrees with what this device verified, it goes on.
        let effects = a1.engine.on_text(&forked_status(&relay, &mut c1));
        assert!(!stops(&effects), "{effects:?}");
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(0, effects)],
        );
        assert!(a1.ended.is_none());
        assert_eq!(a1.engine.activity().divergent.len(), 2);
        assert_eq!(connected(&a1).0, 1);
        // A log fetch that contradicts the verified head still stops it.
        let mut stored = a1.engine.keys().head.unwrap();
        stored.hash = hangar_team_protocol::e2e::LogHash::from_bytes([9; 32]);
        a1.engine.keys.head = Some(stored);
        let effects = a1.engine.sync_log();
        run(&mut relay, &mut [&mut a1], vec![(0, effects)]);
        assert_eq!(
            a1.ended,
            Some(SessionEnd::Stop(StopReason::Attention(
                AttentionReason::HistoryMismatch
            )))
        );
    }

    /// A presence status `client` signs over its own verified head.
    fn honest_status(relay: &Relay, client: &mut Client) -> String {
        let (cipher, head, _) = client.engine.current_cipher().unwrap();
        let status = br#"{"v":1,"name":"","sessions":[]}"#;
        let sealed = client
            .engine
            .seal(&cipher, &head, RecordKind::PresenceStatus, b"", status)
            .unwrap();
        presence_text(relay, &client.holder, Some(sealed))
    }

    fn stops(effects: &[Effect]) -> bool {
        effects.iter().any(|e| {
            matches!(
                e,
                Effect::End(SessionEnd::Stop(StopReason::Attention(
                    AttentionReason::HistoryMismatch
                )))
            )
        })
    }

    #[test]
    fn devices_of_one_member_never_stop_the_team_even_when_nobody_else_is_heard() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let mut c1 = carol_joins(&mut relay, &mut a1, &mut b1);
        let mut b2 = client("bob", "b2", Role::Member);
        let hello = b2.engine.hello(ticket());
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1, &mut b2],
            vec![(3, hello)],
        );
        // Bob confirms his own new device.
        let poll = b1.engine.poll_devices();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1, &mut b2],
            vec![(1, poll)],
        );
        let request = b1.engine.pending().into_iter().next().unwrap();
        let confirm = b1.engine.confirm(&request.id).unwrap();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1, &mut b2],
            vec![(1, confirm)],
        );
        assert_eq!(connected(&b2), (1, 3, 4));

        // Bob's two devices never stop the team, whether another device is heard or not: which
        // devices are heard is the relay's word.
        a1.engine.peers.clear();
        let effects = a1.engine.on_text(&honest_status(&relay, &mut c1));
        assert!(!stops(&effects), "{effects:?}");
        assert_eq!(a1.engine.peers.len(), 1);
        for heard in [true, false] {
            a1.engine.divergent.clear();
            if !heard {
                a1.engine.peers.clear();
            }
            for device in [&mut b1, &mut b2] {
                let effects = a1.engine.on_text(&forked_status(&relay, device));
                assert!(!stops(&effects), "heard: {heard}, {effects:?}");
                assert!(
                    log_get(&effects).is_some() || a1.engine.syncing,
                    "{effects:?}"
                );
            }
            assert_eq!(a1.engine.activity().divergent.len(), 2);
            assert!(a1.engine.peers.len() <= 1);
        }
        // A second member's device too: still only set aside (see the next test).
        let effects = a1.engine.on_text(&forked_status(&relay, &mut c1));
        assert!(!stops(&effects), "{effects:?}");
        assert_eq!(a1.engine.activity().divergent.len(), 3);
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
        let _ = carol_joins(&mut relay, &mut a1, &mut b1);
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
        // A state file of the first format: one mark per signing key of the same device, and no
        // key period.
        let mark = |counter: u64| HolderMark {
            holder: bob.clone(),
            period: 0,
            counter,
        };
        let local = LocalState {
            presence_marks: vec![mark(9), mark(7)],
            ..LocalState::default()
        };
        let mut marks = LiveMarks::from_local(&local);
        assert_eq!(marks.presence.len(), 1);
        assert_eq!(marks.presence[&bob].counter, 9);
        // Without a stored period the counter alone decides.
        assert_eq!(marks.presence(&bob, at(8)), Mark::Stale);
        assert_eq!(marks.presence(&bob, at(9)), Mark::Seen);
        assert_eq!(marks.presence(&bob, at(10)), Mark::Raised);
        // The device id admitted again (keys added at index 9) with a clock behind: newer anyway.
        let readmitted = Stamp {
            period: 9,
            counter: 5,
        };
        assert_eq!(marks.presence(&bob, readmitted), Mark::Raised);
        // A status of the old keys stays older, whatever its counter.
        assert_eq!(marks.presence(&bob, at(u64::MAX)), Mark::Stale);
        // Stored with the period and read back alike.
        let mut stored = LocalState::default();
        marks.store(&mut stored);
        assert_eq!(stored.presence_marks[0].period, 9);
        assert_eq!(LiveMarks::from_local(&stored), marks);
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
        // A rise left for later goes along with the next write that happens anyway.
        advance(1_000);
        assert_eq!(stores(&a1.engine.on_text(&three)), 0);
        assert_ne!(LiveMarks::from_local(a1.engine.local()), a1.engine.live);
        let written = LiveMarks::from_local(a1.engine.local_to_store());
        assert_eq!(written, a1.engine.live);
        assert!(a1.engine.flush_marks().is_empty(), "nothing left");
        advance(MARKS_STORE_EVERY_MS);
        let four = status("four.rs");
        assert_eq!(stores(&a1.engine.on_text(&four)), 1);
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

    // -----------------------------------------------------------------------------------------
    // Reservations
    // -----------------------------------------------------------------------------------------

    /// Alice and Bob connected with a shared session each (their names go into the notes).
    fn team_sharing() -> (Relay, Client, Client) {
        let (mut relay, mut a1, mut b1) = team_of_two();
        let a = a1
            .engine
            .set_share(share("Alice", 1, "Claude Code", &[("src/a.rs", 1, 2)]));
        let b = b1
            .engine
            .set_share(share("Bob", 7, "Codex", &[("docs/b.md", 1, 1)]));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, a), (1, b)]);
        (relay, a1, b1)
    }

    /// Delivers frames the relay made outside a client's request (time, presence) to everyone.
    fn deliver(relay: &mut Relay, clients: &mut [&mut Client], texts: Vec<String>) {
        for client in clients.iter_mut() {
            client.inbox.extend(texts.iter().cloned());
        }
        run(relay, clients, Vec::new());
    }

    fn reserve(
        relay: &mut Relay,
        clients: &mut [&mut Client],
        at: usize,
        title: &str,
        area: Option<&str>,
    ) {
        let session = area.map(|_| if at == 0 { 1 } else { 7 });
        let effects = clients[at]
            .engine
            .reserve_task(title, session, area)
            .unwrap();
        run(relay, clients, vec![(at, effects)]);
    }

    #[test]
    fn the_first_reservation_wins_and_the_second_waits_with_the_holder_named() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(
            &mut relay,
            &mut [&mut a1, &mut b1],
            0,
            "Fix login",
            Some("src/auth"),
        );
        let mine = a1.engine.activity().tasks;
        assert_eq!(mine.len(), 1);
        assert!(mine[0].mine);
        assert_eq!(mine[0].title, "Fix login");
        assert_eq!(mine[0].area.as_deref(), Some("src/auth"));
        assert!(
            a1.engine.activity().attempts.is_empty(),
            "held, not waiting"
        );

        // Bob asks for the same task (the title compares normalized): the relay says Alice
        // holds it; nothing is applied on Bob's side before the relay's word.
        let effects = b1.engine.reserve_task("  fix LOGIN ", None, None).unwrap();
        assert!(b1.engine.activity().attempts[0].state == AttemptState::Reserving);
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, effects)]);
        let seen = b1.engine.activity();
        assert_eq!(seen.tasks.len(), 1);
        assert_eq!(seen.tasks[0].title, "Fix login");
        assert_eq!(seen.tasks[0].name, "Alice");
        assert!(!seen.tasks[0].mine && !seen.tasks[0].own);
        assert_eq!(seen.attempts.len(), 1);
        assert_eq!(seen.attempts[0].state, AttemptState::Blocked);
        assert_eq!(seen.attempts[0].member.as_deref(), Some("alice"));
        assert_eq!(seen.attempts[0].name, "Alice");

        // The gate and the context: Alice's area pauses Bob's edits there; Bob's own wish is
        // named as wanted, Alice's own reservation as hers.
        let tasks = b1.engine.team_tasks();
        let held = tasks.iter().find(|t| !t.wanted).unwrap();
        assert_eq!(held.who, "Alice #alice");
        assert!(!held.mine);
        assert_eq!(held.area.as_deref(), Some("src/auth"));
        assert_eq!(held.repo.as_deref(), Some("a".repeat(40).as_str()));
        assert!(tasks.iter().any(|t| t.wanted && t.who == "Alice #alice"));
        assert!(a1
            .engine
            .team_tasks()
            .iter()
            .all(|t| t.mine && t.session == Some(1)));

        // The relay never saw the title, the name or the area.
        assert!(relay
            .task_events
            .iter()
            .all(|t| !t.contains("Fix login") && !t.contains("src/auth") && !t.contains("Alice")));
    }

    #[test]
    fn a_release_request_is_declined_then_accepted_and_the_requester_hears_both() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        reserve(&mut relay, &mut [&mut a1, &mut b1], 1, "Fix login", None);
        let task = b1.engine.activity().attempts[0].task.clone();

        let ask = b1.engine.ask_release(&task).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, ask)]);
        assert!(b1.engine.activity().attempts[0].asked);
        let card = a1.engine.activity().tasks[0].request.clone().unwrap();
        assert_eq!(card.member, "bob");
        assert_eq!(card.name, "Bob", "the requester's signed note names him");

        // Declined: the hold stays, Bob hears it.
        let keep = a1.engine.answer_release(&task, "bob", false).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, keep)]);
        assert!(a1.engine.activity().tasks[0].request.is_none());
        assert!(a1.engine.activity().tasks[0].mine);
        let notices = b1.engine.activity().notices;
        assert_eq!(notices.last().unwrap().kind, NoticeKind::ReleaseDeclined);
        assert_eq!(notices.last().unwrap().title, "Fix login");
        assert_eq!(notices.last().unwrap().name, "Alice");
        assert!(!b1.engine.activity().attempts[0].asked);
        // Asking again at once is refused by the relay's cooldown; Bob hears that too.
        let ask = b1.engine.ask_release(&task).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, ask)]);
        assert_eq!(
            b1.engine.activity().notices.last().unwrap().kind,
            NoticeKind::AskRefused
        );

        // After the cooldown: accepted, the task is Bob's, and the team reads its title again.
        relay.clock += 11 * 60_000;
        let ask = b1.engine.ask_release(&task).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, ask)]);
        let hand = a1.engine.answer_release(&task, "bob", true).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, hand)]);
        let bobs = b1.engine.activity();
        assert_eq!(bobs.tasks.len(), 1);
        assert!(bobs.tasks[0].mine);
        assert!(bobs.attempts.is_empty());
        assert_eq!(
            bobs.notices.last().unwrap().kind,
            NoticeKind::ReleaseAccepted
        );
        let alices = a1.engine.activity();
        assert_eq!(alices.tasks[0].member, "bob");
        assert_eq!(
            alices.tasks[0].title, "Fix login",
            "Bob's own note after the handover"
        );
        assert_eq!(alices.tasks[0].name, "Bob");
        assert!(
            a1.engine.answer_release(&task, "bob", true).is_err(),
            "no longer hers"
        );
    }

    #[test]
    fn an_offline_holders_reservation_warns_first_then_expires_and_the_waiting_one_gets_it() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        reserve(&mut relay, &mut [&mut a1, &mut b1], 1, "Fix login", None);
        // Alice's device goes offline: her hold starts to run (30 minutes without activity).
        let alice = a1.holder.clone();
        let offline = relay.presence_of(&alice, false);
        deliver(&mut relay, &mut [&mut a1, &mut b1], offline);
        let expires = b1.engine.activity().tasks[0].expires_at.unwrap();
        assert_eq!(expires, relay.clock + 30 * 60_000);

        // Five minutes before: the warning, for Alice (she sees it on any of her devices).
        let warned = relay.tick(26 * 60_000);
        deliver(&mut relay, &mut [&mut a1, &mut b1], warned);
        let notice = a1.engine.activity().notices.last().cloned().unwrap();
        assert_eq!(notice.kind, NoticeKind::Expiring);
        assert_eq!(notice.at, Some(expires));
        assert!(b1.engine.activity().tasks[0].expiring);
        assert!(b1
            .engine
            .activity()
            .notices
            .iter()
            .all(|n| n.kind != NoticeKind::Expiring));

        // Then it expires: Alice is told, Bob's waiting attempt tries again and wins.
        let expired = relay.tick(5 * 60_000);
        deliver(&mut relay, &mut [&mut a1, &mut b1], expired);
        assert_eq!(
            a1.engine.activity().notices.last().unwrap().kind,
            NoticeKind::Expired
        );
        let bobs = b1.engine.activity();
        assert_eq!(bobs.tasks.len(), 1);
        assert!(bobs.tasks[0].mine);
        assert!(bobs.attempts.is_empty());
    }

    /// `client`'s connection drops and its holds run out while it hears nothing; the others
    /// see it happen.
    fn runs_out_while_away(relay: &mut Relay, away: &mut Client, other: &mut Client) {
        let holder = away.holder.clone();
        let mut texts = relay.presence_of(&holder, false);
        texts.extend(relay.tick(31 * 60_000));
        other.inbox.extend(texts);
        run(relay, &mut [away, other], Vec::new());
    }

    /// The app quits and starts again: a new engine from what the device stored.
    fn restart(client: &mut Client) {
        let text = serde_json::to_string(client.engine.local_to_store()).unwrap();
        let local: LocalState = serde_json::from_str(&text).unwrap();
        let keys = StoredKeys::decode(&client.engine.keys().encode()).unwrap();
        let identity = DeviceIdentity::import(&client.engine.identity.export()).unwrap();
        let read = Arc::clone(&client.clock);
        client.engine = Engine::new(EngineSetup {
            project: "p1".to_owned(),
            team: team(),
            device: client.holder.device.clone(),
            identity,
            member: client.engine.member().cloned(),
            account: Some(client.holder.member.clone()),
            website_role: client.role,
            owner: Some(MemberId::new("alice").unwrap()),
            keys,
            local,
            now: Box::new(move || read.load(std::sync::atomic::Ordering::SeqCst)),
            entropy: Box::new(Scripted(Arc::clone(&client.fail_draw))),
            website_every_ms: WEBSITE_CHECK_EVERY_MS,
            members_every_ms: MEMBERS_CHECK_EVERY_MS,
        });
    }

    /// A new connection whose answer is `resync_required` with `snapshot` (the relay's
    /// `hello` itself is not sent: its welcome would skip the snapshot).
    fn come_back(relay: &mut Relay, clients: &mut [&mut Client], at: usize, snapshot: &[String]) {
        let hello = clients[at].engine.hello(ticket());
        assert!(hello.iter().any(|e| matches!(e, Effect::Send(_))));
        resync_with(relay, clients, at, snapshot);
    }

    fn ended_away(client: &Client) -> Vec<String> {
        client
            .engine
            .activity()
            .notices
            .iter()
            .filter(|n| n.kind == NoticeKind::EndedAway)
            .map(|n| n.title.clone())
            .collect()
    }

    #[test]
    fn a_reservation_that_ran_out_while_away_is_told_after_a_resync_and_not_taken_again() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        assert_eq!(a1.engine.local().own_holds.len(), 1);
        assert_eq!(a1.engine.local().own_holds[0].title, "Fix login");
        runs_out_while_away(&mut relay, &mut a1, &mut b1);
        assert!(b1.engine.activity().tasks.is_empty());
        assert!(ended_away(&a1).is_empty(), "she heard nothing yet");

        come_back(&mut relay, &mut [&mut a1, &mut b1], 0, &[]);
        assert_eq!(ended_away(&a1), ["Fix login"]);
        let hers = a1.engine.activity();
        assert!(
            hers.tasks.is_empty(),
            "not reserved again without her knowing"
        );
        assert!(hers.attempts.is_empty());
        assert!(a1.engine.local().own_holds.is_empty());
        assert!(b1.engine.activity().tasks.is_empty());

        // Once told: the next resync says nothing more.
        come_back(&mut relay, &mut [&mut a1, &mut b1], 0, &[]);
        assert_eq!(ended_away(&a1).len(), 1);
    }

    #[test]
    fn a_reservation_that_ran_out_before_a_restart_is_told_after_it() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Write docs", None);
        runs_out_while_away(&mut relay, &mut a1, &mut b1);
        restart(&mut a1);
        assert!(a1.engine.activity().notices.is_empty());

        come_back(&mut relay, &mut [&mut a1, &mut b1], 0, &[]);
        let mut told = ended_away(&a1);
        told.sort();
        assert_eq!(told, ["Fix login", "Write docs"]);
        assert!(a1.engine.local().own_holds.is_empty());
    }

    #[test]
    fn a_reservation_that_still_stands_after_a_restart_or_a_resync_is_not_told() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        let held = relay
            .task_events
            .iter()
            .rev()
            .find(|t| t.contains("task.reserved"))
            .cloned()
            .unwrap();
        come_back(
            &mut relay,
            &mut [&mut a1, &mut b1],
            0,
            std::slice::from_ref(&held),
        );
        restart(&mut a1);
        come_back(&mut relay, &mut [&mut a1, &mut b1], 0, &[held]);
        assert!(ended_away(&a1).is_empty());
        let hers = a1.engine.activity();
        assert_eq!(hers.tasks.len(), 1);
        assert!(hers.tasks[0].mine);
        assert_eq!(a1.engine.local().own_holds.len(), 1);
    }

    #[test]
    fn a_reservation_that_ran_out_while_online_is_told_once_not_again_after_a_resync() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        let alice = a1.holder.clone();
        let mut texts = relay.presence_of(&alice, false);
        texts.extend(relay.tick(31 * 60_000));
        deliver(&mut relay, &mut [&mut a1, &mut b1], texts);
        assert!(a1
            .engine
            .activity()
            .notices
            .iter()
            .any(|n| n.kind == NoticeKind::Expired));
        assert!(a1.engine.local().own_holds.is_empty());
        come_back(&mut relay, &mut [&mut a1, &mut b1], 0, &[]);
        assert!(ended_away(&a1).is_empty());
    }

    #[test]
    fn own_holds_in_the_state_file_read_back_and_older_files_have_none() {
        let local = LocalState {
            own_holds: vec![OwnHold {
                task: "ab".repeat(32),
                title: "Fix login".to_owned(),
            }],
            ..LocalState::default()
        };
        let text = serde_json::to_string(&local).unwrap();
        assert!(text.contains("\"ownHolds\""), "{text}");
        assert_eq!(serde_json::from_str::<LocalState>(&text).unwrap(), local);
        let older: LocalState = serde_json::from_str("{\"counter\":3}").unwrap();
        assert!(older.own_holds.is_empty());
        assert!(!serde_json::to_string(&older).unwrap().contains("ownHolds"));
    }

    /// Re-sends a logged frame with the next sequence number.
    fn again(relay: &mut Relay, text: &str) -> String {
        let mut frame = decode_server(text).unwrap();
        relay.seq += 1;
        frame.seq = Some(Seq(relay.seq));
        encode_server(&frame).unwrap()
    }

    #[test]
    fn reservations_of_unlisted_devices_replays_and_older_notes_are_ignored() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(
            &mut relay,
            &mut [&mut a1, &mut b1],
            0,
            "Fix login",
            Some("src/old"),
        );
        let first = relay.task_events.last().cloned().unwrap();
        reserve(
            &mut relay,
            &mut [&mut a1, &mut b1],
            0,
            "Fix login",
            Some("src/new"),
        );
        assert_eq!(
            b1.engine.activity().tasks[0].area.as_deref(),
            Some("src/new")
        );
        // The relay shows the older note again (with a new seq): the newer stays.
        let old = again(&mut relay, &first);
        deliver(&mut relay, &mut [&mut a1, &mut b1], vec![old]);
        assert_eq!(
            b1.engine.activity().tasks[0].area.as_deref(),
            Some("src/new")
        );
        // The same frame again (same seq) is a duplicate.
        let same = relay.task_events.last().cloned().unwrap();
        let before = b1.engine.activity().tasks.len();
        deliver(&mut relay, &mut [&mut a1, &mut b1], vec![same]);
        assert_eq!(b1.engine.activity().tasks.len(), before);

        // A holder the verified log does not list holds nothing here.
        relay.seq += 1;
        let made_up = relay.text(
            None,
            Some(relay.seq),
            ServerMessage::TaskReserved(TaskReserved {
                task: KeyedHash::from_bytes([7; 32]),
                holder: holder("mallory", "m1"),
                epoch: Epoch(1),
                since: UnixMillis(1),
                expires_at: None,
                note: None,
            }),
        );
        deliver(&mut relay, &mut [&mut a1, &mut b1], vec![made_up]);
        assert_eq!(b1.engine.activity().tasks.len(), 1);
        // A note moved to another task does not open there: no title, no area for the gate.
        let mut frame = decode_server(&first).unwrap();
        if let ServerMessage::TaskReserved(m) = &mut frame.message {
            m.task = KeyedHash::from_bytes([9; 32]);
        }
        relay.seq += 1;
        frame.seq = Some(Seq(relay.seq));
        let moved = encode_server(&frame).unwrap();
        deliver(&mut relay, &mut [&mut a1, &mut b1], vec![moved]);
        let tasks = b1.engine.activity().tasks;
        assert_eq!(tasks.len(), 2);
        assert!(tasks.iter().any(|t| t.title.is_empty() && t.area.is_none()));
        assert_eq!(b1.engine.team_tasks().len(), 1, "only the signed one");
    }

    #[test]
    fn the_first_connection_of_an_engine_takes_a_snapshot_and_the_next_one_resumes() {
        let (_relay, mut a1, _b1) = team_sharing();
        let mut fresh = client("carol", "c1", Role::Member);
        let hello = fresh.engine.hello(ticket());
        let Effect::Send(ClientFrame {
            message: ClientMessage::Hello(first),
            ..
        }) = hello.iter().find(|e| matches!(e, Effect::Send(_))).unwrap()
        else {
            panic!("hello not sent");
        };
        assert!(
            first.last_seq.is_none(),
            "no cursor before the first snapshot"
        );
        // A welcomed engine resumes from its cursor.
        let hello = a1.engine.hello(ticket());
        let Effect::Send(ClientFrame {
            message: ClientMessage::Hello(next),
            ..
        }) = hello.iter().find(|e| matches!(e, Effect::Send(_))).unwrap()
        else {
            panic!("hello not sent");
        };
        assert!(next.last_seq.is_some());
    }

    /// The op id of the `task.reserve` among `effects`.
    fn reserve_op(effects: &[Effect]) -> OpId {
        effects
            .iter()
            .find_map(|e| match e {
                Effect::Send(ClientFrame {
                    id,
                    message: ClientMessage::TaskReserve(_),
                    ..
                }) => Some(id.clone()),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn the_activity_lists_the_sessions_a_reservation_can_be_linked_to() {
        let (_relay, a1, _b1) = team_sharing();
        assert_eq!(a1.engine.activity().sessions, [1]);
        // A session outside the share is refused.
        let mut a1 = a1;
        assert!(matches!(
            a1.engine.reserve_task("x", Some(9), None),
            Err(TeamError::UnknownRepository)
        ));
    }

    #[test]
    fn a_conflict_naming_an_unknown_holder_waits_for_the_log_then_fails() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        let effects = a1.engine.reserve_task("Fix login", None, None).unwrap();
        let id = reserve_op(&effects);
        // The relay answers with a holder the log does not list.
        let refused = relay.text(
            Some(id),
            None,
            ServerMessage::Err(ErrorBody {
                code: ErrorCode::Conflict,
                retry_after_ms: None,
                holder: Some(holder("mallory", "m1")),
            }),
        );
        let after = a1.engine.on_text(&refused);
        // Not shown as blocked by an unknown device: it still waits, and the log is fetched.
        let attempt = &a1.engine.activity().attempts[0];
        assert_eq!(attempt.state, AttemptState::Reserving);
        assert!(attempt.member.is_none());
        assert!(
            a1.engine.team_tasks().is_empty(),
            "no pause for the relay's word"
        );
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, after)]);
        // The log still does not list it: the attempt ends, with a notice.
        let activity = a1.engine.activity();
        assert!(activity.attempts.is_empty());
        assert_eq!(
            activity.notices.last().unwrap().kind,
            NoticeKind::ReserveFailed
        );
    }

    #[test]
    fn a_note_of_an_ended_hold_never_comes_back_also_after_a_resync_or_a_restart() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(
            &mut relay,
            &mut [&mut a1, &mut b1],
            0,
            "Fix login",
            Some("src/auth"),
        );
        let noted = relay.task_events.last().cloned().unwrap();
        let task = a1.engine.activity().tasks[0].task.clone();
        let release = a1.engine.release_task(&task).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, release)]);
        assert!(b1.engine.activity().tasks.is_empty());
        // The relay brings the hold back with its old note: no title, no area, no pause.
        let old = again(&mut relay, &noted);
        deliver(&mut relay, &mut [&mut a1, &mut b1], vec![old]);
        let shown = b1.engine.activity().tasks;
        assert_eq!(shown.len(), 1);
        assert!(shown[0].title.is_empty() && shown[0].area.is_none());
        assert!(b1.engine.team_tasks().is_empty());
        // After resync_required and its snapshot: still old.
        relay.seq += 1;
        let resync = relay.text(
            None,
            None,
            ServerMessage::ResyncRequired(hangar_team_protocol::wire::ResyncRequired {
                stream: StreamId::new("s").unwrap(),
                head: Seq(relay.seq),
                reason: hangar_team_protocol::wire::ResyncReason::TooOld,
            }),
        );
        let mut snapshot = decode_server(&noted).unwrap();
        snapshot.seq = None;
        let snapshot = encode_server(&snapshot).unwrap();
        let welcome = relay.text(
            None,
            None,
            ServerMessage::Welcome(Welcome {
                proto: 1,
                stream: StreamId::new("s").unwrap(),
                head: Seq(relay.seq),
                member: b1.holder.member.clone(),
                epoch: Epoch(0),
                challenge: None,
            }),
        );
        b1.inbox.extend([resync, snapshot, welcome]);
        run(&mut relay, &mut [&mut a1, &mut b1], Vec::new());
        assert!(b1
            .engine
            .activity()
            .tasks
            .iter()
            .all(|t| t.title.is_empty()));
        assert!(b1.engine.team_tasks().is_empty());
        // Across a restart: the marks are in the state file.
        let flushed = b1.engine.flush_marks();
        drop(flushed);
        let local = b1.engine.local_to_store().clone();
        let text = serde_json::to_string(&local).unwrap();
        let restored: LocalState = serde_json::from_str(&text).unwrap();
        assert!(!restored.note_marks.is_empty());
        let mut marks = LiveMarks::from_local(&restored);
        let alice = a1.holder.clone();
        let key = note_key(
            RecordKind::ReservationNote,
            &KeyedHash::try_from(task).unwrap(),
        );
        let mark = *marks.notes.get(&(alice.clone(), key.clone())).unwrap();
        let old = Stamp {
            period: mark.period,
            counter: mark.counter - 1,
        };
        assert_eq!(marks.note(&alice, &key, old, &BTreeSet::new()), Mark::Stale);
    }

    #[test]
    fn notices_come_only_with_a_matching_hold_or_request() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        let task = KeyedHash::try_from(a1.engine.activity().tasks[0].task.clone()).unwrap();
        let before = a1.engine.activity().notices.len();
        // An expiry of a hold another device of the relay's choosing never had: nothing.
        relay.seq += 1;
        let expired = relay.text(
            None,
            Some(relay.seq),
            ServerMessage::ReservationExpired(ReservationExpired {
                task: KeyedHash::from_bytes([3; 32]),
                holder: a1.holder.clone(),
            }),
        );
        // An answer to a request Bob never made.
        relay.seq += 1;
        let answered = relay.text(
            None,
            Some(relay.seq),
            ServerMessage::TaskReleaseAnswered(TaskReleaseAnswered {
                task,
                holder: a1.holder.clone(),
                requester: b1.holder.member.clone(),
                accepted: true,
            }),
        );
        deliver(&mut relay, &mut [&mut a1, &mut b1], vec![expired, answered]);
        assert_eq!(a1.engine.activity().notices.len(), before);
        assert!(b1.engine.activity().notices.is_empty());
    }

    #[test]
    fn only_a_signed_request_gets_a_card_and_it_is_answered_once() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        let hex = a1.engine.activity().tasks[0].task.clone();
        let task = KeyedHash::try_from(hex.clone()).unwrap();
        // The relay's word alone (no note): no card, no answer.
        relay.seq += 1;
        let unsigned = relay.text(
            None,
            Some(relay.seq),
            ServerMessage::TaskReleaseRequested(TaskReleaseRequested {
                task,
                holder: a1.holder.clone(),
                requester: b1.holder.member.clone(),
                note: None,
            }),
        );
        deliver(&mut relay, &mut [&mut a1, &mut b1], vec![unsigned]);
        let request = a1.engine.activity().tasks[0].request.clone().unwrap();
        assert!(!request.confirmed);
        assert!(matches!(
            a1.engine.answer_release(&hex, "bob", true),
            Err(TeamError::UnknownTask)
        ));
        // Ended, then Bob asks for real: a card for Bob only, answered once.
        relay.seq += 1;
        let ended = relay.text(
            None,
            Some(relay.seq),
            ServerMessage::TaskReleaseRequestEnded(TaskReleaseRequestEnded {
                task,
                holder: a1.holder.clone(),
                requester: b1.holder.member.clone(),
                reason: RequestEndReason::Withdrawn,
            }),
        );
        deliver(&mut relay, &mut [&mut a1, &mut b1], vec![ended]);
        let ask = b1.engine.ask_release(&hex).unwrap();
        // Delivered by hand, so the answer can be checked before the relay sees it.
        for effect in ask {
            if let Effect::Send(frame) = effect {
                let (own, all, _) = relay.handle(&b1.holder.clone(), Role::Member, frame);
                b1.inbox.extend(own);
                a1.inbox.extend(all.iter().cloned());
                b1.inbox.extend(all);
            }
        }
        run(&mut relay, &mut [&mut a1, &mut b1], Vec::new());
        let request = a1.engine.activity().tasks[0].request.clone().unwrap();
        assert!(request.confirmed);
        assert!(matches!(
            a1.engine.answer_release(&hex, "carol", false),
            Err(TeamError::UnknownTask)
        ));
        let answer = a1.engine.answer_release(&hex, "bob", false).unwrap();
        assert!(
            a1.engine.activity().tasks[0].request.is_none(),
            "the card is gone"
        );
        assert!(a1.engine.answer_release(&hex, "bob", false).is_err());
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, answer)]);
        assert_eq!(
            b1.engine.activity().notices.last().unwrap().kind,
            NoticeKind::ReleaseDeclined
        );
    }

    /// A reservation note a1 signs for `task` with any plaintext, as the relay would carry it.
    fn forged_note(client: &mut Client, task: &KeyedHash, plaintext: &[u8]) -> Sealed {
        let (cipher, head, _) = client.engine.current_cipher().unwrap();
        let record = record_task_id(task);
        client
            .engine
            .seal(
                &cipher,
                &head,
                RecordKind::ReservationNote,
                record.as_bytes(),
                plaintext,
            )
            .unwrap()
    }

    #[test]
    fn a_note_must_name_its_own_task_and_a_plain_name() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        let task = KeyedHash::try_from(a1.engine.activity().tasks[0].task.clone()).unwrap();
        let note = |title: &str, name: &str| {
            team_tasks::note_plaintext(&TaskNote {
                title: title.into(),
                name: name.into(),
                ..TaskNote::default()
            })
        };
        // Signed by the holder for this hash, but the title is another task's.
        let other = forged_note(&mut a1, &task, &note("Something else", "Alice"));
        // The right title with a name that reorders text.
        let bidi = forged_note(&mut a1, &task, &note("Fix login", "Al\u{202E}ice"));
        for sealed in [other, bidi] {
            relay.seq += 1;
            let text = relay.text(
                None,
                Some(relay.seq),
                ServerMessage::TaskReserved(TaskReserved {
                    task,
                    holder: a1.holder.clone(),
                    epoch: Epoch(1),
                    since: UnixMillis(1),
                    expires_at: None,
                    note: Some(sealed),
                }),
            );
            deliver(&mut relay, &mut [&mut a1, &mut b1], vec![text]);
            let shown = &b1.engine.activity().tasks[0];
            assert_eq!(shown.title, "Fix login", "the earlier note stays");
            assert_eq!(shown.name, "Alice");
        }
    }

    #[test]
    fn prev_hashes_go_only_for_holds_of_an_earlier_epoch() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        // Bob leaves: Alice's device rotates to epoch 2 and moves its hold with `prev`.
        let remove = a1
            .engine
            .remove_member(&MemberId::new("bob").unwrap())
            .unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, remove)]);
        assert_eq!(connected(&a1).0, 2);
        let moved = relay.reserves.last().unwrap();
        assert_eq!(moved.epoch, Epoch(2));
        assert_eq!(moved.prev.len(), 1, "the old hash of its own hold");
        // A task nobody held under the old epoch names no old hash.
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Brand new", None);
        assert!(relay.reserves.last().unwrap().prev.is_empty());
    }

    #[test]
    fn a_hold_still_standing_under_an_older_epoch_after_coming_back_moves_to_the_current_hash() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        // Alice's second device, so the team rotates while her first one is away.
        let mut a2 = client("alice", "a2", Role::Owner);
        let hello = a2.engine.hello(ticket());
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut a2],
            vec![(2, hello)],
        );
        let poll = a1.engine.poll_devices();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut a2],
            vec![(0, poll)],
        );
        let own = a1.engine.pending().into_iter().next().unwrap();
        let confirm = a1.engine.confirm(&own.id).unwrap();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut a2],
            vec![(0, confirm)],
        );
        assert_eq!(connected(&a2).0, 1);
        reserve(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut a2],
            0,
            "Fix login",
            None,
        );
        let held = relay
            .task_events
            .iter()
            .rev()
            .find(|t| t.contains("task.reserved"))
            .cloned()
            .unwrap();

        // Away: Bob leaves and the second device rotates to epoch 2 without her first one.
        let remove = a2
            .engine
            .remove_member(&MemberId::new("bob").unwrap())
            .unwrap();
        run(&mut relay, &mut [&mut a2, &mut b1], vec![(0, remove)]);
        assert_eq!(connected(&a2).0, 2);
        let sent = relay.reserves.len();

        // Back with a snapshot that still has her hold under epoch 1.
        come_back(&mut relay, &mut [&mut a1, &mut a2], 0, &[held]);
        assert!(ended_away(&a1).is_empty());
        assert_eq!(connected(&a1).0, 2);
        let moved: Vec<&TaskReserve> = relay.reserves[sent..]
            .iter()
            .filter(|r| r.epoch == Epoch(2))
            .collect();
        assert_eq!(moved.len(), 1, "{:?}", relay.reserves[sent..].to_vec());
        assert_eq!(moved[0].prev.len(), 1, "the old hash of her hold");
        let own = &a1.engine.local().own_holds;
        assert_eq!(own.len(), 1);
        assert_eq!(own[0].task, record_task_id(&moved[0].task));
        assert_eq!(own[0].title, "Fix login");
    }

    #[test]
    fn a_move_to_the_current_epoch_lost_with_the_connection_goes_again_after_the_check() {
        let (mut relay, mut a1, mut b1) = team_of_two();
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        let held = relay
            .task_events
            .iter()
            .rev()
            .find(|t| t.contains("task.reserved"))
            .cloned()
            .unwrap();
        let before = a1.engine.local().own_holds.clone();
        // Bob leaves: her device rotates to epoch 2, but its move of the hold is lost with the
        // connection.
        relay.lose_reserves = true;
        let remove = a1
            .engine
            .remove_member(&MemberId::new("bob").unwrap())
            .unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, remove)]);
        relay.lose_reserves = false;
        assert_eq!(connected(&a1).0, 2);
        assert_eq!(a1.engine.local().own_holds, before, "still under epoch 1");
        let sent = relay.reserves.len();

        // Back with the hold still under epoch 1. The device holds epoch 2's key already, so
        // the share on the new connection runs before the check and holds the attempt back;
        // the check moves it.
        come_back(&mut relay, &mut [&mut a1], 0, &[held]);
        assert!(ended_away(&a1).is_empty());
        let moved: Vec<&TaskReserve> = relay.reserves[sent..]
            .iter()
            .filter(|r| r.epoch == Epoch(2))
            .collect();
        assert_eq!(moved.len(), 1, "{:?}", relay.reserves[sent..].to_vec());
        assert_eq!(moved[0].prev.len(), 1, "the old hash of her hold");
        let own = &a1.engine.local().own_holds;
        assert_eq!(own.len(), 1);
        assert_eq!(own[0].task, record_task_id(&moved[0].task));
    }

    #[test]
    fn unknown_holders_fetch_the_log_at_most_every_few_seconds() {
        let (relay, mut a1, _b1) = team_sharing();
        let mut fetches = 0;
        for n in 0..5u8 {
            let text = relay.text(
                None,
                None,
                ServerMessage::TaskReserved(TaskReserved {
                    task: KeyedHash::from_bytes([n; 32]),
                    holder: holder("mallory", &format!("m{n}")),
                    epoch: Epoch(1),
                    since: UnixMillis(1),
                    expires_at: None,
                    note: None,
                }),
            );
            fetches += a1
                .engine
                .on_text(&text)
                .iter()
                .filter(|e| {
                    matches!(
                        e,
                        Effect::Send(ClientFrame {
                            message: ClientMessage::MemberLogGet(_),
                            ..
                        })
                    )
                })
                .count();
        }
        assert_eq!(fetches, 1);
    }

    /// `resync_required`, the given snapshot events (without `seq`) and the `welcome` to `client`.
    fn resync_with(relay: &mut Relay, clients: &mut [&mut Client], at: usize, snapshot: &[String]) {
        let resync = relay.text(
            None,
            None,
            ServerMessage::ResyncRequired(hangar_team_protocol::wire::ResyncRequired {
                stream: StreamId::new("s").unwrap(),
                head: Seq(relay.seq),
                reason: hangar_team_protocol::wire::ResyncReason::TooOld,
            }),
        );
        let welcome = relay.text(
            None,
            None,
            ServerMessage::Welcome(Welcome {
                proto: 1,
                stream: StreamId::new("s").unwrap(),
                head: Seq(relay.seq),
                member: clients[at].holder.member.clone(),
                epoch: Epoch(0),
                challenge: None,
            }),
        );
        let mut texts = vec![resync];
        for text in snapshot {
            let mut frame = decode_server(text).unwrap();
            frame.seq = None;
            texts.push(encode_server(&frame).unwrap());
        }
        texts.push(welcome);
        clients[at].inbox.extend(texts);
        run(relay, clients, Vec::new());
    }

    #[test]
    fn marks_of_running_holds_survive_many_old_marks_and_a_resync() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        let mut latest = Vec::new();
        for n in 0..32 {
            reserve(
                &mut relay,
                &mut [&mut a1, &mut b1],
                0,
                &format!("Task {n}"),
                None,
            );
            latest.push(relay.task_events.last().cloned().unwrap());
        }
        assert_eq!(b1.engine.activity().tasks.len(), 32);
        // 33 newer marks of Alice's device for notes that do not run any more.
        let alice = a1.holder.clone();
        let top = b1
            .engine
            .live
            .notes
            .iter()
            .filter(|((h, _), _)| *h == alice)
            .map(|(_, s)| *s)
            .max()
            .unwrap();
        for n in 0..33u64 {
            let protected = b1.engine.running_notes(&alice);
            let stamp = Stamp {
                period: top.period,
                counter: top.counter + 100 + n,
            };
            b1.engine
                .live
                .note(&alice, &format!("old/{n}"), stamp, &protected);
        }
        // Through the state file (a restart) and a resync with the same holds: all 32 stay.
        let restored = LiveMarks::from_local(b1.engine.local_to_store());
        for task in b1.engine.holds.keys() {
            let key = note_key(RecordKind::ReservationNote, task);
            assert!(restored.notes.contains_key(&(alice.clone(), key)));
        }
        resync_with(&mut relay, &mut [&mut a1, &mut b1], 1, &latest);
        let tasks = b1.engine.activity().tasks;
        assert_eq!(tasks.len(), 32);
        assert!(tasks.iter().all(|t| !t.title.is_empty()), "{tasks:?}");
    }

    #[test]
    fn a_blocked_attempt_pauses_only_while_the_blocking_hold_is_known() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        // A conflict naming a listed holder, but no hold behind it: nothing pauses.
        let effects = b1.engine.reserve_task("Ghost", Some(7), None).unwrap();
        let id = reserve_op(&effects);
        let refused = relay.text(
            Some(id),
            None,
            ServerMessage::Err(ErrorBody {
                code: ErrorCode::Conflict,
                retry_after_ms: None,
                holder: Some(a1.holder.clone()),
            }),
        );
        let _ = b1.engine.on_text(&refused);
        assert_eq!(
            b1.engine.activity().attempts[0].state,
            AttemptState::Blocked
        );
        assert!(b1.engine.team_tasks().iter().all(|t| !t.wanted));
        // A real hold blocks and pauses Bob's session; its release ends the pause.
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        let effects = b1.engine.reserve_task("Fix login", Some(7), None).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, effects)]);
        assert!(b1
            .engine
            .team_tasks()
            .iter()
            .any(|t| t.wanted && t.session == Some(7)));
        let task = a1
            .engine
            .activity()
            .tasks
            .iter()
            .find(|t| t.title == "Fix login")
            .unwrap()
            .task
            .clone();
        let release = a1.engine.release_task(&task).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, release)]);
        assert!(b1
            .engine
            .team_tasks()
            .iter()
            .all(|t| !(t.wanted && t.title == "Fix login")));
    }

    #[test]
    fn a_repeated_request_keeps_its_answer_and_a_replaced_one_is_old() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        let hex = a1.engine.activity().tasks[0].task.clone();
        let ask = b1.engine.ask_release(&hex).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, ask)]);
        let requested = relay
            .task_events
            .iter()
            .rev()
            .find(|t| t.contains("task.release_requested"))
            .cloned()
            .unwrap();
        let _answer = a1.engine.answer_release(&hex, "bob", false).unwrap();
        // The relay repeats the request: the card does not come back.
        let repeat = again(&mut relay, &requested);
        let _ = a1.engine.on_text(&repeat);
        assert!(a1.engine.activity().tasks[0].request.is_none());
        // Replaced by an unsigned one, then the signed one again: old now, no card.
        let mut frame = decode_server(&requested).unwrap();
        if let ServerMessage::TaskReleaseRequested(m) = &mut frame.message {
            m.note = None;
        }
        relay.seq += 1;
        frame.seq = Some(Seq(relay.seq));
        let unsigned = encode_server(&frame).unwrap();
        let _ = a1.engine.on_text(&unsigned);
        let back = again(&mut relay, &requested);
        let _ = a1.engine.on_text(&back);
        let request = a1.engine.activity().tasks[0].request.clone().unwrap();
        assert!(!request.confirmed);
    }

    #[test]
    fn a_hold_missing_from_a_snapshot_has_an_old_note_afterwards() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        let noted = relay.task_events.last().cloned().unwrap();
        // The hold ended while Bob did not hear it: the snapshot has no hold.
        resync_with(&mut relay, &mut [&mut a1, &mut b1], 1, &[]);
        assert!(b1.engine.activity().tasks.is_empty());
        let old = again(&mut relay, &noted);
        let _ = b1.engine.on_text(&old);
        let tasks = b1.engine.activity().tasks;
        assert_eq!(tasks.len(), 1);
        assert!(tasks[0].title.is_empty());
    }

    #[test]
    fn a_note_never_opened_is_old_once_its_hold_ended() {
        let (_relay, mut a1, mut b1) = team_sharing();
        let task = KeyedHash::from_bytes([5; 32]);
        let sealed = forged_note(
            &mut a1,
            &task,
            &team_tasks::note_plaintext(&TaskNote {
                title: "Fix login".into(),
                name: "Alice".into(),
                ..TaskNote::default()
            }),
        );
        // A hold whose note waited for a key, then ended.
        b1.engine.holds.insert(
            task,
            Hold {
                holder: a1.holder.clone(),
                epoch: Epoch(1),
                since: UnixMillis(1),
                expires_at: None,
                expiring: false,
                unopened: Some(sealed.clone()),
                note: None,
                request: None,
            },
        );
        let alice = a1.holder.clone();
        let _ = b1.engine.hold_ended(task, &alice, None, false);
        let key = note_key(RecordKind::ReservationNote, &task);
        let mark = *b1.engine.live.notes.get(&(alice.clone(), key)).unwrap();
        let (_, counter) = sealed_header(&sealed).unwrap();
        assert_eq!(mark.counter, counter + 1);
        // Shown again: older than the mark.
        assert!(matches!(
            b1.engine.open_note(&task, &alice, &sealed),
            NoteOpen::Keep
        ));
    }

    #[test]
    fn a_forged_huge_header_counter_marks_only_up_to_the_clock() {
        let (_relay, mut a1, mut b1) = team_sharing();
        let task = KeyedHash::from_bytes([6; 32]);
        let note = team_tasks::note_plaintext(&TaskNote {
            title: "Fix login".into(),
            name: "Alice".into(),
            ..TaskNote::default()
        });
        a1.engine.counter = SendCounter::resume(u64::MAX / 2);
        let forged = forged_note(&mut a1, &task, &note);
        b1.engine.holds.insert(
            task,
            Hold {
                holder: a1.holder.clone(),
                epoch: Epoch(1),
                since: UnixMillis(1),
                expires_at: None,
                expiring: false,
                unopened: Some(forged),
                note: None,
                request: None,
            },
        );
        let alice = a1.holder.clone();
        let _ = b1.engine.hold_ended(task, &alice, None, false);
        let key = note_key(RecordKind::ReservationNote, &task);
        let ceiling = crate::team::counter_floor(1_000 + HEADER_COUNTER_SLACK_MS);
        let mark = *b1.engine.live.notes.get(&(alice.clone(), key)).unwrap();
        assert_eq!(mark.counter, ceiling + 1);
        // A note Alice seals once her clock is past the ceiling is newer than the mark (its
        // made-up task fails only the title check after that).
        a1.engine.counter = SendCounter::resume(ceiling + 10);
        let later = forged_note(&mut a1, &task, &note);
        let _ = b1.engine.open_note(&task, &alice, &later);
        let key = note_key(RecordKind::ReservationNote, &task);
        let mark = *b1.engine.live.notes.get(&(alice.clone(), key)).unwrap();
        assert_eq!(mark.counter, ceiling + 11);
    }

    #[test]
    fn a_resync_does_not_drop_the_mark_of_a_hold_that_comes_back_later_in_the_snapshot() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        let first = relay.task_events.last().cloned().unwrap();
        let alice = a1.holder.clone();
        let low = *b1
            .engine
            .live
            .notes
            .iter()
            .find(|((h, _), _)| *h == alice)
            .unwrap()
            .1;
        // 63 marks of ended notes, all newer than the running hold's: 64 marks in all.
        for n in 0..63u64 {
            let protected = b1.engine.running_notes(&alice);
            let stamp = Stamp {
                period: low.period,
                counter: low.counter + 1 + n,
            };
            b1.engine
                .live
                .note(&alice, &format!("old/{n}"), stamp, &protected);
        }
        // A newer hold Bob did not hear about, first in the snapshot after a resync.
        a1.engine.counter = SendCounter::resume(a1.engine.counter.last() + 1_000);
        reserve(&mut relay, &mut [&mut a1], 0, "Write docs", None);
        let second = relay.task_events.last().cloned().unwrap();
        resync_with(&mut relay, &mut [&mut a1, &mut b1], 1, &[second, first]);
        let tasks = b1.engine.activity().tasks;
        assert_eq!(tasks.len(), 2);
        assert!(tasks.iter().all(|t| !t.title.is_empty()), "{tasks:?}");
    }

    /// A hold of `holder` on `task` whose note `sealed` never opened.
    fn unopened_hold(holder: &Holder, sealed: Sealed) -> Hold {
        Hold {
            holder: holder.clone(),
            epoch: Epoch(1),
            since: UnixMillis(1),
            expires_at: None,
            expiring: false,
            unopened: Some(sealed),
            note: None,
            request: None,
        }
    }

    #[test]
    fn a_header_mark_needs_a_key_period_room_and_never_raises_the_floor() {
        let (_relay, mut a1, mut b1) = team_sharing();
        let note = team_tasks::note_plaintext(&TaskNote {
            title: "Fix login".into(),
            name: "Alice".into(),
            ..TaskNote::default()
        });
        // A device without any key period in the log: no mark (it would count in every period).
        let mallory = holder("mallory", "m1");
        let task = KeyedHash::from_bytes([7; 32]);
        let sealed = forged_note(&mut a1, &task, &note);
        b1.engine
            .holds
            .insert(task, unopened_hold(&mallory, sealed));
        let _ = b1.engine.hold_ended(task, &mallory, None, false);
        assert!(b1.engine.live.notes.keys().all(|(h, _)| *h != mallory));
        // Alice's marks full: a header mark takes no other's place and leaves the floor alone.
        let alice = a1.holder.clone();
        for n in 0..MAX_DETAIL_SESSIONS as u64 {
            let stamp = Stamp {
                period: 0,
                counter: 10 + n,
            };
            b1.engine
                .live
                .note(&alice, &format!("old/{n}"), stamp, &BTreeSet::new());
        }
        let before = (
            b1.engine.live.notes.clone(),
            b1.engine.live.note_floors.clone(),
        );
        let task = KeyedHash::from_bytes([8; 32]);
        let sealed = forged_note(&mut a1, &task, &note);
        b1.engine.holds.insert(task, unopened_hold(&alice, sealed));
        let _ = b1.engine.hold_ended(task, &alice, None, false);
        assert_eq!(
            (
                b1.engine.live.notes.clone(),
                b1.engine.live.note_floors.clone()
            ),
            before
        );
    }

    #[test]
    fn a_note_waiting_for_its_key_stays_with_its_hold_after_a_log_fetch() {
        let (_relay, mut a1, mut b1) = team_sharing();
        let task = KeyedHash::from_bytes([10; 32]);
        let mut sealed = forged_note(&mut a1, &task, &TaskNote::default().title.into_bytes());
        // An epoch this device holds no key for yet.
        sealed.epoch = Epoch(sealed.epoch.0 + 1);
        let alice = a1.holder.clone();
        b1.engine.holds.insert(task, unopened_hold(&alice, sealed));
        let _ = b1.engine.drain_tasks();
        assert!(b1.engine.holds[&task].unopened.is_some());
    }

    #[test]
    fn a_log_answer_inside_a_snapshot_does_not_end_the_holds_still_to_come() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        reserve(&mut relay, &mut [&mut a1, &mut b1], 0, "Fix login", None);
        let noted = relay.task_events.last().cloned().unwrap();
        let answer = relay.text(
            None,
            None,
            ServerMessage::MemberLogEntries(MemberLogEntries {
                entries: Vec::new(),
                more: false,
            }),
        );
        let mut answer = decode_server(&answer).unwrap();
        answer.seq = None;
        // The answer is the first event after `resync_required`, before the hold comes back.
        resync_with(
            &mut relay,
            &mut [&mut a1, &mut b1],
            1,
            &[encode_server(&answer).unwrap(), noted],
        );
        let tasks = b1.engine.activity().tasks;
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].title, "Fix login");
    }

    #[test]
    fn a_note_with_the_last_counter_of_all_is_never_shown() {
        let (_relay, mut a1, mut b1) = team_sharing();
        let task = KeyedHash::from_bytes([9; 32]);
        a1.engine.counter = SendCounter::resume(u64::MAX - 1);
        let note = team_tasks::note_plaintext(&TaskNote {
            title: "Fix login".into(),
            name: "Alice".into(),
            ..TaskNote::default()
        });
        let sealed = forged_note(&mut a1, &task, &note);
        let alice = a1.holder.clone();
        assert!(matches!(
            b1.engine.open_note(&task, &alice, &sealed),
            NoteOpen::Keep
        ));
        let key = note_key(RecordKind::ReservationNote, &task);
        assert!(!b1.engine.live.notes.contains_key(&(alice, key)));
    }

    #[test]
    fn conflicts_with_unknown_holders_fetch_the_log_at_most_every_few_seconds() {
        let (mut relay, mut a1, mut b1) = team_sharing();
        let mut fetches = Vec::new();
        for title in ["One", "Two"] {
            let effects = b1.engine.reserve_task(title, None, None).unwrap();
            let id = reserve_op(&effects);
            let refused = relay.text(
                Some(id),
                None,
                ServerMessage::Err(ErrorBody {
                    code: ErrorCode::Conflict,
                    retry_after_ms: None,
                    holder: Some(holder("mallory", "m1")),
                }),
            );
            let after = b1.engine.on_text(&refused);
            fetches.push(
                after
                    .iter()
                    .filter(|e| {
                        matches!(
                            e,
                            Effect::Send(ClientFrame {
                                message: ClientMessage::MemberLogGet(_),
                                ..
                            })
                        )
                    })
                    .count(),
            );
            run(&mut relay, &mut [&mut a1, &mut b1], vec![(1, after)]);
        }
        // The clock of the test stands still: the second conflict waits for the next ping.
        assert_eq!(fetches, [1, 0]);
        let ping = b1.engine.ping();
        assert!(ping.iter().any(|e| matches!(
            e,
            Effect::Send(ClientFrame {
                message: ClientMessage::MemberLogGet(_),
                ..
            })
        )));
    }

    // -----------------------------------------------------------------------------------------
    // The team's notes key ring (ADR 0006, phase 2)
    // -----------------------------------------------------------------------------------------

    fn notes_team() -> (Relay, Client, Client) {
        let mut relay = Relay::new();
        relay.proto = 2;
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

    fn ring_ids(client: &Client) -> Vec<String> {
        client
            .engine
            .notes_view()
            .ring
            .map(|ring| ring.ids().iter().map(|id| id.to_hex()).collect())
            .unwrap_or_default()
    }

    fn turn_on(relay: &mut Relay, a1: &mut Client, b1: &mut Client) {
        let effects = a1.engine.notes_enable(None).unwrap();
        run(relay, &mut [&mut *a1, &mut *b1], vec![(0, effects)]);
        assert_eq!(a1.engine.take_notes_outcome(), Some(Ok(())));
    }

    #[test]
    fn a_relay_of_version_one_is_never_asked_for_notes_keys() {
        let (relay, mut a1, b1) = team_of_two();
        assert_eq!(relay.notes_frames, 0);
        let view = a1.engine.notes_view();
        assert_eq!(view.speaks, Some(false));
        assert_eq!(view.exists, None);
        assert!(view.ring.is_none());
        assert_eq!(
            a1.engine.notes_enable(None).err(),
            Some(TeamError::NotesUnsupported)
        );
        assert_eq!(b1.engine.notes_view().speaks, Some(false));
        assert_eq!(relay.notes_frames, 0);
    }

    #[test]
    fn the_owner_turns_shared_notes_on_and_the_member_gets_the_ring() {
        let (mut relay, mut a1, mut b1) = notes_team();
        // Both asked once and learned there is no ring yet.
        assert_eq!(a1.engine.notes_view().exists, Some(false));
        assert_eq!(b1.engine.notes_view().exists, Some(false));
        assert!(a1.engine.notes_view().manager);
        assert!(!b1.engine.notes_view().manager);
        assert_eq!(
            b1.engine.notes_enable(None).err(),
            Some(TeamError::NotOwnerOrAdmin)
        );

        turn_on(&mut relay, &mut a1, &mut b1);
        let record = relay.notes_record.clone().expect("stored");
        assert_eq!(record.version, 1);
        assert_eq!(record.by, a1.holder);
        // The relay holds an opaque record, never the key.
        let key_hex = ring_ids(&a1);
        assert_eq!(key_hex.len(), 1);
        assert!(!record.content.data.as_str().contains(&key_hex[0]));
        // The hint made the member fetch and open it.
        assert_eq!(ring_ids(&b1), key_hex);
        assert_eq!(b1.engine.notes_view().exists, Some(true));
        assert!(!b1.engine.notes_view().renewing);
        // Turning on again changes nothing.
        let again = a1.engine.notes_enable(None).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, again)]);
        assert_eq!(a1.engine.take_notes_outcome(), Some(Ok(())));
        assert_eq!(relay.notes_record.as_ref().map(|r| r.version), Some(1));
    }

    #[test]
    fn a_removal_reseals_the_ring_with_a_new_key_and_a_later_member_reads_every_key() {
        let (mut relay, mut a1, mut b1) = notes_team();
        turn_on(&mut relay, &mut a1, &mut b1);
        let before = ring_ids(&a1);
        let bobs = ring_ids(&b1);
        assert_eq!(bobs, before);

        // The website no longer lists bob: removal, rotation, and the ring under the new epoch
        // with one more key.
        let _ = a1.engine.ping();
        let alice = MemberId::new("alice").unwrap();
        let effects = a1.engine.set_website(Website {
            role: Role::Owner,
            owner: Some(alice.clone()),
            members: Some(vec![WebsiteMember {
                member: alice,
                role: Some(Role::Owner),
            }]),
        });
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        assert_eq!(connected(&a1).0, 2);
        assert_eq!(b1.ended, Some(SessionEnd::Stop(StopReason::Removed)));
        let after = ring_ids(&a1);
        assert_eq!(after.len(), 2);
        assert_eq!(after[0], before[0]);
        let record = relay.notes_record.clone().expect("stored");
        assert_eq!(record.content.epoch, Epoch(2));
        assert_eq!(record.version, 2);
        assert!(!a1.engine.notes_view().renewing);
        // Bob's device never got the new key.
        assert!(!ring_ids(&b1).contains(&after[1]));

        // Carol, admitted afterwards with only epoch 2's key, reads every key.
        let _ = a1.engine.ping();
        let listed = a1.engine.set_website(website(
            Role::Owner,
            Some(listing(&[("alice", Role::Owner), ("carol", Role::Member)])),
        ));
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, listed)]);
        let mut c1 = carol_joins(&mut relay, &mut a1, &mut b1);
        let effects = c1.engine.ping();
        run(
            &mut relay,
            &mut [&mut a1, &mut b1, &mut c1],
            vec![(2, effects)],
        );
        assert_eq!(ring_ids(&c1), after);
    }

    #[test]
    fn while_the_relay_refuses_the_renewal_the_ring_shows_as_renewing() {
        let (mut relay, mut a1, mut b1) = notes_team();
        turn_on(&mut relay, &mut a1, &mut b1);
        relay.refuse_notes = true;
        let _ = a1.engine.ping();
        let alice = MemberId::new("alice").unwrap();
        let effects = a1.engine.set_website(Website {
            role: Role::Owner,
            owner: Some(alice.clone()),
            members: Some(vec![WebsiteMember {
                member: alice,
                role: Some(Role::Owner),
            }]),
        });
        let sets_before = relay.notes_frames;
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        assert_eq!(connected(&a1).0, 2);
        let view = a1.engine.notes_view();
        assert!(view.renewing, "the ring is still sealed under epoch 1");
        // The new key was never stored, so it is not used.
        assert_eq!(ring_ids(&a1).len(), 1);
        // Bounded: the refusals do not loop.
        assert!(relay.notes_frames - sets_before <= 10);
    }

    #[test]
    fn a_solo_ring_joins_the_team_ring_also_after_a_lost_race() {
        let (mut relay, mut a1, mut b1) = notes_team();
        turn_on(&mut relay, &mut a1, &mut b1);
        let team_key = ring_ids(&a1);
        let solo = hangar_team_crypto::notes::KeyRing::generate(&mut OsEntropy).unwrap();
        let solo_key = solo.ids()[0].to_hex();
        relay.notes_race = true;
        let effects = a1.engine.notes_enable(Some(solo)).unwrap();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        assert_eq!(a1.engine.take_notes_outcome(), Some(Ok(())));
        let ring = ring_ids(&a1);
        // The team's key, the solo key and a new key for what follows.
        assert_eq!(ring.len(), 3);
        assert!(ring.contains(&team_key[0]) && ring.contains(&solo_key));
        assert_eq!(ring_ids(&b1), ring);
    }

    #[test]
    fn a_member_never_accepts_a_ring_a_member_wrote() {
        let (mut relay, mut a1, mut b1) = notes_team();
        turn_on(&mut relay, &mut a1, &mut b1);
        // A relay that lets a member's device write: the record is signed and sealed, but bob is
        // no owner or admin, so alice keeps her ring and does not take his key.
        let state = b1.engine.verifier.state().unwrap().clone();
        let secret = b1.engine.keys.epochs.get(&Epoch(1)).unwrap().clone();
        let cipher = EpochCipher::for_epoch(&state, &secret, Epoch(1)).unwrap();
        let forged = hangar_team_crypto::notes::KeyRing::generate(&mut OsEntropy).unwrap();
        let sealed = hangar_team_crypto::notes::seal_notes_keys(
            &cipher,
            &b1.holder,
            &b1.engine.identity,
            state.head(),
            u64::MAX / 2,
            &forged,
            &mut OsEntropy,
        )
        .unwrap();
        relay.notes_record = Some(hangar_team_protocol::wire::NotesKeysRecord {
            version: 7,
            by: b1.holder.clone(),
            content: sealed,
        });
        let mine = ring_ids(&a1);
        let effects = a1.engine.on_notes_updated();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        let now = ring_ids(&a1);
        assert!(!now.contains(&forged.ids()[0].to_hex()));
        assert!(mine.iter().all(|id| now.contains(id)));
        // Alice is an owner: she writes her ring over the member's record.
        let record = relay.notes_record.clone().unwrap();
        assert_eq!(record.by, a1.holder);
        assert_eq!(record.version, 8);
    }

    fn remove_bob(relay: &mut Relay, a1: &mut Client, b1: &mut Client) {
        let _ = a1.engine.ping();
        let alice = MemberId::new("alice").unwrap();
        let effects = a1.engine.set_website(Website {
            role: Role::Owner,
            owner: Some(alice.clone()),
            members: Some(vec![WebsiteMember {
                member: alice,
                role: Some(Role::Owner),
            }]),
        });
        run(relay, &mut [&mut *a1, &mut *b1], vec![(0, effects)]);
    }

    /// Seals `ring` as `client`'s `notes-keys` record under epoch 1, naming `head`.
    fn ring_record(
        client: &Client,
        head: &ChainHead,
        ring: &hangar_team_crypto::notes::KeyRing,
    ) -> hangar_team_protocol::wire::NotesKeysRecord {
        let state = client.engine.verifier.state().unwrap().clone();
        let secret = client.engine.keys.epochs.get(&Epoch(1)).unwrap().clone();
        let cipher = EpochCipher::for_epoch(&state, &secret, Epoch(1)).unwrap();
        let content = hangar_team_crypto::notes::seal_notes_keys(
            &cipher,
            &client.holder,
            &client.engine.identity,
            head,
            u64::MAX / 2,
            ring,
            &mut OsEntropy,
        )
        .unwrap();
        hangar_team_protocol::wire::NotesKeysRecord {
            version: 9,
            by: client.holder.clone(),
            content,
        }
    }

    #[test]
    fn a_relay_that_lost_the_ring_still_gets_a_new_key_after_a_removal() {
        let (mut relay, mut a1, mut b1) = notes_team();
        turn_on(&mut relay, &mut a1, &mut b1);
        let before = ring_ids(&a1);
        // The relay loses the record (or says it never had one) before the removal.
        relay.notes_record = None;
        remove_bob(&mut relay, &mut a1, &mut b1);
        assert_eq!(connected(&a1).0, 2);
        let record = relay.notes_record.clone().expect("stored");
        assert_eq!(record.content.epoch, Epoch(2));
        let after = ring_ids(&a1);
        // The ring under epoch 2 must carry a key bob never had: notes written from now on are
        // closed to him.
        assert_eq!(after.len(), 2, "no new key after the rotation");
        assert!(!before.contains(&after[1]));
        assert!(!a1.engine.notes_view().renewing);
    }

    #[test]
    fn while_a_removal_waits_for_its_rotation_the_ring_shows_as_renewing() {
        let (mut relay, mut a1, mut b1) = notes_team();
        turn_on(&mut relay, &mut a1, &mut b1);
        assert!(!a1.engine.notes_view().renewing);
        // Bob is removed, but no rotation reaches the relay (as when no owner or admin is
        // online to rotate): the current key is still in bob's hands.
        relay.refuse_rotate = true;
        remove_bob(&mut relay, &mut a1, &mut b1);
        assert!(a1.engine.verifier.state().unwrap().rotation_urgent());
        assert!(
            a1.engine.notes_view().renewing,
            "notes must not be written with a key the removed member holds"
        );
    }

    #[test]
    fn a_notes_keys_record_ahead_of_the_log_does_not_make_the_device_fetch_the_log_forever() {
        let (mut relay, mut a1, mut b1) = notes_team();
        turn_on(&mut relay, &mut a1, &mut b1);
        // A record naming a log entry that does not exist (anyone can write such a head: it is
        // checked before the signature).
        let head = *a1.engine.verifier.state().unwrap().head();
        let far = ChainHead {
            index: head.index + 50,
            hash: head.hash,
        };
        let ring = hangar_team_crypto::notes::KeyRing::generate(&mut OsEntropy).unwrap();
        relay.notes_record = Some(ring_record(&a1, &far, &ring));
        let effects = a1.engine.on_notes_updated();
        // `run` panics when the clients never settle.
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        assert!(!ring_ids(&a1).contains(&ring.ids()[0].to_hex()));
    }

    #[test]
    fn a_notes_keys_record_with_another_log_history_marks_its_sender_as_diverged() {
        let (mut relay, mut a1, mut b1) = notes_team();
        turn_on(&mut relay, &mut a1, &mut b1);
        // Bob's device signs a record naming another hash at an entry both verified: the relay
        // showed the two devices different logs.
        let head = *b1.engine.verifier.state().unwrap().head();
        let forked = ChainHead {
            index: head.index,
            hash: hangar_team_protocol::e2e::LogHash::from_bytes([7; 32]),
        };
        let ring = hangar_team_crypto::notes::KeyRing::generate(&mut OsEntropy).unwrap();
        relay.notes_record = Some(ring_record(&b1, &forked, &ring));
        let effects = a1.engine.on_notes_updated();
        run(&mut relay, &mut [&mut a1, &mut b1], vec![(0, effects)]);
        assert!(a1.engine.divergent.contains_key(&b1.holder));
    }
}
