//! Team decisions (protocol version 4, ADR 0021): a short statement with its reason that every
//! agent of the team may read. A decision is a `blob.put` of the kind `decision`, sealed with the
//! newest key of the team's notes key ring ([`crate::notes::KeyRing`], ADR 0006 phase 2) and signed
//! by the writing device, checked against the verified membership log like every record.
//!
//! The notes key ring is not bound to an epoch: a member admitted later reads every decision, and
//! a rotation re-seals nothing. After a removal the rotating device adds a new notes key, so what
//! is written afterwards is closed to whoever left.
//!
//! # Format (version 1)
//!
//! ```text
//! data = 0x01 || notes key id (8 bytes) || decision id (16 bytes, the UUID's bytes)
//!        || head index (8 bytes) || head hash (32 bytes) || counter (8 bytes)
//!        || nonce (24 random bytes) || XChaCha20-Poly1305(key, nonce, padded, aad)
//!        || Ed25519 signature of the sender (64 bytes)
//! key  = HKDF-SHA256(salt none, ikm = notes key,
//!                    info = field("hangar-decision/v1/key") || field(team))
//! aad  = field("hangar-team/v1/record") || field(0x01) || field(team) || field(epoch, u32)
//!        || field("decision/v1") || field(notes key id) || field(decision id as text)
//!        || field(sender member) || field(sender device)
//!        || field(head index, u64) || field(head hash) || field(counter, u64)
//! signed message = field("hangar-team/v1/record-sig") || field(aad) || field(nonce)
//!                  || field(ciphertext)
//! padded = plaintext length (u32 big endian) || plaintext || zero bytes, to a multiple of 256
//! ```
//!
//! The associated data has the shape of an epoch record's ([`crate::record`]) with the notes key
//! id after the kind label; the kind label `decision/v1` is used by nothing else, so no decision
//! opens as an epoch record and no record as a decision. The envelope's epoch is the team's
//! current epoch at the time of writing: it keeps the relay's stale epoch check (`blob.put`
//! carries content of the current epoch only) and is bound in the associated data, but chooses
//! no key.
//!
//! **The decision id** is the decision's lower case UUID, made by the writer. It travels in the
//! clear (the opener needs it for the associated data before decrypting) and the document inside
//! must name the same id (the app's check), so a blob the relay copies opens only as the same
//! decision, never as another one. The relay's blob id is not bound: it is assigned after sealing.
//!
//! Order when opening ([`open_decision`]): format, the notes key, a head index this device
//! verified, the sender's keys at that index, signature, the epoch and its start, decryption, and
//! only then the head's hash (fork), as [`crate::record::EpochCipher::open_verified`] does.
//!
//! **Removed senders.** A decision whose sender the log no longer lists still opens when it
//! names a head at which the log vouched for its keys: people leave, their decisions stay the
//! team's. What a removed device writes later needs a relay that accepts its `blob.put` (the
//! relay refuses devices the log does not list) and a notes key it still holds; the key added
//! after the removal closes everything new. Owners and admins can withdraw any decision.

use std::fmt;

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use hangar_team_protocol::e2e::{ChainHead, LogHash, PublicKey};
use hangar_team_protocol::hash::hkdf_sha256;
use hangar_team_protocol::ids::{Epoch, Sealed, TeamId};
use hangar_team_protocol::limits::MAX_DECISION_CIPHERTEXT_LEN;
use hangar_team_protocol::wire::Holder;
use thiserror::Error;
use zeroize::Zeroizing;

use crate::encoding::{base64url_len, framed, from_ciphertext, to_ciphertext, Writer};
use crate::entropy::{Entropy, EntropyError};
use crate::identity::{array24, array32, verify, DeviceIdentity};
use crate::memberlog::TeamState;
use crate::notes::{is_note_id, pad, unpad, KeyId, KeyRing, NotesKey};
use crate::record::{OpenError, LABEL_RECORD, LABEL_RECORD_SIG};

const LABEL_KEY: &[u8] = b"hangar-decision/v1/key";
/// The kind label in the associated data.
pub const DECISION_LABEL: &str = "decision/v1";
const DECISION_VERSION: u8 = 1;
const KEY_ID_LEN: usize = 8;
const ID_LEN: usize = 16;
const HEADER_LEN: usize = 1 + KEY_ID_LEN + ID_LEN + 8 + 32 + 8;
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const SIGNATURE_LEN: usize = 64;
/// The largest decision document sealed: far above the app's 300 plus 1000 characters (4 bytes
/// each at most) and its metadata, far below the blob limit.
pub const MAX_DECISION_PLAINTEXT: usize = 16 * 1024;

/// Why a decision was not sealed or opened.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DecisionError {
    /// Not a decision of this format.
    #[error("the decision is malformed")]
    Malformed,
    /// A format version this crate does not know.
    #[error("decision format version {0} is not supported")]
    Version(u8),
    /// The decision id is not a lower case UUID.
    #[error("the decision id is not a lower case UUID")]
    Id,
    /// The document is larger than [`MAX_DECISION_PLAINTEXT`].
    #[error("the decision is too large")]
    TooLarge,
    /// Sealed with a notes key this device does not hold (yet).
    #[error("the decision was sealed with a notes key this device does not have")]
    UnknownKey(KeyId),
    /// The checks every record goes through: sender, log head, signature, epoch, decryption,
    /// fork ([`OpenError`]).
    #[error(transparent)]
    Record(#[from] OpenError),
    /// No randomness for the nonce.
    #[error(transparent)]
    Entropy(#[from] EntropyError),
    /// The cipher refused (not expected).
    #[error("sealing failed")]
    Cipher,
}

/// An opened decision. `Debug` does not show the plaintext.
pub struct OpenedDecision {
    /// The decision's id (lower case UUID), as bound in the associated data.
    pub id: String,
    /// The device that sealed it.
    pub sender: Holder,
    /// The sender's log head when it sealed.
    pub head: ChainHead,
    /// The sender's record counter.
    pub counter: u64,
    /// The sender's Ed25519 key the signature verified with.
    pub sign: PublicKey,
    /// The notes key that sealed it.
    pub key: KeyId,
    /// The decrypted document, wiped on drop.
    pub plaintext: Zeroizing<Vec<u8>>,
}

impl fmt::Debug for OpenedDecision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenedDecision")
            .field("id", &self.id)
            .field("sender", &self.sender)
            .field("head", &self.head)
            .field("counter", &self.counter)
            .field("key", &self.key)
            .field("len", &self.plaintext.len())
            .finish()
    }
}

/// What a sealed decision says about itself in the clear, read without a key and **without**
/// checking anything: never a reason to trust it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecisionHeader {
    /// The notes key it names.
    pub key: KeyId,
    /// The decision id it names.
    pub id: String,
    /// The log head it names.
    pub head: ChainHead,
    /// The counter it names.
    pub counter: u64,
}

struct Parsed<'a> {
    header: DecisionHeader,
    nonce: &'a [u8],
    ciphertext: &'a [u8],
    signature: [u8; SIGNATURE_LEN],
}

/// The 16 bytes of a lower case UUID.
fn id_bytes(id: &str) -> Option<[u8; ID_LEN]> {
    if !is_note_id(id) {
        return None;
    }
    let digits: Vec<u8> = id.bytes().filter(|b| *b != b'-').collect();
    let mut out = [0u8; ID_LEN];
    for (index, pair) in digits.as_chunks::<2>().0.iter().enumerate() {
        let value = |b: u8| match b {
            b'0'..=b'9' => Some(b - b'0'),
            b'a'..=b'f' => Some(b - b'a' + 10),
            _ => None,
        };
        out[index] = (value(pair[0])? << 4) | value(pair[1])?;
    }
    Some(out)
}

/// The lower case UUID text of 16 bytes.
fn id_text(bytes: &[u8; ID_LEN]) -> String {
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

fn u64_of(bytes: &[u8]) -> u64 {
    let mut out = [0u8; 8];
    out.copy_from_slice(bytes);
    u64::from_be_bytes(out)
}

fn parse(data: &[u8]) -> Result<Parsed<'_>, DecisionError> {
    let (&version, _) = data.split_first().ok_or(DecisionError::Malformed)?;
    if version != DECISION_VERSION {
        return Err(DecisionError::Version(version));
    }
    if data.len() < HEADER_LEN + NONCE_LEN + TAG_LEN + SIGNATURE_LEN {
        return Err(DecisionError::Malformed);
    }
    let rest = &data[1..];
    let (key, rest) = rest.split_at(KEY_ID_LEN);
    let (id, rest) = rest.split_at(ID_LEN);
    let (index, rest) = rest.split_at(8);
    let (hash, rest) = rest.split_at(32);
    let (counter, rest) = rest.split_at(8);
    let (nonce, rest) = rest.split_at(NONCE_LEN);
    let (ciphertext, signature) = rest.split_at(rest.len() - SIGNATURE_LEN);
    let mut sig = [0u8; SIGNATURE_LEN];
    sig.copy_from_slice(signature);
    let mut id_array = [0u8; ID_LEN];
    id_array.copy_from_slice(id);
    let mut key_array = [0u8; KEY_ID_LEN];
    key_array.copy_from_slice(key);
    Ok(Parsed {
        header: DecisionHeader {
            key: KeyId::from_bytes(key_array),
            id: id_text(&id_array),
            head: ChainHead {
                index: u64_of(index),
                hash: LogHash::from_bytes(array32(hash)),
            },
            counter: u64_of(counter),
        },
        nonce,
        ciphertext,
        signature: sig,
    })
}

/// Reads a sealed decision's clear header ([`DecisionHeader`]); `None` for data that is not a
/// decision of this version.
pub fn decision_header(sealed: &Sealed) -> Option<DecisionHeader> {
    let data = from_ciphertext(&sealed.data)?;
    parse(&data).ok().map(|parsed| parsed.header)
}

/// What the associated data binds besides the header.
struct Context<'a> {
    team: &'a TeamId,
    epoch: Epoch,
    sender: &'a Holder,
}

fn aad(context: &Context<'_>, header: &DecisionHeader, key_id: &[u8]) -> Vec<u8> {
    let mut w = Writer::new();
    w.field(LABEL_RECORD)
        .field(&[DECISION_VERSION])
        .field(context.team.as_str().as_bytes())
        .field(&context.epoch.0.to_be_bytes())
        .field(DECISION_LABEL.as_bytes())
        .field(key_id)
        .field(header.id.as_bytes())
        .field(context.sender.member.as_str().as_bytes())
        .field(context.sender.device.as_str().as_bytes())
        .u64(header.head.index)
        .field(header.head.hash.as_bytes())
        .u64(header.counter);
    w.into_bytes()
}

fn cipher(key: &NotesKey, team: &TeamId) -> Result<XChaCha20Poly1305, DecisionError> {
    let mut derived = Zeroizing::new([0u8; 32]);
    hkdf_sha256(key.secret())
        .expand(
            &framed(&[LABEL_KEY, team.as_str().as_bytes()]),
            derived.as_mut(),
        )
        .map_err(|_| DecisionError::Cipher)?;
    XChaCha20Poly1305::new_from_slice(derived.as_ref()).map_err(|_| DecisionError::Cipher)
}

/// Seals the decision document `plaintext` with the ring's newest key, as `sender` (whose
/// private keys `identity` holds) with its verified log `head` and the next number of its
/// record counter. `epoch` is the team's current epoch (the envelope's, for the relay's check).
#[allow(clippy::too_many_arguments)]
pub fn seal_decision(
    ring: &KeyRing,
    team: &TeamId,
    epoch: Epoch,
    id: &str,
    sender: &Holder,
    identity: &DeviceIdentity,
    head: &ChainHead,
    counter: u64,
    plaintext: &[u8],
    entropy: &mut impl Entropy,
) -> Result<Sealed, DecisionError> {
    let id_raw = id_bytes(id).ok_or(DecisionError::Id)?;
    if plaintext.len() > MAX_DECISION_PLAINTEXT {
        return Err(DecisionError::TooLarge);
    }
    let padded = pad(plaintext).map_err(|_| DecisionError::TooLarge)?;
    let key = ring.newest();
    let key_raw = *key.id().as_bytes();
    let header = DecisionHeader {
        key: key.id(),
        id: id.to_owned(),
        head: *head,
        counter,
    };
    let total = HEADER_LEN + NONCE_LEN + padded.len() + TAG_LEN + SIGNATURE_LEN;
    if base64url_len(total) > MAX_DECISION_CIPHERTEXT_LEN {
        return Err(DecisionError::TooLarge);
    }
    let mut nonce = [0u8; NONCE_LEN];
    entropy.fill(&mut nonce)?;
    let context = Context {
        team,
        epoch,
        sender,
    };
    let aad = aad(&context, &header, &key_raw);
    let ciphertext = cipher(key, team)?
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &padded,
                aad: &aad,
            },
        )
        .map_err(|_| DecisionError::Cipher)?;
    let signature = identity.sign(&framed(&[LABEL_RECORD_SIG, &aad, &nonce, &ciphertext]));
    let mut data = Vec::with_capacity(total);
    data.push(DECISION_VERSION);
    data.extend_from_slice(&key_raw);
    data.extend_from_slice(&id_raw);
    data.extend_from_slice(&head.index.to_be_bytes());
    data.extend_from_slice(head.hash.as_bytes());
    data.extend_from_slice(&counter.to_be_bytes());
    data.extend_from_slice(&nonce);
    data.extend_from_slice(&ciphertext);
    data.extend_from_slice(&signature);
    Ok(Sealed {
        epoch,
        data: to_ciphertext(&data).map_err(|_| DecisionError::TooLarge)?,
    })
}

/// Opens a decision `sender` sealed (the relay names the sender in `blob.list`; the signature
/// must verify for it), against the verified log `state`, with a key of `ring`:
///
/// - the notes key the header names is in `ring` ([`DecisionError::UnknownKey`] otherwise: the
///   ring may grow with the next `notes.keys`);
/// - the sender's log head is one this device verified ([`OpenError::AheadOfLog`] otherwise:
///   fetch the log and try again);
/// - the log vouched for the sender's keys at that head index, also when the device was removed
///   since, and the sender is no recovery device;
/// - the signature over the associated data, nonce and ciphertext;
/// - the envelope's epoch is one of the log and the head is not from before it began;
/// - decryption and padding, and only then the head's hash ([`OpenError::Fork`]).
pub fn open_decision(
    ring: &KeyRing,
    state: &TeamState,
    sender: &Holder,
    sealed: &Sealed,
) -> Result<OpenedDecision, DecisionError> {
    // Nothing larger than a decision is decoded or kept (the relay should refuse it as well).
    if sealed.data.as_str().len() > MAX_DECISION_CIPHERTEXT_LEN {
        return Err(DecisionError::TooLarge);
    }
    let data = from_ciphertext(&sealed.data).ok_or(DecisionError::Malformed)?;
    let parsed = parse(&data)?;
    let header = &parsed.header;
    let key = ring
        .get(header.key)
        .ok_or(DecisionError::UnknownKey(header.key))?;
    let key_raw = *header.key.as_bytes();
    let context = Context {
        team: state.team(),
        epoch: sealed.epoch,
        sender,
    };
    let aad = aad(&context, header, &key_raw);
    let signed_by = |sign: &PublicKey| {
        verify(
            sign,
            &framed(&[LABEL_RECORD_SIG, &aad, parsed.nonce, parsed.ciphertext]),
            &parsed.signature,
        )
    };
    let Some(known) = state.hash_at(header.head.index) else {
        let signed = state
            .key_periods(sender)
            .last()
            .filter(|p| p.removed_at.is_none() && !p.recovery)
            .is_some_and(|p| signed_by(&p.keys.sign));
        return Err(OpenError::AheadOfLog {
            index: header.head.index,
            signed,
        }
        .into());
    };
    let periods = state.key_periods(sender);
    if periods.is_empty() {
        return Err(OpenError::UnknownSender.into());
    }
    let period = periods
        .iter()
        .find(|p| p.covers(header.head.index))
        .ok_or(OpenError::SenderNotValid)?;
    if period.recovery {
        return Err(OpenError::RecoverySender.into());
    }
    let sign = period.keys.sign;
    if !signed_by(&sign) {
        return Err(OpenError::Signature.into());
    }
    let epoch = state.epoch(sealed.epoch).ok_or(OpenError::UnknownEpoch)?;
    if header.head.index < epoch.index {
        return Err(OpenError::HeadBeforeEpoch.into());
    }
    let padded = Zeroizing::new(
        cipher(key, state.team())?
            .decrypt(
                &XNonce::from(array24(parsed.nonce)),
                Payload {
                    msg: parsed.ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| OpenError::Decrypt)?,
    );
    if known != header.head.hash {
        return Err(OpenError::Fork {
            index: header.head.index,
        }
        .into());
    }
    let plaintext = unpad(&padded).map_err(|_| DecisionError::Malformed)?;
    Ok(OpenedDecision {
        id: header.id.clone(),
        sender: sender.clone(),
        head: header.head,
        counter: header.counter,
        sign,
        key: header.key,
        plaintext,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_only_lower_case_uuids_count() {
        let id = "00112233-4455-4677-8899-aabbccddeeff";
        let bytes = id_bytes(id).expect("uuid");
        assert_eq!(bytes[0], 0x00);
        assert_eq!(bytes[15], 0xff);
        assert_eq!(id_text(&bytes), id);
        assert_eq!(id_bytes("00112233-4455-4677-8899-AABBCCDDEEFF"), None);
        assert_eq!(id_bytes("not-a-uuid"), None);
    }

    #[test]
    fn the_largest_document_fits_the_decision_blob_limit() {
        let blocks = (4 + MAX_DECISION_PLAINTEXT).div_ceil(256) * 256;
        let total = HEADER_LEN + NONCE_LEN + blocks + TAG_LEN + SIGNATURE_LEN;
        assert!(base64url_len(total) <= MAX_DECISION_CIPHERTEXT_LEN);
    }

    #[test]
    fn garbage_is_malformed_and_another_version_says_so() {
        assert!(matches!(parse(&[]), Err(DecisionError::Malformed)));
        assert!(matches!(parse(&[2; 300]), Err(DecisionError::Version(2))));
        assert!(matches!(parse(&[1; 40]), Err(DecisionError::Malformed)));
    }
}
