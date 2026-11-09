//! Frames of the WebSocket protocol (team service plan, section 4): one JSON object per text
//! message.
//!
//! Client frame: `{"v":1,"id":"<op id>","team":"<team>","type":"task.reserve","body":{..}}`.
//! Server frame: `{"v":1,"id":"<op id answered>","seq":12,"team":"<team>","type":"..","body":{..}}`.
//!
//! Decoding checks in this order, so every failure has one clear cause: the frame's size, the
//! version (read on its own first and tolerantly, so a frame of another version gets a version
//! error even when it has fields this version does not know), the envelope (unknown and
//! duplicate fields rejected), the body (parsed straight into the message's type: unknown and
//! duplicate fields rejected at every level), and the limits of lists and ciphertexts.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::hash::{KeyedHash, PathHashes};
use crate::ids::{
    BlobId, Ciphertext, DeviceId, Epoch, MemberId, OpId, Sealed, Seq, SessionKey, StreamId, TeamId,
    Ticket, UnixMillis,
};
use crate::limits::{
    MAX_CIPHERTEXT_LEN, MAX_CLAIM_FILES, MAX_FRAME_BYTES, MAX_HEARTBEAT_TASKS,
    MAX_INLINE_CIPHERTEXT_LEN, MAX_PREV_HASHES,
};
use crate::PROTOCOL_VERSION;

/// Why a frame was not accepted. The server answers each with an `err` code
/// ([`DecodeError::code`]); the text never contains the frame's content.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecodeError {
    #[error("the frame is larger than {max} bytes")]
    TooLarge { max: usize },
    #[error("the frame is not a valid envelope: {0}")]
    Envelope(String),
    #[error("protocol version {got} is not supported (this side speaks {supported})")]
    Version { got: u64, supported: u16 },
    #[error("the body of `{kind}` is not valid: {reason}")]
    Body { kind: &'static str, reason: String },
    #[error("`{kind}` needs a team")]
    MissingTeam { kind: &'static str },
    #[error("server event `{kind}` {problem}")]
    Sequence {
        kind: &'static str,
        problem: &'static str,
    },
    #[error("`{field}` is over its limit of {max}")]
    Limit { field: &'static str, max: usize },
}

impl DecodeError {
    /// The `err` code the server sends for this failure.
    pub fn code(&self) -> ErrorCode {
        match self {
            DecodeError::TooLarge { .. } | DecodeError::Limit { .. } => ErrorCode::Limit,
            DecodeError::Version { got, supported } if *got > u64::from(*supported) => {
                ErrorCode::BadRequest
            }
            DecodeError::Version { .. } => ErrorCode::OutdatedClient,
            _ => ErrorCode::BadRequest,
        }
    }

    /// On the client: the server speaks a newer protocol, so Hangar needs an update.
    pub fn needs_update(&self) -> bool {
        matches!(self, DecodeError::Version { got, supported } if *got > u64::from(*supported))
    }
}

/// A frame could not be built (a value over a limit; never sent).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum EncodeError {
    #[error("`{field}` is over its limit of {max}")]
    Limit { field: &'static str, max: usize },
    #[error("`{kind}` needs a team")]
    MissingTeam { kind: &'static str },
    #[error("the frame is larger than {max} bytes")]
    TooLarge { max: usize },
    #[error("the frame could not be serialized: {0}")]
    Json(String),
}

/// `min` is above `max`.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("a protocol range needs min <= max")]
pub struct InvalidRange;

/// The protocol versions one side speaks, both ends included; always `min <= max`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "RawRange", into = "RawRange")]
pub struct ProtoRange {
    min: u16,
    max: u16,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRange {
    min: u16,
    max: u16,
}

impl TryFrom<RawRange> for ProtoRange {
    type Error = InvalidRange;
    fn try_from(raw: RawRange) -> Result<Self, Self::Error> {
        ProtoRange::new(raw.min, raw.max)
    }
}

impl From<ProtoRange> for RawRange {
    fn from(range: ProtoRange) -> RawRange {
        RawRange {
            min: range.min,
            max: range.max,
        }
    }
}

impl ProtoRange {
    /// Exactly the version this crate speaks.
    pub const CURRENT: ProtoRange = ProtoRange {
        min: PROTOCOL_VERSION,
        max: PROTOCOL_VERSION,
    };

    /// Checks `min <= max`.
    pub fn new(min: u16, max: u16) -> Result<Self, InvalidRange> {
        if min > max {
            return Err(InvalidRange);
        }
        Ok(Self { min, max })
    }

    /// The oldest version.
    pub fn min(&self) -> u16 {
        self.min
    }

    /// The newest version.
    pub fn max(&self) -> u16 {
        self.max
    }

    /// Whether `version` is in the range.
    pub fn contains(&self, version: u16) -> bool {
        (self.min..=self.max).contains(&version)
    }

    /// The highest version both sides speak (`self` is the client). `None`: no common version;
    /// the server answers `outdatedClient` when the client's newest version is older than the
    /// server's oldest, and `badRequest` otherwise (a client newer than the server).
    pub fn negotiate(self, server: ProtoRange) -> Option<u16> {
        let high = self.max.min(server.max);
        let low = self.min.max(server.min);
        (low <= high).then_some(high)
    }
}

/// The server chose a version the client did not offer.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("the server chose protocol version {chosen}, outside what this side speaks")]
pub struct UnexpectedVersion {
    pub chosen: u16,
}

/// The client's check of a `welcome`: the version the server chose must be one the client
/// offered in `hello.proto`.
pub fn check_welcome(own: ProtoRange, welcome: &Welcome) -> Result<(), UnexpectedVersion> {
    if own.contains(welcome.proto) {
        Ok(())
    } else {
        Err(UnexpectedVersion {
            chosen: welcome.proto,
        })
    }
}

/// Where a client stopped reading a team's event log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cursor {
    pub stream: StreamId,
    pub seq: Seq,
}

/// A device of a member: who holds a reservation, who is present. The relay takes `member` from
/// the verified ticket's subject and accepts `device` only if the website lists it for that
/// member, so one member can never speak for another member's device.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Holder {
    pub member: MemberId,
    pub device: DeviceId,
}

/// A device's presence, as `presence.set` sets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PresenceState {
    /// Hangar runs and an agent works.
    Active,
    /// Hangar runs, nothing happens.
    Idle,
    /// The person said they are away.
    Away,
}

/// What an encrypted blob holds, so the server can apply retention per kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BlobKind {
    Decision,
    Task,
    Transcript,
}

// ---------------------------------------------------------------------------------------------
// Client messages
// ---------------------------------------------------------------------------------------------

/// First frame per team on a connection. The server answers `welcome` (and replays) or
/// `resync_required`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Hello {
    pub ticket: Ticket,
    pub device: DeviceId,
    pub proto: ProtoRange,
    /// `None` on the first connection or after a lost state: the server sends a snapshot.
    pub last_seq: Option<Cursor>,
}

/// `presence.set`: the device's presence and, encrypted, what its agents do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresenceSet {
    pub state: PresenceState,
    /// What the agents do, readable by the team only.
    pub status: Option<Sealed>,
}

/// Activity on reserved tasks (the agent changed files): extends their reservations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Heartbeat {
    pub tasks: Vec<KeyedHash>,
}

/// `task.reserve`: ask to hold a task; the first one wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskReserve {
    pub task: KeyedHash,
    pub epoch: Epoch,
    /// The same task's hashes under the epochs before `epoch` whose keys the client still has
    /// (newest first, at most [`MAX_PREV_HASHES`]). The relay checks them against holds that
    /// were not rekeyed yet, so a holder who is offline during a key rotation keeps the task.
    pub prev: Vec<KeyedHash>,
    pub note: Option<Sealed>,
}

/// Names one task (`task.release`, `task.release_withdraw`, `task.force_release`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRef {
    pub task: KeyedHash,
}

/// `task.release_request`: ask the holder to hand a task over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskReleaseRequest {
    pub task: KeyedHash,
    pub note: Option<Sealed>,
}

/// `task.release_answer`: the holder accepts or declines a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskReleaseAnswer {
    pub task: KeyedHash,
    pub requester: MemberId,
    pub accept: bool,
}

/// After `key.rotated`, the holder moves its reservation to the hash under the new epoch's key;
/// nobody else can, since only the holder knows which old hash belongs to which task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskRekey {
    pub from: KeyedHash,
    pub to: KeyedHash,
    pub epoch: Epoch,
}

/// Replaces everything this session claims. A session works in one repository; a `claim.set`
/// with another `repo` replaces the claims of the previous one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimSet {
    pub session: SessionKey,
    pub repo: KeyedHash,
    pub epoch: Epoch,
    pub files: Vec<PathHashes>,
    /// More files changed than [`MAX_CLAIM_FILES`]; the rest is not listed.
    pub truncated: bool,
    /// Base commit, paths and line ranges for the team's clients, which refine "same file" to
    /// "overlapping lines" locally. The server never reads it; it passes it on in
    /// `claim.overlap` to sessions that share a file with this one.
    pub detail: Option<Sealed>,
}

/// `claim.clear`: the session ended or changes nothing any more.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimClear {
    pub session: SessionKey,
}

/// `blob.put`: store an encrypted blob; the `ack` names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlobPut {
    pub kind: BlobKind,
    pub content: Sealed,
}

/// `blob.get`: fetch a stored blob; the `ack` carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlobGet {
    pub blob: BlobId,
}

/// Everything a client sends. `type` on the wire is the serde name; [`CLIENT_KINDS`] lists them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "body")]
pub enum ClientMessage {
    #[serde(rename = "hello")]
    Hello(Hello),
    #[serde(rename = "presence.set")]
    PresenceSet(PresenceSet),
    #[serde(rename = "heartbeat")]
    Heartbeat(Heartbeat),
    #[serde(rename = "task.reserve")]
    TaskReserve(TaskReserve),
    #[serde(rename = "task.release")]
    TaskRelease(TaskRef),
    #[serde(rename = "task.release_request")]
    TaskReleaseRequest(TaskReleaseRequest),
    /// The requester takes its open release request back.
    #[serde(rename = "task.release_withdraw")]
    TaskReleaseWithdraw(TaskRef),
    #[serde(rename = "task.release_answer")]
    TaskReleaseAnswer(TaskReleaseAnswer),
    /// An owner or admin (the role comes from the ticket) ends someone else's hold.
    #[serde(rename = "task.force_release")]
    TaskForceRelease(TaskRef),
    #[serde(rename = "task.rekey")]
    TaskRekey(TaskRekey),
    #[serde(rename = "claim.set")]
    ClaimSet(ClaimSet),
    #[serde(rename = "claim.clear")]
    ClaimClear(ClaimClear),
    #[serde(rename = "blob.put")]
    BlobPut(BlobPut),
    #[serde(rename = "blob.get")]
    BlobGet(BlobGet),
    /// Keeps the connection alive; answered with `ack`.
    #[serde(rename = "ping")]
    Ping,
}

/// A payload bound to an older (or newer) key epoch than the team's current one.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("`{kind}` carries epoch {got:?}, the team is at {current:?}")]
pub struct StaleEpoch {
    pub kind: &'static str,
    pub got: Epoch,
    pub current: Epoch,
}

impl StaleEpoch {
    /// The `err` code the relay answers with.
    pub fn code(&self) -> ErrorCode {
        ErrorCode::StaleEpoch
    }
}

impl ClientMessage {
    /// The `type` on the wire.
    pub fn kind(&self) -> &'static str {
        match self {
            ClientMessage::Hello(_) => "hello",
            ClientMessage::PresenceSet(_) => "presence.set",
            ClientMessage::Heartbeat(_) => "heartbeat",
            ClientMessage::TaskReserve(_) => "task.reserve",
            ClientMessage::TaskRelease(_) => "task.release",
            ClientMessage::TaskReleaseRequest(_) => "task.release_request",
            ClientMessage::TaskReleaseWithdraw(_) => "task.release_withdraw",
            ClientMessage::TaskReleaseAnswer(_) => "task.release_answer",
            ClientMessage::TaskForceRelease(_) => "task.force_release",
            ClientMessage::TaskRekey(_) => "task.rekey",
            ClientMessage::ClaimSet(_) => "claim.set",
            ClientMessage::ClaimClear(_) => "claim.clear",
            ClientMessage::BlobPut(_) => "blob.put",
            ClientMessage::BlobGet(_) => "blob.get",
            ClientMessage::Ping => "ping",
        }
    }

    /// Only `ping` belongs to the connection; everything else to one team.
    fn needs_team(&self) -> bool {
        !matches!(self, ClientMessage::Ping)
    }

    /// Every epoch the payload is bound to: hashes made with an epoch's matching key and content
    /// sealed with its sealing key. `prev` of `task.reserve` is old on purpose and not listed.
    pub fn epochs(&self) -> Vec<Epoch> {
        let sealed = |value: Option<&Sealed>| value.map(|s| s.epoch);
        match self {
            ClientMessage::PresenceSet(m) => sealed(m.status.as_ref()).into_iter().collect(),
            ClientMessage::TaskReserve(m) => std::iter::once(m.epoch)
                .chain(sealed(m.note.as_ref()))
                .collect(),
            ClientMessage::TaskReleaseRequest(m) => sealed(m.note.as_ref()).into_iter().collect(),
            ClientMessage::TaskRekey(m) => vec![m.epoch],
            ClientMessage::ClaimSet(m) => std::iter::once(m.epoch)
                .chain(sealed(m.detail.as_ref()))
                .collect(),
            ClientMessage::BlobPut(m) => vec![m.content.epoch],
            ClientMessage::Hello(_)
            | ClientMessage::Heartbeat(_)
            | ClientMessage::TaskRelease(_)
            | ClientMessage::TaskReleaseWithdraw(_)
            | ClientMessage::TaskReleaseAnswer(_)
            | ClientMessage::TaskForceRelease(_)
            | ClientMessage::ClaimClear(_)
            | ClientMessage::BlobGet(_)
            | ClientMessage::Ping => Vec::new(),
        }
    }

    /// The relay's check before it stores or passes on anything: every epoch the payload names
    /// is the team's current one. Hashes and ciphertexts of an old epoch are made with keys a
    /// removed member still has.
    pub fn check_epoch(&self, current: Epoch) -> Result<(), StaleEpoch> {
        match self.epochs().into_iter().find(|epoch| *epoch != current) {
            Some(got) => Err(StaleEpoch {
                kind: self.kind(),
                got,
                current,
            }),
            None => Ok(()),
        }
    }

    /// SHA-256 of the message's canonical JSON, so the relay can tell a retry (same op id, same
    /// body) from a different intent that reuses an op id.
    pub fn digest(&self) -> [u8; 32] {
        // Serializing these types cannot fail (no maps with non-string keys, no custom
        // serializers that error); an empty input would still give a stable digest.
        let json = serde_json::to_vec(self).unwrap_or_default();
        Sha256::digest(&json).into()
    }

    fn check_limits(&self) -> Result<(), (&'static str, usize)> {
        match self {
            ClientMessage::PresenceSet(m) => inline("status", m.status.as_ref()),
            ClientMessage::Heartbeat(m) => count("tasks", m.tasks.len(), MAX_HEARTBEAT_TASKS),
            ClientMessage::TaskReserve(m) => {
                count("prev", m.prev.len(), MAX_PREV_HASHES)?;
                inline("note", m.note.as_ref())
            }
            ClientMessage::TaskReleaseRequest(m) => inline("note", m.note.as_ref()),
            ClientMessage::ClaimSet(m) => {
                count("files", m.files.len(), MAX_CLAIM_FILES)?;
                sealed("detail", m.detail.as_ref(), MAX_CIPHERTEXT_LEN)
            }
            ClientMessage::BlobPut(m) => sealed("content", Some(&m.content), MAX_CIPHERTEXT_LEN),
            ClientMessage::Hello(_)
            | ClientMessage::TaskRelease(_)
            | ClientMessage::TaskReleaseWithdraw(_)
            | ClientMessage::TaskReleaseAnswer(_)
            | ClientMessage::TaskForceRelease(_)
            | ClientMessage::TaskRekey(_)
            | ClientMessage::ClaimClear(_)
            | ClientMessage::BlobGet(_)
            | ClientMessage::Ping => Ok(()),
        }
    }
}

fn count(field: &'static str, len: usize, max: usize) -> Result<(), (&'static str, usize)> {
    if len > max {
        return Err((field, max));
    }
    Ok(())
}

fn sealed(
    field: &'static str,
    value: Option<&Sealed>,
    max: usize,
) -> Result<(), (&'static str, usize)> {
    count(field, value.map_or(0, |s| s.data.as_str().len()), max)
}

fn inline(field: &'static str, value: Option<&Sealed>) -> Result<(), (&'static str, usize)> {
    sealed(field, value, MAX_INLINE_CIPHERTEXT_LEN)
}

// ---------------------------------------------------------------------------------------------
// Server messages
// ---------------------------------------------------------------------------------------------

/// The hello was accepted. Events after the client's cursor follow (with `seq`), then live ones.
/// After `resync_required` the snapshot ends with a `welcome` whose `head` is the new cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Welcome {
    pub proto: u16,
    pub stream: StreamId,
    /// The newest event of the log when the welcome was sent.
    pub head: Seq,
    pub member: MemberId,
    pub epoch: Epoch,
}

/// Why the server asks for a resync instead of replaying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResyncReason {
    /// The client sent no cursor.
    NoCursor,
    /// The cursor names another stream (server restored or reset).
    OtherStream,
    /// The events after the cursor left the replay window.
    TooOld,
    /// The cursor is ahead of the log (the client saw events the server lost).
    Ahead,
}

/// The client drops its team state; the server sends the current state as events without
/// `seq` (`member.joined`, `presence.update`, `task.reserved` per hold, and for a hold in that
/// state also `reservation.expiring` and `task.release_requested`, so a holder can still answer
/// an open request) and ends with `welcome`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResyncRequired {
    pub stream: StreamId,
    pub head: Seq,
    pub reason: ResyncReason,
}

/// An intent was applied (the envelope's `id` names it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ack {
    /// `blob.put`: the stored blob; `blob.get`: the blob asked for.
    pub blob: Option<BlobId>,
    /// `blob.get`: its content.
    pub content: Option<Sealed>,
}

/// The code of an `err`; clients map it to a message of their own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// Someone else holds the reservation (first wins), or a release request is already open.
    Conflict,
    /// A frame, a list or a count is over its limit (also seats and reservations per member).
    Limit,
    /// Not a member, not the holder, or not allowed for this role.
    Forbidden,
    /// Too many frames or a release request too soon after the last one; `retryAfterMs` says
    /// when to try again.
    RateLimited,
    /// The client speaks an older protocol than the server accepts; Hangar asks for an update.
    OutdatedClient,
    /// A hash or a ciphertext was made with another epoch's key: the client waits for
    /// `key.rotated` (or fetches the current key) and sends it again.
    StaleEpoch,
    /// The frame is malformed or does not fit the state (for example a heartbeat after expiry,
    /// or an op id used again with another body).
    BadRequest,
    /// The task, blob or team is unknown.
    NotFound,
}

/// `err`: an intent was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub retry_after_ms: Option<u64>,
    /// `conflict` on `task.reserve`: who holds the task.
    pub holder: Option<Holder>,
}

/// `presence.update`: a device's presence changed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresenceUpdate {
    pub holder: Holder,
    pub state: PresenceState,
    pub status: Option<Sealed>,
    pub at: UnixMillis,
}

/// A task is held, newly or with a new deadline (heartbeat, holder back online).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TaskReserved {
    pub task: KeyedHash,
    pub holder: Holder,
    pub epoch: Epoch,
    pub since: UnixMillis,
    /// `None` while the holder is online and the policy lets reservations of online members
    /// stand.
    pub expires_at: Option<UnixMillis>,
    pub note: Option<Sealed>,
}

/// Why a hold ended (`task.released`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ReleaseReason {
    /// The holder released it (also the old hash of a rekeyed hold).
    Released,
    /// The holder accepted a release request; a `task.reserved` for the requester follows.
    HandedOver,
    /// The holder left the team or the device was removed.
    MemberRemoved,
    /// An owner or admin ended the hold.
    ForceReleased,
}

/// `task.released`: a hold ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskReleased {
    pub task: KeyedHash,
    pub holder: Holder,
    pub reason: ReleaseReason,
}

/// Sent once per hold before it expires, so the holder can keep it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReservationExpiring {
    pub task: KeyedHash,
    pub holder: Holder,
    pub expires_at: UnixMillis,
}

/// `reservation.expired`: a hold ran out (or a hold of an old epoch was not rekeyed within the
/// grace period); anyone may reserve the task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReservationExpired {
    pub task: KeyedHash,
    pub holder: Holder,
}

/// How much two sessions' claims share, as far as the server can tell from keyed hashes. Lines
/// travel encrypted (`ClaimSet::detail`), so "overlapping lines" is decided by the clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MatchLevel {
    /// No longer overlapping (sent once when an earlier overlap ended).
    None,
    /// Same first folder (the area of step A), no shared file.
    SameArea,
    SameFile,
}

/// Another member's session as the server knows it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRef {
    pub holder: Holder,
    pub session: SessionKey,
}

/// Sent to both sides of an overlap, each with its own session as `session`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimOverlap {
    pub repo: KeyedHash,
    pub session: SessionKey,
    pub other: SessionRef,
    pub level: MatchLevel,
    /// For `sameFile`: one shared file (the first in the receiver's order).
    pub file: Option<KeyedHash>,
    /// Shared files in all.
    pub files: u32,
    /// For `sameFile`: the other session's encrypted detail, so the receiver can refine to
    /// overlapping lines. Sent again when the other side's detail changes.
    pub detail: Option<Sealed>,
}

/// `task.release_requested`: someone asks the holder for the task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskReleaseRequested {
    pub task: KeyedHash,
    pub holder: Holder,
    pub requester: MemberId,
    pub note: Option<Sealed>,
}

/// `task.release_answered`: the holder answered a release request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskReleaseAnswered {
    pub task: KeyedHash,
    pub holder: Holder,
    pub requester: MemberId,
    pub accepted: bool,
}

/// Why a release request ended without an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RequestEndReason {
    /// The requester took it back, or the requesting device was removed.
    Withdrawn,
    /// The holder did not answer in time.
    TimedOut,
}

/// `task.release_request_ended`: an open release request ended without an answer; the hold
/// goes on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskReleaseRequestEnded {
    pub task: KeyedHash,
    pub holder: Holder,
    pub requester: MemberId,
    pub reason: RequestEndReason,
}

/// A member's role in a team (as the website's ticket names it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Owner,
    Admin,
    Member,
}

/// `member.joined`: someone joined the team.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberJoined {
    pub member: MemberId,
    pub role: Role,
}

/// `member.removed`: someone left or was removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberRemoved {
    pub member: MemberId,
}

/// A new team key epoch. Clients rekey their reservations and resend their claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyRotated {
    pub epoch: Epoch,
    /// The new team key wrapped to the receiving device (step E); the server sends each device
    /// its own.
    pub wrapped: Option<Ciphertext>,
}

/// What a `notify` is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NotifyTopic {
    /// Something in the team needs this person (the content says what, if any).
    Attention,
}

/// `notify`: something needs the person; no content unless encrypted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notify {
    pub topic: NotifyTopic,
    pub content: Option<Sealed>,
}

/// Everything the server sends. `type` on the wire is the serde name; [`SERVER_KINDS`] lists
/// them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", content = "body")]
pub enum ServerMessage {
    #[serde(rename = "welcome")]
    Welcome(Welcome),
    #[serde(rename = "resync_required")]
    ResyncRequired(ResyncRequired),
    #[serde(rename = "ack")]
    Ack(Ack),
    #[serde(rename = "err")]
    Err(ErrorBody),
    #[serde(rename = "presence.update")]
    PresenceUpdate(PresenceUpdate),
    #[serde(rename = "task.reserved")]
    TaskReserved(TaskReserved),
    #[serde(rename = "task.released")]
    TaskReleased(TaskReleased),
    #[serde(rename = "reservation.expiring")]
    ReservationExpiring(ReservationExpiring),
    #[serde(rename = "reservation.expired")]
    ReservationExpired(ReservationExpired),
    #[serde(rename = "claim.overlap")]
    ClaimOverlap(ClaimOverlap),
    #[serde(rename = "task.release_requested")]
    TaskReleaseRequested(TaskReleaseRequested),
    #[serde(rename = "task.release_answered")]
    TaskReleaseAnswered(TaskReleaseAnswered),
    #[serde(rename = "task.release_request_ended")]
    TaskReleaseRequestEnded(TaskReleaseRequestEnded),
    #[serde(rename = "member.joined")]
    MemberJoined(MemberJoined),
    #[serde(rename = "member.removed")]
    MemberRemoved(MemberRemoved),
    #[serde(rename = "key.rotated")]
    KeyRotated(KeyRotated),
    #[serde(rename = "notify")]
    Notify(Notify),
}

impl ServerMessage {
    /// The `type` on the wire.
    pub fn kind(&self) -> &'static str {
        match self {
            ServerMessage::Welcome(_) => "welcome",
            ServerMessage::ResyncRequired(_) => "resync_required",
            ServerMessage::Ack(_) => "ack",
            ServerMessage::Err(_) => "err",
            ServerMessage::PresenceUpdate(_) => "presence.update",
            ServerMessage::TaskReserved(_) => "task.reserved",
            ServerMessage::TaskReleased(_) => "task.released",
            ServerMessage::ReservationExpiring(_) => "reservation.expiring",
            ServerMessage::ReservationExpired(_) => "reservation.expired",
            ServerMessage::ClaimOverlap(_) => "claim.overlap",
            ServerMessage::TaskReleaseRequested(_) => "task.release_requested",
            ServerMessage::TaskReleaseAnswered(_) => "task.release_answered",
            ServerMessage::TaskReleaseRequestEnded(_) => "task.release_request_ended",
            ServerMessage::MemberJoined(_) => "member.joined",
            ServerMessage::MemberRemoved(_) => "member.removed",
            ServerMessage::KeyRotated(_) => "key.rotated",
            ServerMessage::Notify(_) => "notify",
        }
    }

    /// Whether the event may enter the team's log and so carry a `seq`. Presence and overlaps
    /// are live state: a reconnecting client gets them again (its own `claim.set` and the
    /// others' presence), they are never replayed. Snapshot events of a resync carry no `seq`
    /// even when their kind may.
    pub fn loggable(&self) -> bool {
        matches!(
            self,
            ServerMessage::TaskReserved(_)
                | ServerMessage::TaskReleased(_)
                | ServerMessage::ReservationExpiring(_)
                | ServerMessage::ReservationExpired(_)
                | ServerMessage::TaskReleaseRequested(_)
                | ServerMessage::TaskReleaseAnswered(_)
                | ServerMessage::TaskReleaseRequestEnded(_)
                | ServerMessage::MemberJoined(_)
                | ServerMessage::MemberRemoved(_)
                | ServerMessage::KeyRotated(_)
                | ServerMessage::Notify(_)
        )
    }

    fn check_limits(&self) -> Result<(), (&'static str, usize)> {
        match self {
            ServerMessage::Ack(m) => sealed("content", m.content.as_ref(), MAX_CIPHERTEXT_LEN),
            ServerMessage::PresenceUpdate(m) => inline("status", m.status.as_ref()),
            ServerMessage::TaskReserved(m) => inline("note", m.note.as_ref()),
            ServerMessage::TaskReleaseRequested(m) => inline("note", m.note.as_ref()),
            ServerMessage::ClaimOverlap(m) => {
                sealed("detail", m.detail.as_ref(), MAX_CIPHERTEXT_LEN)
            }
            ServerMessage::KeyRotated(m) => count(
                "wrapped",
                m.wrapped.as_ref().map_or(0, |w| w.as_str().len()),
                MAX_INLINE_CIPHERTEXT_LEN,
            ),
            ServerMessage::Notify(m) => inline("content", m.content.as_ref()),
            _ => Ok(()),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------------------------

/// A frame a client sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientFrame {
    pub id: OpId,
    /// Required for everything but `ping`.
    pub team: Option<TeamId>,
    pub message: ClientMessage,
}

/// A frame the server sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerFrame {
    /// The client's op this answers (`ack`, `err`, `welcome`, `resync_required`, and events an
    /// op caused for its sender).
    pub id: Option<OpId>,
    /// Position in the team's log; only for loggable events.
    pub seq: Option<Seq>,
    pub team: Option<TeamId>,
    pub message: ServerMessage,
}

/// The envelope as it is written: the message adds `type` and `body` in declaration order.
#[derive(Serialize)]
struct OutFrame<'a, M> {
    v: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<&'a OpId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seq: Option<Seq>,
    #[serde(skip_serializing_if = "Option::is_none")]
    team: Option<&'a TeamId>,
    #[serde(flatten)]
    message: &'a M,
}

/// Read first and on its own: other fields are ignored here, so a frame of another version is
/// told so before its envelope is checked against this version's fields.
#[derive(Deserialize)]
struct VersionProbe {
    v: u64,
}

/// The envelope as it is read; the body stays raw JSON until its type is known.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InFrame<'a> {
    #[allow(dead_code)]
    v: u64,
    #[serde(default)]
    id: Option<OpId>,
    #[serde(default)]
    seq: Option<Seq>,
    #[serde(default)]
    team: Option<TeamId>,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default, borrow)]
    body: Option<&'a RawValue>,
}

fn finish<M: Serialize>(frame: &OutFrame<'_, M>) -> Result<String, EncodeError> {
    let text = serde_json::to_string(frame).map_err(|e| EncodeError::Json(e.to_string()))?;
    if text.len() > MAX_FRAME_BYTES {
        return Err(EncodeError::TooLarge {
            max: MAX_FRAME_BYTES,
        });
    }
    Ok(text)
}

fn limit_error((field, max): (&'static str, usize)) -> EncodeError {
    EncodeError::Limit { field, max }
}

/// Builds the text of a client frame; refuses values over a limit and team messages without a
/// team.
pub fn encode_client(frame: &ClientFrame) -> Result<String, EncodeError> {
    if frame.message.needs_team() && frame.team.is_none() {
        return Err(EncodeError::MissingTeam {
            kind: frame.message.kind(),
        });
    }
    frame.message.check_limits().map_err(limit_error)?;
    finish(&OutFrame {
        v: PROTOCOL_VERSION,
        id: Some(&frame.id),
        seq: None,
        team: frame.team.as_ref(),
        message: &frame.message,
    })
}

/// Builds the text of a server frame; refuses values over a limit.
pub fn encode_server(frame: &ServerFrame) -> Result<String, EncodeError> {
    frame.message.check_limits().map_err(limit_error)?;
    finish(&OutFrame {
        v: PROTOCOL_VERSION,
        id: frame.id.as_ref(),
        seq: frame.seq,
        team: frame.team.as_ref(),
        message: &frame.message,
    })
}

fn envelope(text: &str) -> Result<InFrame<'_>, DecodeError> {
    if text.len() > MAX_FRAME_BYTES {
        return Err(DecodeError::TooLarge {
            max: MAX_FRAME_BYTES,
        });
    }
    let probe: VersionProbe =
        serde_json::from_str(text).map_err(|e| DecodeError::Envelope(scrub(&e.to_string())))?;
    if probe.v != u64::from(PROTOCOL_VERSION) {
        return Err(DecodeError::Version {
            got: probe.v,
            supported: PROTOCOL_VERSION,
        });
    }
    serde_json::from_str(text).map_err(|e| DecodeError::Envelope(scrub(&e.to_string())))
}

/// Every `type` a client may send.
pub const CLIENT_KINDS: &[&str] = &[
    "hello",
    "presence.set",
    "heartbeat",
    "task.reserve",
    "task.release",
    "task.release_request",
    "task.release_withdraw",
    "task.release_answer",
    "task.force_release",
    "task.rekey",
    "claim.set",
    "claim.clear",
    "blob.put",
    "blob.get",
    "ping",
];

/// Every `type` the server may send.
pub const SERVER_KINDS: &[&str] = &[
    "welcome",
    "resync_required",
    "ack",
    "err",
    "presence.update",
    "task.reserved",
    "task.released",
    "reservation.expiring",
    "reservation.expired",
    "claim.overlap",
    "task.release_requested",
    "task.release_answered",
    "task.release_request_ended",
    "member.joined",
    "member.removed",
    "key.rotated",
    "notify",
];

/// Parses a body straight into its type, so duplicate and unknown fields are refused by the
/// type's own rules.
fn parse<T: DeserializeOwned>(
    kind: &'static str,
    body: Option<&RawValue>,
) -> Result<T, DecodeError> {
    let body = body.ok_or_else(|| DecodeError::Body {
        kind,
        reason: "missing field `body`".into(),
    })?;
    serde_json::from_str(body.get()).map_err(|e| DecodeError::Body {
        kind,
        reason: scrub(&e.to_string()),
    })
}

/// Only a known type is named in an error; anything else could be a secret sent by mistake.
fn known(kind: &str, kinds: &[&'static str]) -> Result<&'static str, DecodeError> {
    kinds
        .iter()
        .copied()
        .find(|known| *known == kind)
        .ok_or_else(unlisted)
}

fn client_message(kind: &str, body: Option<&RawValue>) -> Result<ClientMessage, DecodeError> {
    let kind = known(kind, CLIENT_KINDS)?;
    Ok(match kind {
        "hello" => ClientMessage::Hello(parse(kind, body)?),
        "presence.set" => ClientMessage::PresenceSet(parse(kind, body)?),
        "heartbeat" => ClientMessage::Heartbeat(parse(kind, body)?),
        "task.reserve" => ClientMessage::TaskReserve(parse(kind, body)?),
        "task.release" => ClientMessage::TaskRelease(parse(kind, body)?),
        "task.release_request" => ClientMessage::TaskReleaseRequest(parse(kind, body)?),
        "task.release_withdraw" => ClientMessage::TaskReleaseWithdraw(parse(kind, body)?),
        "task.release_answer" => ClientMessage::TaskReleaseAnswer(parse(kind, body)?),
        "task.force_release" => ClientMessage::TaskForceRelease(parse(kind, body)?),
        "task.rekey" => ClientMessage::TaskRekey(parse(kind, body)?),
        "claim.set" => ClientMessage::ClaimSet(parse(kind, body)?),
        "claim.clear" => ClientMessage::ClaimClear(parse(kind, body)?),
        "blob.put" => ClientMessage::BlobPut(parse(kind, body)?),
        "blob.get" => ClientMessage::BlobGet(parse(kind, body)?),
        "ping" if body.is_none() => ClientMessage::Ping,
        "ping" => {
            return Err(DecodeError::Body {
                kind,
                reason: "has no body".into(),
            })
        }
        _ => return Err(unlisted()),
    })
}

fn server_message(kind: &str, body: Option<&RawValue>) -> Result<ServerMessage, DecodeError> {
    let kind = known(kind, SERVER_KINDS)?;
    Ok(match kind {
        "welcome" => ServerMessage::Welcome(parse(kind, body)?),
        "resync_required" => ServerMessage::ResyncRequired(parse(kind, body)?),
        "ack" => ServerMessage::Ack(parse(kind, body)?),
        "err" => ServerMessage::Err(parse(kind, body)?),
        "presence.update" => ServerMessage::PresenceUpdate(parse(kind, body)?),
        "task.reserved" => ServerMessage::TaskReserved(parse(kind, body)?),
        "task.released" => ServerMessage::TaskReleased(parse(kind, body)?),
        "reservation.expiring" => ServerMessage::ReservationExpiring(parse(kind, body)?),
        "reservation.expired" => ServerMessage::ReservationExpired(parse(kind, body)?),
        "claim.overlap" => ServerMessage::ClaimOverlap(parse(kind, body)?),
        "task.release_requested" => ServerMessage::TaskReleaseRequested(parse(kind, body)?),
        "task.release_answered" => ServerMessage::TaskReleaseAnswered(parse(kind, body)?),
        "task.release_request_ended" => ServerMessage::TaskReleaseRequestEnded(parse(kind, body)?),
        "member.joined" => ServerMessage::MemberJoined(parse(kind, body)?),
        "member.removed" => ServerMessage::MemberRemoved(parse(kind, body)?),
        "key.rotated" => ServerMessage::KeyRotated(parse(kind, body)?),
        "notify" => ServerMessage::Notify(parse(kind, body)?),
        _ => return Err(unlisted()),
    })
}

/// An unknown kind (or a listed one without a decoder, which a test rules out); the error never
/// repeats the kind, which could be a secret sent by mistake.
fn unlisted() -> DecodeError {
    DecodeError::Body {
        kind: "unknown type",
        reason: "no such message type".into(),
    }
}

/// serde's messages quote the offending value or field name, which may be a ticket or a path
/// typed into the wrong field; errors end up in logs, so the message ends before its first
/// quote. Quoted parts cannot be skipped one by one: field names are quoted without escaping,
/// so a backtick or quote inside the input would end the hidden part early.
fn scrub(message: &str) -> String {
    let (kept, cut) = match message.find(['"', '`']) {
        Some(at) => (&message[..at], true),
        None => (message, false),
    };
    let mut out: String = kept.trim_end().chars().take(200).collect();
    if cut {
        out.push_str(" ..");
    }
    out
}

fn decode_limit((field, max): (&'static str, usize)) -> DecodeError {
    DecodeError::Limit { field, max }
}

/// The server's side: one text message of a client.
pub fn decode_client(text: &str) -> Result<ClientFrame, DecodeError> {
    let raw = envelope(text)?;
    let id = raw
        .id
        .ok_or_else(|| DecodeError::Envelope("missing field `id`".into()))?;
    if raw.seq.is_some() {
        return Err(DecodeError::Envelope("a client frame has no `seq`".into()));
    }
    let message = client_message(&raw.kind, raw.body)?;
    if message.needs_team() && raw.team.is_none() {
        return Err(DecodeError::MissingTeam {
            kind: message.kind(),
        });
    }
    message.check_limits().map_err(decode_limit)?;
    Ok(ClientFrame {
        id,
        team: raw.team,
        message,
    })
}

/// The client's side: one text message of the server.
pub fn decode_server(text: &str) -> Result<ServerFrame, DecodeError> {
    let raw = envelope(text)?;
    let message = server_message(&raw.kind, raw.body)?;
    if raw.seq.is_some() && !message.loggable() {
        return Err(DecodeError::Sequence {
            kind: message.kind(),
            problem: "never carries a seq",
        });
    }
    message.check_limits().map_err(decode_limit)?;
    Ok(ServerFrame {
        id: raw.id,
        seq: raw.seq,
        team: raw.team,
        message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(byte: u8) -> KeyedHash {
        KeyedHash::from_bytes([byte; 32])
    }

    fn holder() -> Holder {
        Holder {
            member: MemberId::new("m1").unwrap(),
            device: DeviceId::new("d1").unwrap(),
        }
    }

    fn sealed_text(len: usize) -> Sealed {
        Sealed {
            epoch: Epoch(2),
            data: Ciphertext::new("A".repeat(len)).unwrap(),
        }
    }

    fn client(message: ClientMessage) -> ClientFrame {
        ClientFrame {
            id: OpId::new("op-1").unwrap(),
            team: Some(TeamId::new("t1").unwrap()),
            message,
        }
    }

    fn all_client_messages() -> Vec<ClientMessage> {
        vec![
            ClientMessage::Hello(Hello {
                ticket: Ticket::new("ticket-marker").unwrap(),
                device: DeviceId::new("d1").unwrap(),
                proto: ProtoRange::CURRENT,
                last_seq: Some(Cursor {
                    stream: StreamId::new("s1").unwrap(),
                    seq: Seq(41),
                }),
            }),
            ClientMessage::PresenceSet(PresenceSet {
                state: PresenceState::Active,
                status: Some(sealed_text(8)),
            }),
            ClientMessage::Heartbeat(Heartbeat {
                tasks: vec![h(1), h(2)],
            }),
            ClientMessage::TaskReserve(TaskReserve {
                task: h(1),
                epoch: Epoch(2),
                prev: vec![h(3)],
                note: None,
            }),
            ClientMessage::TaskRelease(TaskRef { task: h(1) }),
            ClientMessage::TaskReleaseRequest(TaskReleaseRequest {
                task: h(1),
                note: Some(sealed_text(4)),
            }),
            ClientMessage::TaskReleaseWithdraw(TaskRef { task: h(1) }),
            ClientMessage::TaskReleaseAnswer(TaskReleaseAnswer {
                task: h(1),
                requester: MemberId::new("m2").unwrap(),
                accept: true,
            }),
            ClientMessage::TaskForceRelease(TaskRef { task: h(1) }),
            ClientMessage::TaskRekey(TaskRekey {
                from: h(1),
                to: h(9),
                epoch: Epoch(3),
            }),
            ClientMessage::ClaimSet(ClaimSet {
                session: SessionKey::new("k1").unwrap(),
                repo: h(5),
                epoch: Epoch(2),
                files: vec![
                    PathHashes {
                        area: Some(h(6)),
                        file: h(8),
                    },
                    PathHashes {
                        area: None,
                        file: h(7),
                    },
                ],
                truncated: false,
                detail: Some(sealed_text(12)),
            }),
            ClientMessage::ClaimClear(ClaimClear {
                session: SessionKey::new("k1").unwrap(),
            }),
            ClientMessage::BlobPut(BlobPut {
                kind: BlobKind::Decision,
                content: sealed_text(40),
            }),
            ClientMessage::BlobGet(BlobGet {
                blob: BlobId::new("b1").unwrap(),
            }),
            ClientMessage::Ping,
        ]
    }

    fn all_server_messages() -> Vec<ServerMessage> {
        let other = SessionRef {
            holder: holder(),
            session: SessionKey::new("k2").unwrap(),
        };
        vec![
            ServerMessage::Welcome(Welcome {
                proto: 1,
                stream: StreamId::new("s1").unwrap(),
                head: Seq(50),
                member: MemberId::new("m1").unwrap(),
                epoch: Epoch(2),
            }),
            ServerMessage::ResyncRequired(ResyncRequired {
                stream: StreamId::new("s2").unwrap(),
                head: Seq(3),
                reason: ResyncReason::OtherStream,
            }),
            ServerMessage::Ack(Ack {
                blob: Some(BlobId::new("b1").unwrap()),
                content: None,
            }),
            ServerMessage::Err(ErrorBody {
                code: ErrorCode::StaleEpoch,
                retry_after_ms: None,
                holder: Some(holder()),
            }),
            ServerMessage::PresenceUpdate(PresenceUpdate {
                holder: holder(),
                state: PresenceState::Idle,
                status: None,
                at: UnixMillis(1000),
            }),
            ServerMessage::TaskReserved(TaskReserved {
                task: h(1),
                holder: holder(),
                epoch: Epoch(2),
                since: UnixMillis(10),
                expires_at: Some(UnixMillis(20)),
                note: None,
            }),
            ServerMessage::TaskReleased(TaskReleased {
                task: h(1),
                holder: holder(),
                reason: ReleaseReason::ForceReleased,
            }),
            ServerMessage::ReservationExpiring(ReservationExpiring {
                task: h(1),
                holder: holder(),
                expires_at: UnixMillis(20),
            }),
            ServerMessage::ReservationExpired(ReservationExpired {
                task: h(1),
                holder: holder(),
            }),
            ServerMessage::ClaimOverlap(ClaimOverlap {
                repo: h(5),
                session: SessionKey::new("k1").unwrap(),
                other,
                level: MatchLevel::SameFile,
                file: Some(h(8)),
                files: 1,
                detail: Some(sealed_text(16)),
            }),
            ServerMessage::TaskReleaseRequested(TaskReleaseRequested {
                task: h(1),
                holder: holder(),
                requester: MemberId::new("m2").unwrap(),
                note: None,
            }),
            ServerMessage::TaskReleaseAnswered(TaskReleaseAnswered {
                task: h(1),
                holder: holder(),
                requester: MemberId::new("m2").unwrap(),
                accepted: false,
            }),
            ServerMessage::TaskReleaseRequestEnded(TaskReleaseRequestEnded {
                task: h(1),
                holder: holder(),
                requester: MemberId::new("m2").unwrap(),
                reason: RequestEndReason::TimedOut,
            }),
            ServerMessage::MemberJoined(MemberJoined {
                member: MemberId::new("m3").unwrap(),
                role: Role::Member,
            }),
            ServerMessage::MemberRemoved(MemberRemoved {
                member: MemberId::new("m3").unwrap(),
            }),
            ServerMessage::KeyRotated(KeyRotated {
                epoch: Epoch(3),
                wrapped: Some(Ciphertext::new("AAAA").unwrap()),
            }),
            ServerMessage::Notify(Notify {
                topic: NotifyTopic::Attention,
                content: None,
            }),
        ]
    }

    #[test]
    fn kind_lists_are_complete() {
        let client: Vec<_> = all_client_messages().iter().map(|m| m.kind()).collect();
        assert_eq!(client, CLIENT_KINDS);
        let server: Vec<_> = all_server_messages().iter().map(|m| m.kind()).collect();
        assert_eq!(server, SERVER_KINDS);
    }

    #[test]
    fn every_client_message_round_trips() {
        for message in all_client_messages() {
            let frame = client(message);
            let text = encode_client(&frame).unwrap();
            // The serde name and the decoder's table agree.
            assert!(
                text.contains(&format!(r#""type":"{}""#, frame.message.kind())),
                "{text}"
            );
            assert_eq!(decode_client(&text).unwrap(), frame, "{text}");
        }
    }

    #[test]
    fn every_server_message_round_trips() {
        for (n, message) in all_server_messages().into_iter().enumerate() {
            let seq = message.loggable().then_some(Seq(n as u64 + 1));
            let frame = ServerFrame {
                id: Some(OpId::new("op-1").unwrap()),
                seq,
                team: Some(TeamId::new("t1").unwrap()),
                message,
            };
            let text = encode_server(&frame).unwrap();
            assert!(
                text.contains(&format!(r#""type":"{}""#, frame.message.kind())),
                "{text}"
            );
            assert_eq!(decode_server(&text).unwrap(), frame, "{text}");
        }
    }

    #[test]
    fn wire_shape_is_stable() {
        let text = encode_client(&client(ClientMessage::TaskRelease(TaskRef {
            task: h(0xab),
        })))
        .unwrap();
        assert_eq!(
            text,
            format!(
                r#"{{"v":1,"id":"op-1","team":"t1","type":"task.release","body":{{"task":"{}"}}}}"#,
                "ab".repeat(32)
            )
        );
        let ping = encode_client(&ClientFrame {
            id: OpId::new("p").unwrap(),
            team: None,
            message: ClientMessage::Ping,
        })
        .unwrap();
        assert_eq!(ping, r#"{"v":1,"id":"p","type":"ping"}"#);
        let hello = encode_client(&client(all_client_messages().remove(0))).unwrap();
        // Fields come out in declaration order.
        assert!(
            hello.contains(r#""proto":{"min":1,"max":1},"lastSeq":{"stream":"s1","seq":41}"#),
            "{hello}"
        );
    }

    #[test]
    fn a_team_message_without_a_team_is_never_built() {
        let frame = ClientFrame {
            id: OpId::new("o").unwrap(),
            team: None,
            message: ClientMessage::TaskRelease(TaskRef { task: h(1) }),
        };
        assert_eq!(
            encode_client(&frame),
            Err(EncodeError::MissingTeam {
                kind: "task.release"
            })
        );
    }

    #[test]
    fn unknown_and_duplicate_fields_are_rejected() {
        let task = "ab".repeat(32);
        let extra_envelope = format!(
            r#"{{"v":1,"id":"o","team":"t","type":"task.release","body":{{"task":"{task}"}},"x":1}}"#
        );
        assert!(matches!(
            decode_client(&extra_envelope),
            Err(DecodeError::Envelope(_))
        ));
        let extra_body = format!(
            r#"{{"v":1,"id":"o","team":"t","type":"task.release","body":{{"task":"{task}","path":"src/a.rs"}}}}"#
        );
        assert!(matches!(
            decode_client(&extra_body),
            Err(DecodeError::Body { .. })
        ));
        let unknown_type = r#"{"v":1,"id":"o","team":"t","type":"task.steal","body":{}}"#;
        assert!(matches!(
            decode_client(unknown_type),
            Err(DecodeError::Body { .. })
        ));
        let nested = format!(
            r#"{{"v":1,"id":"o","team":"t","type":"claim.set","body":{{"session":"k","repo":"{task}","epoch":1,"files":[{{"area":null,"file":"{task}","name":"x"}}],"truncated":false,"detail":null}}}}"#
        );
        assert!(decode_client(&nested).is_err());
        // Duplicate keys are refused everywhere instead of the last one silently winning.
        let other = "cd".repeat(32);
        let duplicate_body = format!(
            r#"{{"v":1,"id":"o","team":"t","type":"task.release","body":{{"task":"{task}","task":"{other}"}}}}"#
        );
        assert!(matches!(
            decode_client(&duplicate_body),
            Err(DecodeError::Body { .. })
        ));
        let duplicate_team = format!(
            r#"{{"v":1,"id":"o","team":"t","team":"u","type":"task.release","body":{{"task":"{task}"}}}}"#
        );
        assert!(matches!(
            decode_client(&duplicate_team),
            Err(DecodeError::Envelope(_))
        ));
        let duplicate_nested = format!(
            r#"{{"v":1,"id":"o","team":"t","type":"claim.set","body":{{"session":"k","repo":"{task}","epoch":1,"files":[{{"area":null,"file":"{task}","file":"{other}"}}],"truncated":false,"detail":null}}}}"#
        );
        assert!(decode_client(&duplicate_nested).is_err());
        // Ping has no body.
        assert!(decode_client(r#"{"v":1,"id":"o","type":"ping","body":{}}"#).is_err());
    }

    #[test]
    fn envelope_rules() {
        let task = "ab".repeat(32);
        // Version before body: an old client is told so even if its body would not parse.
        let old = r#"{"v":0,"id":"o","team":"t","type":"whatever","body":{"x":1}}"#;
        let error = decode_client(old).unwrap_err();
        assert_eq!(
            error,
            DecodeError::Version {
                got: 0,
                supported: 1
            }
        );
        assert_eq!(error.code(), ErrorCode::OutdatedClient);
        assert!(!error.needs_update());
        // A newer envelope with fields this version does not know is still a version error.
        let newer = r#"{"v":2,"id":"o","type":"ping","trace":"x","prio":1}"#;
        let error = decode_client(newer).unwrap_err();
        assert!(matches!(error, DecodeError::Version { got: 2, .. }));
        assert_eq!(error.code(), ErrorCode::BadRequest);
        let error = decode_server(newer).unwrap_err();
        assert!(error.needs_update(), "{error}");
        assert!(matches!(
            decode_client(r#"{"v":70000,"id":"o","type":"ping"}"#),
            Err(DecodeError::Version { got: 70000, .. })
        ));
        // A client frame needs an op id and, except ping, a team.
        assert!(decode_client(r#"{"v":1,"type":"ping"}"#).is_err());
        assert!(matches!(
            decode_client(r#"{"id":"o","type":"ping"}"#),
            Err(DecodeError::Envelope(_))
        ));
        let no_team =
            format!(r#"{{"v":1,"id":"o","type":"task.release","body":{{"task":"{task}"}}}}"#);
        assert_eq!(
            decode_client(&no_team),
            Err(DecodeError::MissingTeam {
                kind: "task.release"
            })
        );
        assert!(decode_client(r#"{"v":1,"id":"o","seq":3,"type":"ping"}"#).is_err());
        // Wrong types and invalid ids.
        assert!(decode_client(r#"{"v":"1","id":"o","type":"ping"}"#).is_err());
        assert!(decode_client(r#"{"v":1,"id":"a b","type":"ping"}"#).is_err());
        assert!(decode_client("[]").is_err());
        assert!(decode_client("").is_err());
        // Presence and overlaps are never logged.
        let presence = encode_server(&ServerFrame {
            id: None,
            seq: None,
            team: None,
            message: all_server_messages().remove(4),
        })
        .unwrap();
        let with_seq = presence.replacen(r#""v":1,"#, r#""v":1,"seq":4,"#, 1);
        assert!(matches!(
            decode_server(&with_seq),
            Err(DecodeError::Sequence { .. })
        ));
    }

    #[test]
    fn oversized_input_is_rejected() {
        let huge = format!(
            r#"{{"v":1,"id":"o","type":"ping","body":"{}"}}"#,
            "x".repeat(MAX_FRAME_BYTES)
        );
        assert_eq!(
            decode_client(&huge),
            Err(DecodeError::TooLarge {
                max: MAX_FRAME_BYTES
            })
        );
        assert_eq!(decode_client(&huge).unwrap_err().code(), ErrorCode::Limit);

        let beats = ClientMessage::Heartbeat(Heartbeat {
            tasks: vec![h(1); MAX_HEARTBEAT_TASKS + 1],
        });
        assert_eq!(
            encode_client(&client(beats)),
            Err(EncodeError::Limit {
                field: "tasks",
                max: MAX_HEARTBEAT_TASKS
            })
        );
        // The same over-limit body, written by hand, is refused on decoding.
        let tasks = vec![format!("\"{}\"", "01".repeat(32)); MAX_HEARTBEAT_TASKS + 1].join(",");
        let text = format!(
            r#"{{"v":1,"id":"o","team":"t","type":"heartbeat","body":{{"tasks":[{tasks}]}}}}"#
        );
        assert_eq!(
            decode_client(&text),
            Err(DecodeError::Limit {
                field: "tasks",
                max: MAX_HEARTBEAT_TASKS
            })
        );

        let note = ClientMessage::TaskReserve(TaskReserve {
            task: h(1),
            epoch: Epoch(1),
            prev: Vec::new(),
            note: Some(sealed_text(MAX_INLINE_CIPHERTEXT_LEN + 1)),
        });
        assert!(encode_client(&client(note)).is_err());
        let prev = ClientMessage::TaskReserve(TaskReserve {
            task: h(1),
            epoch: Epoch(1),
            prev: vec![h(2); MAX_PREV_HASHES + 1],
            note: None,
        });
        assert_eq!(
            encode_client(&client(prev)),
            Err(EncodeError::Limit {
                field: "prev",
                max: MAX_PREV_HASHES
            })
        );
    }

    #[test]
    fn the_largest_frames_fit() {
        let largest = ClientMessage::ClaimSet(ClaimSet {
            session: SessionKey::new("k".repeat(64)).unwrap(),
            repo: h(1),
            epoch: Epoch(u32::MAX),
            files: vec![
                PathHashes {
                    area: Some(h(2)),
                    file: h(3),
                };
                MAX_CLAIM_FILES
            ],
            truncated: true,
            detail: Some(sealed_text(MAX_CIPHERTEXT_LEN)),
        });
        let text = encode_client(&client(largest)).unwrap();
        assert!(text.len() <= MAX_FRAME_BYTES, "{}", text.len());
        assert!(decode_client(&text).is_ok());
        let overlap = ServerMessage::ClaimOverlap(ClaimOverlap {
            repo: h(5),
            session: SessionKey::new("k".repeat(64)).unwrap(),
            other: SessionRef {
                holder: Holder {
                    member: MemberId::new("m".repeat(128)).unwrap(),
                    device: DeviceId::new("d".repeat(128)).unwrap(),
                },
                session: SessionKey::new("k".repeat(64)).unwrap(),
            },
            level: MatchLevel::SameFile,
            file: Some(h(8)),
            files: u32::MAX,
            detail: Some(sealed_text(MAX_CIPHERTEXT_LEN)),
        });
        let frame = ServerFrame {
            id: None,
            seq: None,
            team: Some(TeamId::new("t".repeat(128)).unwrap()),
            message: overlap,
        };
        assert!(decode_server(&encode_server(&frame).unwrap()).is_ok());
    }

    #[test]
    fn versions_are_negotiated_and_checked() {
        let range = |min, max| ProtoRange::new(min, max).unwrap();
        let server = range(1, 3);
        assert_eq!(range(1, 1).negotiate(server), Some(1));
        assert_eq!(range(2, 5).negotiate(server), Some(3));
        assert_eq!(range(4, 5).negotiate(server), None);
        assert_eq!(range(0, 0).negotiate(server), None);
        assert_eq!(ProtoRange::new(3, 2), Err(InvalidRange));
        assert!(serde_json::from_str::<ProtoRange>(r#"{"min":3,"max":2}"#).is_err());
        assert!(serde_json::from_str::<ProtoRange>(r#"{"min":1,"max":2,"x":0}"#).is_err());
        let hello = r#"{"v":1,"id":"o","team":"t","type":"hello","body":{"ticket":"t","device":"d","proto":{"min":2,"max":1},"lastSeq":null}}"#;
        assert!(matches!(
            decode_client(hello),
            Err(DecodeError::Body { kind: "hello", .. })
        ));

        let mut welcome = Welcome {
            proto: 1,
            stream: StreamId::new("s").unwrap(),
            head: Seq(0),
            member: MemberId::new("m").unwrap(),
            epoch: Epoch(1),
        };
        assert_eq!(check_welcome(ProtoRange::CURRENT, &welcome), Ok(()));
        welcome.proto = 2;
        assert_eq!(
            check_welcome(ProtoRange::CURRENT, &welcome),
            Err(UnexpectedVersion { chosen: 2 })
        );
    }

    #[test]
    fn epoch_bound_payloads_are_checked() {
        let current = Epoch(2);
        for message in all_client_messages() {
            let result = message.check_epoch(current);
            // Every sample payload uses epoch 2, except the rekey, which targets epoch 3.
            if matches!(message, ClientMessage::TaskRekey(_)) {
                assert!(result.is_err());
            } else {
                assert_eq!(result, Ok(()), "{}", message.kind());
            }
        }
        let stale_detail = ClientMessage::ClaimSet(ClaimSet {
            session: SessionKey::new("k").unwrap(),
            repo: h(1),
            epoch: current,
            files: Vec::new(),
            truncated: false,
            detail: Some(Sealed {
                epoch: Epoch(1),
                data: Ciphertext::new("AAAA").unwrap(),
            }),
        });
        let error = stale_detail.check_epoch(current).unwrap_err();
        assert_eq!(
            error,
            StaleEpoch {
                kind: "claim.set",
                got: Epoch(1),
                current
            }
        );
        assert_eq!(error.code(), ErrorCode::StaleEpoch);
        let blob = ClientMessage::BlobPut(BlobPut {
            kind: BlobKind::Task,
            content: Sealed {
                epoch: Epoch(3),
                data: Ciphertext::new("AAAA").unwrap(),
            },
        });
        assert!(blob.check_epoch(current).is_err());
        // Old hashes in `prev` are expected.
        let reserve = ClientMessage::TaskReserve(TaskReserve {
            task: h(1),
            epoch: current,
            prev: vec![h(2), h(3)],
            note: None,
        });
        assert_eq!(reserve.check_epoch(current), Ok(()));
    }

    #[test]
    fn digests_tell_retries_from_other_intents() {
        let a = ClientMessage::TaskRelease(TaskRef { task: h(1) });
        let b = ClientMessage::TaskRelease(TaskRef { task: h(2) });
        let c = ClientMessage::TaskReleaseWithdraw(TaskRef { task: h(1) });
        assert_eq!(a.digest(), a.clone().digest());
        assert_ne!(a.digest(), b.digest());
        assert_ne!(a.digest(), c.digest());
    }

    #[test]
    fn errors_quote_nothing_from_the_frame() {
        assert_eq!(
            scrub(r#"invalid type: string "secret", expected u16 at `x"#),
            "invalid type: string .."
        );
        assert_eq!(
            scrub("expected value at line 1 column 1"),
            "expected value at line 1 column 1"
        );
        let misplaced = r#"{"v":1,"id":"o","team":"t","type":"hello","body":{"ticket":"t","device":"d","proto":"ticket-marker","lastSeq":null}}"#;
        let error = decode_client(misplaced).unwrap_err().to_string();
        assert!(!error.contains("ticket-marker"), "{error}");
        let unknown = r#"{"v":1,"id":"o","type":"ticket-marker"}"#;
        let error = decode_client(unknown).unwrap_err().to_string();
        assert!(!error.contains("ticket-marker"), "{error}");
        // serde quotes strings with escapes and field names without any, so a quote or a
        // backtick inside the value must not end the hidden part early.
        let escaped = r#"{"v":1,"id":"o","team":"t","type":"hello","body":{"ticket":"t","device":"d","proto":"x\"ticket-marker","lastSeq":null}}"#;
        let error = decode_client(escaped).unwrap_err().to_string();
        assert!(!error.contains("ticket-marker"), "{error}");
        let field =
            r#"{"v":1,"id":"o","team":"t","type":"task.release","body":{"x`ticket-marker`y":1}}"#;
        let error = decode_client(field).unwrap_err().to_string();
        assert!(!error.contains("ticket-marker"), "{error}");
        let envelope_field = r#"{"v":1,"id":"o","x\"ticket-marker`":1,"type":"ping"}"#;
        let error = decode_client(envelope_field).unwrap_err().to_string();
        assert!(!error.contains("ticket-marker"), "{error}");
        let duplicate = r#"{"v":1,"id":"o","team":"t","type":"task.release","body":{"ticket-marker":1,"ticket-marker":2}}"#;
        let error = decode_client(duplicate).unwrap_err().to_string();
        assert!(!error.contains("ticket-marker"), "{error}");
    }

    #[test]
    fn errors_never_echo_the_ticket() {
        let text = r#"{"v":1,"id":"o","team":"t","type":"hello","body":{"ticket":"ticket-marker","device":"d","proto":{"min":1,"max":1},"lastSeq":null,"extra":true}}"#;
        let error = decode_client(text).unwrap_err().to_string();
        assert!(!error.contains("ticket-marker"), "{error}");
        let frame = decode_client(&text.replace(r#","extra":true"#, "")).unwrap();
        assert!(!format!("{frame:?}").contains("ticket-marker"));
    }
}
