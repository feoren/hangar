//! Persisting a [`ReservationBook`] across relay restarts: [`ReservationBook::export`] writes a
//! versioned [`BookSnapshot`], [`ReservationBook::restore`] checks one and rebuilds the book.
//!
//! What is kept: every hold with its epoch, `since`, last activity, offline time and warning,
//! open release requests, cooldowns, the book's epoch and the start of a running rekey grace
//! (so holds of an older epoch keep their grace). Deadlines are not stored: they follow from
//! the last activity, the presence and the [`Policy`] passed to `restore`, so a changed policy
//! applies to restored holds too.
//!
//! What is not kept: connection counts. Connections do not survive a restart, so after
//! `restore` every holder counts as offline until its device connects again and the relay
//! reports it with [`ReservationBook::presence`] as usual.
//!
//! Effect on expiry: the restart counts as every holder disconnecting at the restore's `now`.
//! A hold's deadline becomes `max(last_activity + inactivity, now + warn_before)`: a hold of a
//! connected holder starts to run (with the usual warning first), and the downtime never
//! expires a hold without a warning, since nobody could reach the relay to keep it. Release
//! requests and the rekey grace run on wall-clock time: one that ran out while the relay was
//! down ends at the restore (`timedOut`, or the old-epoch hold expires). The requester may ask
//! again once the usual cooldown, counted from the timeout, has passed; the grace is already
//! long.
//!
//! Consistency with the event log: the relay stores the snapshot and the log's position
//! ([`crate::log::EventLog::position`]) in one write, and no frame with a `seq` leaves before a
//! write whose head covers that `seq` is durable. This holds for every logged event, including
//! the ones that change nothing in the book (`presence.update`, `device.removed`): a stored head
//! behind a sent `seq` would hand that `seq` to a new event after the restart, and a client that
//! saw the old one drops the new one as a duplicate. Then a client at the stored head missed
//! nothing, and [`crate::log::EventLog::resume`] lets it go on without a snapshot. A relay that
//! cannot promise this starts a new stream instead (every client resyncs).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::{
    reserved, BookEvents, Hold, Policy, ReleaseRequest, Reservation, ReservationBook,
    ReservationState,
};
use crate::hash::KeyedHash;
use crate::ids::{Epoch, MemberId, UnixMillis};
use crate::limits::MAX_RESERVATIONS_PER_MEMBER;
use crate::wire::Holder;

/// The snapshot format this crate writes and reads. A change to [`BookSnapshot`]'s shape needs
/// a new version; [`ReservationBook::restore`] refuses every other one.
pub const BOOK_SNAPSHOT_VERSION: u16 = 1;

/// A [`ReservationBook`] as it is stored (JSON through serde). Holds only: free and expired
/// tasks carry nothing a restart needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BookSnapshot {
    /// [`BOOK_SNAPSHOT_VERSION`] when written by this crate.
    pub version: u16,
    /// The key epoch whose hashes may be reserved.
    pub epoch: Epoch,
    /// The last key rotation, while holds of earlier epochs may remain.
    pub rotated_at: Option<UnixMillis>,
    /// One per held task, ordered by task hash (so equal books give equal snapshots).
    pub entries: Vec<EntrySnapshot>,
}

/// One held task in a [`BookSnapshot`]. The state is encoded by the optional fields, so a state
/// without a hold cannot be written: neither `warnedAt` nor `request` means held, `warnedAt`
/// alone expiring, `request` a release request (warned or not).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EntrySnapshot {
    pub task: KeyedHash,
    pub holder: Holder,
    /// The epoch of the key the task's hash was made with.
    pub epoch: Epoch,
    pub since: UnixMillis,
    pub last_activity: UnixMillis,
    /// `None` while the holder's device was connected when the snapshot was taken.
    pub offline_since: Option<UnixMillis>,
    /// When the holder was warned of the expiry.
    pub warned_at: Option<UnixMillis>,
    /// The open release request.
    pub request: Option<ReleaseRequest>,
    /// Members who may not ask for this task again before `until`.
    pub cooldowns: Vec<CooldownSnapshot>,
}

/// A member's cooldown on one task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CooldownSnapshot {
    pub member: MemberId,
    pub until: UnixMillis,
}

/// Why a snapshot was not restored. The relay then starts with an empty book and a new stream,
/// so clients resync instead of trusting a state that may be wrong. The texts name the entry
/// by its position, never its content.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RestoreError {
    #[error("snapshot version {got} is not the supported version {supported}")]
    Version { got: u64, supported: u16 },
    #[error("the snapshot is not valid (line {line}, column {column})")]
    Malformed { line: usize, column: usize },
    #[error("snapshot entry {index}: {problem}")]
    Inconsistent { index: usize, problem: &'static str },
}

/// Read first and on its own, so a snapshot of another version is told so even when its shape
/// differs.
#[derive(Deserialize)]
struct VersionProbe {
    version: u64,
}

impl BookSnapshot {
    /// Parses a stored snapshot: the version first (any other is [`RestoreError::Version`]),
    /// then the fields (unknown ones refused). Writing is `serde_json::to_string`.
    pub fn from_json(text: &str) -> Result<BookSnapshot, RestoreError> {
        let malformed = |e: serde_json::Error| RestoreError::Malformed {
            line: e.line(),
            column: e.column(),
        };
        let probe: VersionProbe = serde_json::from_str(text).map_err(malformed)?;
        if probe.version != u64::from(BOOK_SNAPSHOT_VERSION) {
            return Err(RestoreError::Version {
                got: probe.version,
                supported: BOOK_SNAPSHOT_VERSION,
            });
        }
        serde_json::from_str(text).map_err(malformed)
    }
}

/// A restored book and the events the restart caused, for the relay to log and send in this
/// order: `task.reserved` for every hold whose deadline changed (it starts to run, or the
/// downtime moved it), then what time did at the restore's `now` (warnings, timed out release
/// requests, old-epoch holds whose grace ended).
#[derive(Debug, Clone)]
pub struct Restored {
    pub book: ReservationBook,
    pub events: BookEvents,
}

fn inconsistent(index: usize, problem: &'static str) -> RestoreError {
    RestoreError::Inconsistent { index, problem }
}

impl ReservationBook {
    /// The book as a [`BookSnapshot`], for storage. Applies no time: holds whose deadline has
    /// passed are written as they are, and `restore` handles them.
    pub fn export(&self) -> BookSnapshot {
        let mut entries: Vec<EntrySnapshot> = self
            .entries
            .iter()
            .filter_map(|(task, reservation)| entry_snapshot(*task, reservation))
            .collect();
        entries.sort_by_key(|entry| entry.task);
        BookSnapshot {
            version: BOOK_SNAPSHOT_VERSION,
            epoch: self.epoch,
            rotated_at: self.rotated_at,
            entries,
        }
    }

    /// Rebuilds a book from `snapshot` under `policy` at `now` (module docs: every holder counts
    /// as offline from `now`). Refused, and nothing restored, when the version is not
    /// [`BOOK_SNAPSHOT_VERSION`] or the snapshot is inconsistent: a task twice, a hold of a
    /// newer epoch than the book's, a hold of an older epoch without a rotation, `since` after
    /// the last activity, a release request by the holder itself or older than the hold, or
    /// more holds of one member than [`MAX_RESERVATIONS_PER_MEMBER`]. Cooldowns that ended
    /// before `now` are dropped.
    pub fn restore(
        snapshot: BookSnapshot,
        policy: Policy,
        now: UnixMillis,
    ) -> Result<Restored, RestoreError> {
        if snapshot.version != BOOK_SNAPSHOT_VERSION {
            return Err(RestoreError::Version {
                got: u64::from(snapshot.version),
                supported: BOOK_SNAPSHOT_VERSION,
            });
        }
        let mut book = ReservationBook::new(policy, snapshot.epoch);
        book.rotated_at = snapshot.rotated_at;
        let mut held: HashMap<MemberId, usize> = HashMap::new();
        let mut events = Vec::new();
        for (index, entry) in snapshot.entries.into_iter().enumerate() {
            if book.entries.contains_key(&entry.task) {
                return Err(inconsistent(index, "the task appears twice"));
            }
            if entry.epoch > book.epoch {
                return Err(inconsistent(
                    index,
                    "the hold's epoch is newer than the book's",
                ));
            }
            if entry.epoch < book.epoch && book.rotated_at.is_none() {
                return Err(inconsistent(
                    index,
                    "a hold of an older epoch without a rotation",
                ));
            }
            if entry.since > entry.last_activity {
                return Err(inconsistent(
                    index,
                    "the hold starts after its last activity",
                ));
            }
            if let Some(request) = &entry.request {
                if request.requester == entry.holder {
                    return Err(inconsistent(index, "the holder asks itself for a release"));
                }
                if request.at < entry.since {
                    return Err(inconsistent(index, "the release request predates the hold"));
                }
            }
            let count = held.entry(entry.holder.member.clone()).or_default();
            *count += 1;
            if *count > MAX_RESERVATIONS_PER_MEMBER {
                return Err(inconsistent(index, "more holds of one member than allowed"));
            }

            let mut hold = Hold {
                holder: entry.holder,
                epoch: entry.epoch,
                since: entry.since,
                last_activity: entry.last_activity,
                offline_since: entry.offline_since,
            };
            let before = hold.deadline(&policy);
            hold.offline_since = Some(now);
            if hold.deadline(&policy) != before {
                events.push((entry.task, reserved(&hold, &policy)));
            }
            let state = match (entry.request, entry.warned_at) {
                (Some(request), warned_at) => ReservationState::ReleaseRequested {
                    hold,
                    request,
                    warned_at,
                },
                (None, Some(warned_at)) => ReservationState::Expiring { hold, warned_at },
                (None, None) => ReservationState::Held { hold },
            };
            let cooldowns = entry
                .cooldowns
                .into_iter()
                .filter(|cooldown| cooldown.until > now)
                .map(|cooldown| (cooldown.member, cooldown.until))
                .collect();
            book.entries
                .insert(entry.task, Reservation { state, cooldowns });
        }
        events.extend(book.advance_all(now));
        Ok(Restored { book, events })
    }
}

fn entry_snapshot(task: KeyedHash, reservation: &Reservation) -> Option<EntrySnapshot> {
    let (hold, warned_at, request) = match &reservation.state {
        ReservationState::Held { hold } => (hold, None, None),
        ReservationState::Expiring { hold, warned_at } => (hold, Some(*warned_at), None),
        ReservationState::ReleaseRequested {
            hold,
            request,
            warned_at,
        } => (hold, *warned_at, Some(request.clone())),
        ReservationState::Free | ReservationState::Expired { .. } => return None,
    };
    Some(EntrySnapshot {
        task,
        holder: hold.holder.clone(),
        epoch: hold.epoch,
        since: hold.since,
        last_activity: hold.last_activity,
        offline_since: hold.offline_since,
        warned_at,
        request,
        cooldowns: reservation
            .cooldowns
            .iter()
            .map(|(member, until)| CooldownSnapshot {
                member: member.clone(),
                until: *until,
            })
            .collect(),
    })
}
