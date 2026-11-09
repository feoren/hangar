//! Sealed records: everything the protocol carries as `Sealed` (presence status, reservation and
//! release request notes, claim details, blobs), encrypted with the epoch's sealing key and signed
//! by the sending device.
//!
//! # Format (version 1)
//!
//! ```text
//! data = 0x01 || head index (8 bytes) || head hash (32 bytes) || counter (8 bytes)
//!        || nonce (24 random bytes) || XChaCha20-Poly1305(seal key, nonce, plaintext, aad)
//!        || Ed25519 signature of the sender (64 bytes)
//! aad  = field("hangar-team/v1/record") || field(version) || field(team) || field(epoch, u32)
//!        || field(kind label) || field(record id) || field(sender member) || field(sender device)
//!        || field(head index, u64) || field(head hash) || field(counter, u64)
//! signed message = field("hangar-team/v1/record-sig") || field(aad) || field(nonce) || field(ciphertext)
//! ```
//!
//! The seal key is `EpochKeys::seal_key` of `hangar_team_protocol::hash` (HKDF-SHA256 of the epoch
//! secret with the label `hangar-team/v1/seal-key`, the team, the epoch and the log hash of the
//! epoch's rotate entry). The record id is what the record belongs to and both sides know without
//! the relay's word: the task's keyed hash for a reservation note, the session key for a claim
//! detail, empty when there is none. The associated data binds the record to team, epoch, kind,
//! record id and sender, so the relay cannot move it elsewhere; the signature stops one member
//! from writing in another's name, which the shared key alone would allow.
//!
//! **Log head.** The sender's newest verified membership log entry (index and hash) travels in
//! the signed context. [`EpochCipher::open_verified`] compares it with the opener's own log: a
//! different hash at that index, in a record whose signature verifies, is a fork the relay
//! showed one side ([`OpenError::Fork`]), so a
//! forked log surfaces with the first record that crosses it, not only when people compare
//! safety numbers. The head also dates the record in log time: a device's signature counts only
//! for heads at which the log vouched for its keys.
//!
//! **Counter.** Every device numbers its records (all kinds, all epochs of the team) upward
//! ([`SendCounter`], resumable at any value); the counter is signed. [`ReplayGuard`] keeps the
//! highest counter seen per sender and signing key and refuses anything at or below it, across
//! restarts when the app stores [`ReplayGuard::marks`]. A device id used again after its removal
//! comes with new keys, so the guard gives it a fresh mark. The app resumes every device's
//! counter at a floor taken from the clock (Hangar: Unix milliseconds times 1024), so the counter
//! of a device id used again continues above its old one; marks kept per device alone (as
//! Hangar's live marks are) then also refuse records signed with the old keys.
//!
//! Order when opening ([`EpochCipher::open_verified`]): format, a head index this device verified,
//! the sender's keys at that index, epoch, signature, the epoch's start, decryption, and only
//! then the head's hash (fork). A forged record costs no decryption, a wrong sender fails as a
//! signature error, and only a record that is signed and sealed with the current epoch's key can
//! report a fork: a record anyone could write with a wrong head hash must not set a device aside,
//! and neither may one signed with the old keys of a device id removed and admitted again (after
//! the rotation the removal made due, its old key period ends before the epoch, so it fails as
//! [`OpenError::HeadBeforeEpoch`]).

use std::collections::BTreeMap;
use std::fmt;

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use hangar_team_protocol::e2e::{ChainHead, LogHash, PublicKey};
use hangar_team_protocol::hash::{EpochKeys, KeyError, KeyedHash, MatchKey};
use hangar_team_protocol::ids::{Epoch, Sealed, TeamId};
use hangar_team_protocol::limits::{MAX_CIPHERTEXT_LEN, MAX_INLINE_CIPHERTEXT_LEN};
use hangar_team_protocol::wire::{BlobKind, Holder};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::encoding::{base64url_len, framed, from_ciphertext, to_ciphertext, Writer};
use crate::entropy::{Entropy, EntropyError};
use crate::epoch::TeamSecret;
use crate::identity::{array24, array32, verify, DeviceIdentity};
use crate::memberlog::TeamState;

const LABEL_RECORD: &[u8] = b"hangar-team/v1/record";
const LABEL_RECORD_SIG: &[u8] = b"hangar-team/v1/record-sig";
const RECORD_VERSION: u8 = 1;
const HEADER_LEN: usize = 1 + 8 + 32 + 8;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const SIGNATURE_LEN: usize = 64;
const OVERHEAD: usize = HEADER_LEN + NONCE_LEN + TAG_LEN + SIGNATURE_LEN;

/// What a sealed record is; part of the associated data, so one kind never opens as another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordKind {
    /// `presence.set` status.
    PresenceStatus,
    /// `task.reserve` note (record id: the task's keyed hash).
    ReservationNote,
    /// `task.release_request` note (record id: the task's keyed hash).
    ReleaseRequestNote,
    /// `claim.set` detail (record id: the session key).
    ClaimDetail,
    /// `blob.put` content.
    Blob(BlobKind),
}

impl RecordKind {
    /// The label in the associated data.
    pub fn label(self) -> &'static str {
        match self {
            RecordKind::PresenceStatus => "presence-status",
            RecordKind::ReservationNote => "reservation-note",
            RecordKind::ReleaseRequestNote => "release-request-note",
            RecordKind::ClaimDetail => "claim-detail",
            RecordKind::Blob(BlobKind::Decision) => "blob/decision",
            RecordKind::Blob(BlobKind::Task) => "blob/task",
            RecordKind::Blob(BlobKind::Transcript) => "blob/transcript",
        }
    }

    /// The largest ciphertext the protocol accepts for this kind (characters).
    pub fn max_chars(self) -> usize {
        match self {
            RecordKind::ClaimDetail | RecordKind::Blob(_) => MAX_CIPHERTEXT_LEN,
            _ => MAX_INLINE_CIPHERTEXT_LEN,
        }
    }

    /// The largest plaintext that seals within [`RecordKind::max_chars`].
    pub fn max_plaintext(self) -> usize {
        (self.max_chars() / 4 * 3).saturating_sub(OVERHEAD)
    }
}

/// Why no cipher was set up for an epoch.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CipherError {
    /// The verified log has no such epoch.
    #[error("the membership log has no epoch {0}")]
    UnknownEpoch(u32),
    /// The secret is not the one the log committed to for the epoch.
    #[error("the secret does not match the membership log's commitment for epoch {0}")]
    Commitment(u32),
    /// Key derivation failed (not expected).
    #[error(transparent)]
    Key(#[from] KeyError),
}

/// Why a record was not sealed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum SealError {
    /// The plaintext does not fit the kind's protocol limit.
    #[error("the record is too large for its kind")]
    TooLarge,
    /// No randomness for the nonce.
    #[error(transparent)]
    Entropy(#[from] EntropyError),
    /// The cipher refused (not expected).
    #[error("sealing failed")]
    Cipher,
}

/// Why a record did not open.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum OpenError {
    /// Not a record of this format.
    #[error("the sealed record is malformed")]
    Malformed,
    /// A record format version this crate does not know.
    #[error("sealed record format version {0} is not supported")]
    Version(u8),
    /// Sealed under another epoch than this cipher's.
    #[error("the record was sealed under epoch {got}, expected {expected}")]
    EpochMismatch {
        /// This cipher's epoch.
        expected: u32,
        /// The record's epoch.
        got: u32,
    },
    /// The sender's signature does not verify over this context: another sender, team, epoch,
    /// kind or record id, or a changed record.
    #[error("the record's signature does not verify for this sender and context")]
    Signature,
    /// Signed, but the ciphertext does not open (a sender who sealed with another key).
    #[error("the record does not decrypt")]
    Decrypt,
    /// The sender saw a newer log than this device: fetch the log (`memberlog.get`) and open
    /// the record again.
    #[error("the record names log entry {index}, which this device has not verified yet")]
    AheadOfLog {
        /// The sender's head index.
        index: u64,
    },
    /// The sender's log has another entry at `index` than this device's: the relay shows the
    /// two devices different logs. The app stops trusting the team and asks both people to
    /// compare log safety numbers.
    #[error("the sender's membership log differs from this device's at entry {index}")]
    Fork {
        /// The first index known to differ.
        index: u64,
    },
    /// The record names a log head from before its epoch began.
    #[error("the record names a log head from before its epoch")]
    HeadBeforeEpoch,
    /// The cipher does not belong to this log (another epoch hash).
    #[error("the cipher's epoch is not in this membership log")]
    UnknownEpoch,
    /// The log never listed the sender.
    #[error("the membership log does not know the sender")]
    UnknownSender,
    /// The log did not vouch for the sender's keys at the record's log head.
    #[error("the sender's keys were not valid at the record's log head")]
    SenderNotValid,
    /// Recovery devices restore a member and sign no records.
    #[error("a recovery device does not sign records")]
    RecoverySender,
    /// The sender's counter is at or below the highest one seen ([`ReplayGuard`]).
    #[error("the record was seen before")]
    Replay,
}

/// An opened record. `Debug` does not show the plaintext.
pub struct Opened {
    /// The device that sealed the record.
    pub sender: Holder,
    /// The sender's log head when it sealed.
    pub head: ChainHead,
    /// The sender's record counter, for [`ReplayGuard`].
    pub counter: u64,
    /// The sender's Ed25519 key the signature verified with, for [`ReplayGuard`].
    pub sign: PublicKey,
    /// The decrypted content, wiped on drop.
    pub plaintext: Zeroizing<Vec<u8>>,
}

impl fmt::Debug for Opened {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Opened")
            .field("sender", &self.sender)
            .field("head", &self.head)
            .field("counter", &self.counter)
            .field("sign", &self.sign)
            .field("len", &self.plaintext.len())
            .finish()
    }
}

/// The record id of a reservation note and of a release request note
/// ([`RecordKind::ReservationNote`], [`RecordKind::ReleaseRequestNote`]): the task's keyed hash
/// as its 64 lowercase hex digits, the spelling the wire carries, as UTF-8 bytes of that text.
/// Every client must use this one convention, or the notes of one never open on another.
pub fn task_record_id(task: &KeyedHash) -> String {
    task.to_hex()
}

/// The log head and the counter a sealed record names, read without a key and **without**
/// checking the signature or the epoch: never a reason to trust anything. For bookkeeping only,
/// such as a high-water mark for a record that could not be opened yet whose occasion ended, so
/// the same record shown again later is refused. `None` for data that is not a record of this
/// version.
pub fn sealed_header(sealed: &Sealed) -> Option<(ChainHead, u64)> {
    let data = from_ciphertext(&sealed.data)?;
    let parsed = parse(&data).ok()?;
    Some((parsed.head, parsed.counter))
}

/// Where a record belongs: the opener names the same values it got from the relay's event and
/// its own context, and a record from anywhere else does not open.
#[derive(Debug, Clone, Copy)]
pub struct RecordContext<'a> {
    /// What the record is.
    pub kind: RecordKind,
    /// What the record belongs to (module docs); empty when nothing.
    pub record: &'a [u8],
    /// The device that sealed the record.
    pub sender: &'a Holder,
}

/// A device's record counter. The app stores [`SendCounter::last`] in the OS keychain or app
/// storage before it sends the record with that number, so a restart never reuses one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SendCounter {
    last: u64,
}

impl SendCounter {
    /// Goes on after `last`, the highest number used so far (0: none yet).
    pub fn resume(last: u64) -> Self {
        Self { last }
    }

    /// The highest number used so far, to store.
    pub fn last(&self) -> u64 {
        self.last
    }

    /// The next number; `None` once every `u64` was used.
    pub fn take(&mut self) -> Option<u64> {
        self.last = self.last.checked_add(1)?;
        Some(self.last)
    }
}

/// The parts of a record's data.
struct Parsed<'a> {
    head: ChainHead,
    counter: u64,
    nonce: &'a [u8],
    ciphertext: &'a [u8],
    signature: [u8; SIGNATURE_LEN],
}

fn parse(data: &[u8]) -> Result<Parsed<'_>, OpenError> {
    let (&version, rest) = data.split_first().ok_or(OpenError::Malformed)?;
    if version != RECORD_VERSION {
        return Err(OpenError::Version(version));
    }
    if rest.len() < HEADER_LEN - 1 + NONCE_LEN + TAG_LEN + SIGNATURE_LEN {
        return Err(OpenError::Malformed);
    }
    let (index, rest) = rest.split_at(8);
    let (hash, rest) = rest.split_at(32);
    let (counter, rest) = rest.split_at(8);
    let (nonce, rest) = rest.split_at(NONCE_LEN);
    let (ciphertext, signature) = rest.split_at(rest.len() - SIGNATURE_LEN);
    let mut sig = [0u8; SIGNATURE_LEN];
    sig.copy_from_slice(signature);
    let u64_of = |bytes: &[u8]| {
        let mut out = [0u8; 8];
        out.copy_from_slice(bytes);
        u64::from_be_bytes(out)
    };
    Ok(Parsed {
        head: ChainHead {
            index: u64_of(index),
            hash: LogHash::from_bytes(array32(hash)),
        },
        counter: u64_of(counter),
        nonce,
        ciphertext,
        signature: sig,
    })
}

/// One epoch's keys for sealing, opening and keyed hashes.
#[derive(Debug)]
pub struct EpochCipher {
    team: TeamId,
    epoch: Epoch,
    rotation: LogHash,
    keys: EpochKeys,
}

impl EpochCipher {
    /// The keys of `epoch` of `team` from its secret; `rotation` is the log hash of the epoch's
    /// rotate entry. Prefer [`EpochCipher::for_epoch`], which takes both from the verified log.
    pub fn new(
        secret: &TeamSecret,
        team: &TeamId,
        epoch: Epoch,
        rotation: &LogHash,
    ) -> Result<Self, KeyError> {
        Ok(Self {
            team: team.clone(),
            epoch,
            rotation: *rotation,
            keys: secret.epoch_keys(team, epoch, rotation)?,
        })
    }

    /// The keys of `epoch` as the verified log records it: the secret must match the epoch's
    /// commitment, and the derivation binds the rotate entry's log hash.
    pub fn for_epoch(
        state: &TeamState,
        secret: &TeamSecret,
        epoch: Epoch,
    ) -> Result<Self, CipherError> {
        let record = state
            .epoch(epoch)
            .ok_or(CipherError::UnknownEpoch(epoch.0))?;
        if !secret
            .commitment(state.team(), epoch)
            .matches(&record.commitment)
        {
            return Err(CipherError::Commitment(epoch.0));
        }
        Ok(Self::new(secret, state.team(), epoch, &record.hash)?)
    }

    /// The epoch.
    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    /// The key for keyed hashes of this epoch.
    pub fn match_key(&self) -> &MatchKey {
        self.keys.match_key()
    }

    fn aad(&self, context: &RecordContext<'_>, head: &ChainHead, counter: u64) -> Vec<u8> {
        let mut w = Writer::new();
        w.field(LABEL_RECORD)
            .field(&[RECORD_VERSION])
            .field(self.team.as_str().as_bytes())
            .field(&self.epoch.0.to_be_bytes())
            .field(context.kind.label().as_bytes())
            .field(context.record)
            .field(context.sender.member.as_str().as_bytes())
            .field(context.sender.device.as_str().as_bytes())
            .u64(head.index)
            .field(head.hash.as_bytes())
            .u64(counter);
        w.into_bytes()
    }

    fn cipher(&self) -> Result<XChaCha20Poly1305, ()> {
        XChaCha20Poly1305::new_from_slice(self.keys.seal_key().expose()).map_err(|_| ())
    }

    /// Seals `plaintext` as `context.sender`, whose private keys `identity` holds. `head` is the
    /// sender's newest verified log entry (`TeamState::head`), `counter` the next number of its
    /// [`SendCounter`].
    pub fn seal(
        &self,
        context: &RecordContext<'_>,
        identity: &DeviceIdentity,
        head: &ChainHead,
        counter: u64,
        plaintext: &[u8],
        entropy: &mut impl Entropy,
    ) -> Result<Sealed, SealError> {
        let total = plaintext
            .len()
            .checked_add(OVERHEAD)
            .ok_or(SealError::TooLarge)?;
        if base64url_len(total) > context.kind.max_chars() {
            return Err(SealError::TooLarge);
        }
        let mut nonce = [0u8; NONCE_LEN];
        entropy.fill(&mut nonce)?;
        let aad = self.aad(context, head, counter);
        let ciphertext = self
            .cipher()
            .map_err(|()| SealError::Cipher)?
            .encrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| SealError::Cipher)?;
        let signature = identity.sign(&framed(&[LABEL_RECORD_SIG, &aad, &nonce, &ciphertext]));
        let mut data = Vec::with_capacity(total);
        data.push(RECORD_VERSION);
        data.extend_from_slice(&head.index.to_be_bytes());
        data.extend_from_slice(head.hash.as_bytes());
        data.extend_from_slice(&counter.to_be_bytes());
        data.extend_from_slice(&nonce);
        data.extend_from_slice(&ciphertext);
        data.extend_from_slice(&signature);
        Ok(Sealed {
            epoch: self.epoch,
            data: to_ciphertext(&data).map_err(|_| SealError::TooLarge)?,
        })
    }

    /// Opens a record `context.sender` sealed, with `sender_sign` as that device's Ed25519 key.
    /// Checks format, epoch, signature and decryption only; the app uses
    /// [`EpochCipher::open_verified`], which also checks the sender and its log head against the
    /// verified log.
    pub fn open(
        &self,
        context: &RecordContext<'_>,
        sender_sign: &PublicKey,
        sealed: &Sealed,
    ) -> Result<Opened, OpenError> {
        let data = from_ciphertext(&sealed.data).ok_or(OpenError::Malformed)?;
        let parsed = parse(&data)?;
        self.open_parsed(context, sender_sign, sealed, &parsed)
    }

    fn open_parsed(
        &self,
        context: &RecordContext<'_>,
        sender_sign: &PublicKey,
        sealed: &Sealed,
        parsed: &Parsed<'_>,
    ) -> Result<Opened, OpenError> {
        let aad = self.verify_parsed(context, sender_sign, sealed, parsed)?;
        self.decrypt_parsed(context, sender_sign, parsed, &aad)
    }

    /// Epoch and signature; the associated data for decryption on success.
    fn verify_parsed(
        &self,
        context: &RecordContext<'_>,
        sender_sign: &PublicKey,
        sealed: &Sealed,
        parsed: &Parsed<'_>,
    ) -> Result<Vec<u8>, OpenError> {
        if sealed.epoch != self.epoch {
            return Err(OpenError::EpochMismatch {
                expected: self.epoch.0,
                got: sealed.epoch.0,
            });
        }
        let aad = self.aad(context, &parsed.head, parsed.counter);
        if !verify(
            sender_sign,
            &framed(&[LABEL_RECORD_SIG, &aad, parsed.nonce, parsed.ciphertext]),
            &parsed.signature,
        ) {
            return Err(OpenError::Signature);
        }
        Ok(aad)
    }

    fn decrypt_parsed(
        &self,
        context: &RecordContext<'_>,
        sender_sign: &PublicKey,
        parsed: &Parsed<'_>,
        aad: &[u8],
    ) -> Result<Opened, OpenError> {
        let plaintext = self
            .cipher()
            .map_err(|()| OpenError::Decrypt)?
            .decrypt(
                &XNonce::from(array24(parsed.nonce)),
                Payload {
                    msg: parsed.ciphertext,
                    aad,
                },
            )
            .map_err(|_| OpenError::Decrypt)?;
        Ok(Opened {
            sender: context.sender.clone(),
            head: parsed.head,
            counter: parsed.counter,
            sign: *sender_sign,
            plaintext: Zeroizing::new(plaintext),
        })
    }

    /// Opens a record against the verified log `state`:
    ///
    /// - the cipher's epoch is one of `state` (same rotate entry hash);
    /// - the sender's log head is one this device verified ([`OpenError::AheadOfLog`] otherwise:
    ///   fetch the log and try again);
    /// - the log vouched for the sender's keys at that head index, also when the device was
    ///   removed since (its key history), and the sender is no recovery device;
    /// - epoch and signature as in [`EpochCipher::open`];
    /// - that the head is not from before the epoch began ([`OpenError::HeadBeforeEpoch`]), then
    ///   decryption, and only then the head's hash ([`OpenError::Fork`]).
    ///
    /// Signature, epoch start and decryption come before the fork check: a fork is a reason to
    /// distrust a device or the team, so only a record a listed device signed over the differing
    /// head, sealed with the current epoch's key, may report one. Anyone can write a head with a
    /// wrong hash (unsigned, it is a [`OpenError::Signature`] error), and the old signing key of a
    /// device id removed and admitted again only covers heads from before the epoch.
    ///
    /// A removed device still holds the epoch's key and can name an old head, so its records
    /// stay possible until the next rotation; the relay refuses content of an old epoch.
    pub fn open_verified(
        &self,
        state: &TeamState,
        context: &RecordContext<'_>,
        sealed: &Sealed,
    ) -> Result<Opened, OpenError> {
        let record = state.epoch(self.epoch).ok_or(OpenError::UnknownEpoch)?;
        if record.hash != self.rotation || *state.team() != self.team {
            return Err(OpenError::UnknownEpoch);
        }
        let data = from_ciphertext(&sealed.data).ok_or(OpenError::Malformed)?;
        let parsed = parse(&data)?;
        let head = parsed.head;
        let known = state
            .hash_at(head.index)
            .ok_or(OpenError::AheadOfLog { index: head.index })?;
        let periods = state.key_periods(context.sender);
        if periods.is_empty() {
            return Err(OpenError::UnknownSender);
        }
        let period = periods
            .iter()
            .find(|p| p.covers(head.index))
            .ok_or(OpenError::SenderNotValid)?;
        if period.recovery {
            return Err(OpenError::RecoverySender);
        }
        let sign = period.keys.sign;
        let aad = self.verify_parsed(context, &sign, sealed, &parsed)?;
        if head.index < record.index {
            return Err(OpenError::HeadBeforeEpoch);
        }
        let opened = self.decrypt_parsed(context, &sign, &parsed, &aad)?;
        if known != head.hash {
            return Err(OpenError::Fork { index: head.index });
        }
        Ok(opened)
    }
}

/// The highest record counter seen per sender, to refuse a record shown again (a replayed note
/// or status) and anything older than what was seen. The app runs records through it where each
/// arrives once and in order, such as the relay's live event stream, and stores
/// [`ReplayGuard::marks`] so a restart does not reopen the window. Content the relay resends as
/// state (a resync's `task.reserved` with its note, a blob fetched again) is opened without it.
///
/// Marks are kept per holder and signing key: a device id that was removed and added again
/// with new keys numbers its records from 1 again, and its old mark must not refuse them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReplayGuard {
    marks: BTreeMap<(Holder, PublicKey), u64>,
}

impl ReplayGuard {
    /// A guard that has seen nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// A guard with the high-water marks the app stored.
    pub fn from_marks(marks: BTreeMap<(Holder, PublicKey), u64>) -> Self {
        Self { marks }
    }

    /// The highest counter seen per sender and signing key, to store.
    pub fn marks(&self) -> &BTreeMap<(Holder, PublicKey), u64> {
        &self.marks
    }

    /// Accepts `opened` when its counter is above the mark of its sender and signing key and
    /// raises the mark; [`OpenError::Replay`] otherwise.
    pub fn check(&mut self, opened: &Opened) -> Result<(), OpenError> {
        let mark = self
            .marks
            .entry((opened.sender.clone(), opened.sign))
            .or_insert(0);
        if opened.counter <= *mark {
            return Err(OpenError::Replay);
        }
        *mark = opened.counter;
        Ok(())
    }
}
