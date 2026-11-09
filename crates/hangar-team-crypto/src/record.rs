//! Sealed records: everything the protocol carries as `Sealed` (presence status, reservation and
//! release request notes, claim details, blobs), encrypted with the epoch's sealing key and signed
//! by the sending device.
//!
//! # Format (version 1)
//!
//! ```text
//! data = 0x01 || nonce (24 random bytes) || XChaCha20-Poly1305(seal key, nonce, plaintext, aad)
//!        || Ed25519 signature of the sender (64 bytes)
//! aad  = field("hangar-team/v1/record") || field(version) || field(team) || field(epoch, u32)
//!        || field(kind label) || field(record id) || field(sender member) || field(sender device)
//! signed message = field("hangar-team/v1/record-sig") || field(aad) || field(nonce) || field(ciphertext)
//! ```
//!
//! The seal key is `EpochKeys::seal_key` of `hangar_team_protocol::hash` (HKDF-SHA256 of the epoch
//! secret with the label `hangar-team/v1/seal-key`, the team and the epoch). The record id is what
//! the record belongs to and both sides know without the relay's word: the task's keyed hash for
//! a reservation note, the session key for a claim detail, empty when there is none. The
//! associated data binds the record to team, epoch, kind, record id and sender, so the relay
//! cannot move it elsewhere; the signature stops one member from writing in another's name, which
//! the shared key alone would allow.
//!
//! Order when opening: format, epoch, signature, then decryption, so a forged record costs no
//! decryption and a wrong sender fails as a signature error. The nonce identifies the record for
//! [`ReplayGuard`].

use std::collections::{HashSet, VecDeque};
use std::fmt;

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use hangar_team_protocol::e2e::PublicKey;
use hangar_team_protocol::hash::{EpochKeys, KeyError, MatchKey};
use hangar_team_protocol::ids::{Epoch, Sealed, TeamId};
use hangar_team_protocol::limits::{MAX_CIPHERTEXT_LEN, MAX_INLINE_CIPHERTEXT_LEN};
use hangar_team_protocol::wire::{BlobKind, Holder};
use thiserror::Error;
use zeroize::Zeroizing;

use crate::encoding::{base64url_len, framed, from_ciphertext, to_ciphertext};
use crate::entropy::{Entropy, EntropyError};
use crate::epoch::TeamSecret;
use crate::identity::{array24, verify, DeviceIdentity};

const LABEL_RECORD: &[u8] = b"hangar-team/v1/record";
const LABEL_RECORD_SIG: &[u8] = b"hangar-team/v1/record-sig";
const RECORD_VERSION: u8 = 1;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const SIGNATURE_LEN: usize = 64;
const OVERHEAD: usize = 1 + NONCE_LEN + TAG_LEN + SIGNATURE_LEN;

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
    /// The record was opened before ([`ReplayGuard`]).
    #[error("the record was seen before")]
    Replay,
}

/// The nonce that identifies one sealed record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecordNonce(pub [u8; NONCE_LEN]);

/// An opened record. `Debug` does not show the plaintext.
pub struct Opened {
    /// The record's nonce, for [`ReplayGuard`].
    pub nonce: RecordNonce,
    /// The decrypted content, wiped on drop.
    pub plaintext: Zeroizing<Vec<u8>>,
}

impl fmt::Debug for Opened {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Opened")
            .field("nonce", &self.nonce)
            .field("len", &self.plaintext.len())
            .finish()
    }
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

/// One epoch's keys for sealing, opening and keyed hashes.
#[derive(Debug)]
pub struct EpochCipher {
    team: TeamId,
    epoch: Epoch,
    keys: EpochKeys,
}

impl EpochCipher {
    /// The keys of `epoch` of `team` from its secret.
    pub fn new(secret: &TeamSecret, team: &TeamId, epoch: Epoch) -> Result<Self, KeyError> {
        Ok(Self {
            team: team.clone(),
            epoch,
            keys: secret.epoch_keys(team, epoch)?,
        })
    }

    /// The epoch.
    pub fn epoch(&self) -> Epoch {
        self.epoch
    }

    /// The key for keyed hashes of this epoch.
    pub fn match_key(&self) -> &MatchKey {
        self.keys.match_key()
    }

    fn aad(&self, context: &RecordContext<'_>) -> Vec<u8> {
        framed(&[
            LABEL_RECORD,
            &[RECORD_VERSION],
            self.team.as_str().as_bytes(),
            &self.epoch.0.to_be_bytes(),
            context.kind.label().as_bytes(),
            context.record,
            context.sender.member.as_str().as_bytes(),
            context.sender.device.as_str().as_bytes(),
        ])
    }

    fn cipher(&self) -> Result<XChaCha20Poly1305, ()> {
        XChaCha20Poly1305::new_from_slice(self.keys.seal_key().expose()).map_err(|_| ())
    }

    /// Seals `plaintext` as `context.sender`, whose private keys `identity` holds.
    pub fn seal(
        &self,
        context: &RecordContext<'_>,
        identity: &DeviceIdentity,
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
        let aad = self.aad(context);
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
        data.extend_from_slice(&nonce);
        data.extend_from_slice(&ciphertext);
        data.extend_from_slice(&signature);
        Ok(Sealed {
            epoch: self.epoch,
            data: to_ciphertext(&data).map_err(|_| SealError::TooLarge)?,
        })
    }

    /// Opens a record `context.sender` sealed; `sender_sign` is that device's Ed25519 key from
    /// the verified membership log.
    pub fn open(
        &self,
        context: &RecordContext<'_>,
        sender_sign: &PublicKey,
        sealed: &Sealed,
    ) -> Result<Opened, OpenError> {
        let data = from_ciphertext(&sealed.data).ok_or(OpenError::Malformed)?;
        let (&version, rest) = data.split_first().ok_or(OpenError::Malformed)?;
        if version != RECORD_VERSION {
            return Err(OpenError::Version(version));
        }
        if sealed.epoch != self.epoch {
            return Err(OpenError::EpochMismatch {
                expected: self.epoch.0,
                got: sealed.epoch.0,
            });
        }
        if rest.len() < NONCE_LEN + TAG_LEN + SIGNATURE_LEN {
            return Err(OpenError::Malformed);
        }
        let (nonce, rest) = rest.split_at(NONCE_LEN);
        let (ciphertext, signature) = rest.split_at(rest.len() - SIGNATURE_LEN);
        let mut sig = [0u8; SIGNATURE_LEN];
        sig.copy_from_slice(signature);
        let aad = self.aad(context);
        if !verify(
            sender_sign,
            &framed(&[LABEL_RECORD_SIG, &aad, nonce, ciphertext]),
            &sig,
        ) {
            return Err(OpenError::Signature);
        }
        let nonce = array24(nonce);
        let plaintext = self
            .cipher()
            .map_err(|()| OpenError::Decrypt)?
            .decrypt(
                &XNonce::from(nonce),
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| OpenError::Decrypt)?;
        Ok(Opened {
            nonce: RecordNonce(nonce),
            plaintext: Zeroizing::new(plaintext),
        })
    }
}

/// Remembers the nonces of the last `capacity` records opened, to refuse a record shown twice
/// where the app expects each one once (a replayed note or status). The relay's own replay after
/// a reconnect also repeats records; the app decides where a repeat is an error.
#[derive(Debug, Clone)]
pub struct ReplayGuard {
    seen: HashSet<RecordNonce>,
    order: VecDeque<RecordNonce>,
    capacity: usize,
}

impl ReplayGuard {
    /// A guard for at most `capacity` nonces (the oldest are forgotten first).
    pub fn new(capacity: usize) -> Self {
        Self {
            seen: HashSet::new(),
            order: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    /// Records `opened`'s nonce, or refuses it when it was seen before.
    pub fn check(&mut self, opened: &Opened) -> Result<(), OpenError> {
        if !self.seen.insert(opened.nonce) {
            return Err(OpenError::Replay);
        }
        self.order.push_back(opened.nonce);
        while self.order.len() > self.capacity {
            if let Some(old) = self.order.pop_front() {
                self.seen.remove(&old);
            }
        }
        Ok(())
    }
}
