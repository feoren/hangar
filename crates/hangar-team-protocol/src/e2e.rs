//! Groundwork of end-to-end encryption (step E) that the relay checks without holding a key:
//! device public keys, the hash chain of the membership log, and the rules of a key rotation.
//! The cryptography itself (signatures, HPKE wrapping, sealing) is not part of this crate; the
//! log entries and wrapped keys travel as opaque [`Ciphertext`].
//!
//! # Membership log hash
//!
//! Every entry of a team's membership log names the hash of its predecessor, so the relay cannot
//! drop, reorder or swap entries without the clients noticing. The hash of entry `index` is
//!
//! ```text
//! SHA-256( field("hangar-team/v1/memberlog") || field(index as 8 bytes, big endian)
//!          || field(prev as 32 raw bytes, or empty for the first entry)
//!          || field(entry as the bytes its base64url text decodes to) )
//! ```
//!
//! where `field(x)` is `x`'s length as a 32-bit big endian number followed by `x`, the same
//! framing as the keyed hashes in [`crate::hash`]. The label separates it from every other hash
//! of the protocol, and the length prefixes make the encoding canonical: no two different
//! `(index, prev, entry)` triples give the same input. The entry is hashed as bytes, not as text,
//! and only canonical base64url is accepted (no padding, unused trailing bits zero), so one entry
//! has exactly one spelling and one hash. Test vectors: this module's tests, computed
//! independently by `test_vectors.py`.
//!
//! Indexes start at 1 and have no gaps; the first entry has no `prev`.
//!
//! The chain hash does not contain the team id, so two teams' logs can share entries byte for
//! byte. Step E must therefore put the team id into the **signed content** of every entry
//! (and clients must check it), or a relay could move a validly signed entry, such as "device
//! X approved", from one team's log into another's.
//!
//! # Rights
//!
//! The relay passes the role from the connection's verified ticket and the holder it bound to
//! the connection (ticket subject plus `hello.device`); nothing a frame says about itself counts.
//! Rights that depend on the membership log are checked in `hangar-team-crypto`, which holds
//! the verified `TeamState`: a member's effective role is the lower of its role in the log and
//! its ticket's role, so a downgrade on the website applies at once, before the log follows.
//!
//! | Intent | Allowed for | Refused with |
//! | --- | --- | --- |
//! | `memberlog.append` | decided per entry by `hangar_team_crypto::memberlog::LogVerifier::admit` (signature, rights, team, index, `prev`, ticket role); a rotate epoch entry only inside `key.rotate`, a remove device entry only inside `device.remove` | the error's `code()` |
//! | `key.rotate` | owner, admin ([`check_rotate`]), then the signed rotate epoch entry in `log` by `LogVerifier::admit_rotation` | `forbidden`, `staleEpoch`, `badRequest` |
//! | `key.wrap` | a device of the log: an owner or admin for any other device of the log, a member for another device of its own; one package per sender, target and epoch (`hangar_team_crypto::relay::check_wrap`) | `forbidden`, `notFound`, `conflict`; `limit` when the relay already keeps [`crate::limits::MAX_KEY_PACKAGES`] for the device in the epoch |
//! | `device.remove` | the signed remove device entry in `log`, by `LogVerifier::admit_device_remove` | the error's `code()` |
//!
//! The relay depends on `hangar-team-crypto` (MIT, pure, no secrets) for the membership log
//! checks: the same verifier the clients run, so the relay never stores an entry a client would
//! refuse. Clients still verify every entry themselves.

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::hash::{hex32, length_prefix, parse_hex32};
use crate::ids::{Ciphertext, Epoch};
use crate::wire::{ErrorCode, KeyRotate, MemberLogAppend, MemberLogEntry, Role};

const LABEL_MEMBERLOG: &[u8] = b"hangar-team/v1/memberlog";

/// Bytes of an Ed25519 or X25519 public key.
pub const PUBLIC_KEY_BYTES: usize = 32;

/// Characters of a 32-byte key as base64url without padding.
pub const PUBLIC_KEY_CHARS: usize = 43;

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn base64_value(c: u8) -> Option<u32> {
    let value = match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'-' => 62,
        b'_' => 63,
        _ => return None,
    };
    Some(u32::from(value))
}

/// Canonical base64url without padding: a length that cannot occur (`len % 4 == 1`), a
/// character outside the alphabet (padding included) or a set unused trailing bit is `None`.
pub(crate) fn decode_base64url(text: &str) -> Option<Vec<u8>> {
    let input = text.as_bytes();
    if input.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for &c in input {
        acc = (acc << 6) | base64_value(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from(acc >> bits).ok()?);
            acc &= (1 << bits) - 1;
        }
    }
    // The bits left over pad the last character and must be zero.
    if acc != 0 {
        return None;
    }
    Some(out)
}

/// Base64url without padding.
pub(crate) fn encode_base64url(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let chars = chunk.len() + 1;
        for i in 0..chars {
            let index = (n >> (18 - 6 * i)) & 0x3f;
            out.push(char::from(ALPHABET[index as usize]));
        }
    }
    out
}

/// Not a 32-byte key as canonical base64url (43 characters, no padding).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("a public key is 32 bytes as 43 characters of base64url without padding")]
pub struct InvalidPublicKey;

/// A device's Ed25519 (`sign`) or X25519 (`kem`) public key: 32 bytes, on the wire 43
/// characters of canonical base64url. The crate checks only the encoding and the length;
/// whether the bytes are a valid curve point is step E's job, and clients trust a key only when
/// the membership log vouches for it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct PublicKey([u8; PUBLIC_KEY_BYTES]);

impl PublicKey {
    /// Wraps the raw key bytes.
    pub fn from_bytes(bytes: [u8; PUBLIC_KEY_BYTES]) -> Self {
        Self(bytes)
    }

    /// The raw key bytes.
    pub fn as_bytes(&self) -> &[u8; PUBLIC_KEY_BYTES] {
        &self.0
    }

    /// The 43 base64url characters the wire carries.
    pub fn to_base64url(&self) -> String {
        encode_base64url(&self.0)
    }
}

impl TryFrom<String> for PublicKey {
    type Error = InvalidPublicKey;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != PUBLIC_KEY_CHARS {
            return Err(InvalidPublicKey);
        }
        let bytes = decode_base64url(&value).ok_or(InvalidPublicKey)?;
        let bytes: [u8; PUBLIC_KEY_BYTES] = bytes.try_into().map_err(|_| InvalidPublicKey)?;
        Ok(Self(bytes))
    }
}

impl From<PublicKey> for String {
    fn from(value: PublicKey) -> String {
        value.to_base64url()
    }
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PublicKey({})", self.to_base64url())
    }
}

/// Bytes of an Ed25519 signature.
pub const KEY_PROOF_BYTES: usize = 64;

/// Characters of a 64-byte signature as base64url without padding.
pub const KEY_PROOF_CHARS: usize = 86;

/// Not a 64-byte signature as canonical base64url (86 characters, no padding).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("a key proof is 64 bytes as 86 characters of base64url without padding")]
pub struct InvalidKeyProof;

/// A device's proof of possession (step E): its Ed25519 signature, made with the `sign` key it
/// publishes, over the team, its holder and both public keys. The membership log accepts a
/// device only with this proof, so nobody can enter keys into the log that they do not hold.
/// The crate checks only the encoding and the length; verifying it is step E's job.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct KeyProof([u8; KEY_PROOF_BYTES]);

impl KeyProof {
    /// Wraps the raw signature bytes.
    pub fn from_bytes(bytes: [u8; KEY_PROOF_BYTES]) -> Self {
        Self(bytes)
    }

    /// The raw signature bytes.
    pub fn as_bytes(&self) -> &[u8; KEY_PROOF_BYTES] {
        &self.0
    }

    /// The 86 base64url characters the wire carries.
    pub fn to_base64url(&self) -> String {
        encode_base64url(&self.0)
    }
}

impl TryFrom<String> for KeyProof {
    type Error = InvalidKeyProof;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.len() != KEY_PROOF_CHARS {
            return Err(InvalidKeyProof);
        }
        let bytes = decode_base64url(&value).ok_or(InvalidKeyProof)?;
        let bytes: [u8; KEY_PROOF_BYTES] = bytes.try_into().map_err(|_| InvalidKeyProof)?;
        Ok(Self(bytes))
    }
}

impl From<KeyProof> for String {
    fn from(value: KeyProof) -> String {
        value.to_base64url()
    }
}

impl fmt::Debug for KeyProof {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyProof({})", self.to_base64url())
    }
}

/// Not 64 lowercase hex digits.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("a membership log hash is 64 lowercase hex digits")]
pub struct InvalidLogHash;

/// The SHA-256 of a membership log entry (see the module documentation): 32 bytes, 64
/// lowercase hex digits on the wire. Unkeyed, unlike [`crate::hash::KeyedHash`]: everyone,
/// the relay included, can compute and check it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct LogHash([u8; 32]);

impl LogHash {
    /// Wraps 32 bytes.
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw 32 bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The 64 lowercase hex digits the wire carries.
    pub fn to_hex(&self) -> String {
        hex32(&self.0)
    }
}

impl TryFrom<String> for LogHash {
    type Error = InvalidLogHash;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        parse_hex32(&value).map(Self).ok_or(InvalidLogHash)
    }
}

impl From<LogHash> for String {
    fn from(value: LogHash) -> String {
        value.to_hex()
    }
}

impl fmt::Debug for LogHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LogHash({})", self.to_hex())
    }
}

impl fmt::Display for LogHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// Why a membership log entry does not continue the chain.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ChainError {
    /// The index is already taken. Relay side: another device appended first; the client
    /// fetches the new entries (`memberlog.get`) and tries again on top of them. Client side
    /// ([`MemberChain::verify`]): the relay sent an entry at or below the trusted head, which
    /// an honest relay never does when the client asks with `after` set to its head's index.
    #[error("membership log index {index} is already taken")]
    Taken { index: u64 },
    /// The index skips entries (or is 0).
    #[error("membership log index {got} leaves a gap (expected {expected})")]
    Gap { expected: u64, got: u64 },
    /// `prev` is not the hash of the entry before (or is set on the first entry, or missing on
    /// a later one).
    #[error("`prev` is not the hash of the previous membership log entry")]
    Prev,
    /// The entry is not canonical base64url.
    #[error("the membership log entry is not canonical base64url")]
    Entry,
    /// A received entry's `hash` differs from the hash computed over it (client side): the
    /// relay changed the log.
    #[error("the hash of membership log entry {index} does not match its content")]
    Hash { index: u64 },
    /// The log reached the largest index.
    #[error("the membership log is full")]
    Full,
}

impl ChainError {
    /// The `err` code the relay answers a refused `memberlog.append` with.
    pub fn code(&self) -> ErrorCode {
        match self {
            ChainError::Taken { .. } => ErrorCode::Conflict,
            ChainError::Full => ErrorCode::Limit,
            ChainError::Gap { .. }
            | ChainError::Prev
            | ChainError::Entry
            | ChainError::Hash { .. } => ErrorCode::BadRequest,
        }
    }
}

/// The hash of membership log entry `index` with predecessor hash `prev` (`None` for the first
/// entry); fails only for an entry that is not canonical base64url.
pub fn entry_hash(
    index: u64,
    prev: Option<&LogHash>,
    entry: &Ciphertext,
) -> Result<LogHash, ChainError> {
    let entry = decode_base64url(entry.as_str()).ok_or(ChainError::Entry)?;
    let index = index.to_be_bytes();
    let prev: &[u8] = prev.map_or(&[], |p| p.as_bytes());
    let mut sha = Sha256::new();
    for part in [LABEL_MEMBERLOG, &index, prev, &entry] {
        sha.update(length_prefix(part));
        sha.update(part);
    }
    Ok(LogHash(sha.finalize().into()))
}

/// The newest entry of a membership log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainHead {
    pub index: u64,
    pub hash: LogHash,
}

/// The relay's check of a `memberlog.append` against the log's newest entry (`None`: empty
/// log): the index is the next one, `prev` the head's hash, the entry canonical. Returns the new
/// head, whose hash goes into `memberlog.appended`.
pub fn check_append(
    head: Option<&ChainHead>,
    append: &MemberLogAppend,
) -> Result<ChainHead, ChainError> {
    let expected = next_index(head)?;
    check_index(expected, append.index)?;
    if append.prev.as_ref() != head.map(|h| &h.hash) {
        return Err(ChainError::Prev);
    }
    let hash = entry_hash(append.index, append.prev.as_ref(), &append.entry)?;
    Ok(ChainHead {
        index: append.index,
        hash,
    })
}

fn next_index(head: Option<&ChainHead>) -> Result<u64, ChainError> {
    match head {
        None => Ok(1),
        Some(head) => head.index.checked_add(1).ok_or(ChainError::Full),
    }
}

fn check_index(expected: u64, got: u64) -> Result<(), ChainError> {
    if got == 0 || got > expected {
        return Err(ChainError::Gap { expected, got });
    }
    if got < expected {
        return Err(ChainError::Taken { index: got });
    }
    Ok(())
}

/// A membership log as far as one side knows it: the relay appends to it, a client verifies
/// the entries `memberlog.entries` brings against the hash it already trusts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemberChain {
    head: Option<ChainHead>,
}

impl MemberChain {
    /// An empty log.
    pub fn new() -> Self {
        Self::default()
    }

    /// A log whose newest entry is `head` (from storage, or the last entry a client verified).
    pub fn resume(head: Option<ChainHead>) -> Self {
        Self { head }
    }

    /// The newest entry, if any.
    pub fn head(&self) -> Option<&ChainHead> {
        self.head.as_ref()
    }

    /// The relay's side: checks `append` with [`check_append`] and makes it the head.
    pub fn append(&mut self, append: &MemberLogAppend) -> Result<LogHash, ChainError> {
        let head = check_append(self.head.as_ref(), append)?;
        self.head = Some(head);
        Ok(head.hash)
    }

    /// The client's side: `entry` is the next one, and its stated hash is the one computed
    /// from the trusted head's hash and the entry. A client never takes `hash` from the relay
    /// without this check. On an error the head stays where it was.
    pub fn verify(&mut self, entry: &MemberLogEntry) -> Result<(), ChainError> {
        let expected = next_index(self.head.as_ref())?;
        check_index(expected, entry.index)?;
        let prev = self.head.as_ref().map(|h| h.hash);
        let hash = entry_hash(entry.index, prev.as_ref(), &entry.entry)?;
        if hash != entry.hash {
            return Err(ChainError::Hash { index: entry.index });
        }
        self.head = Some(ChainHead {
            index: entry.index,
            hash,
        });
        Ok(())
    }
}

/// Why a `key.rotate` is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyRotateError {
    /// Only an owner or an admin starts a new epoch.
    #[error("only an owner or an admin rotates the team key")]
    Forbidden,
    /// The epoch is not exactly the current one plus one (`expected` is `None` when the current
    /// epoch is the largest).
    #[error("`key.rotate` names epoch {got:?}, expected {expected:?}")]
    Epoch { expected: Option<Epoch>, got: Epoch },
    /// No wrapped key at all.
    #[error("`key.rotate` wraps the key for no device")]
    Empty,
    /// One holder (member and device) has two wrapped keys.
    #[error("`key.rotate` wraps the key twice for one device")]
    DuplicateDevice,
}

impl KeyRotateError {
    /// The `err` code the relay answers with.
    pub fn code(&self) -> ErrorCode {
        match self {
            KeyRotateError::Forbidden => ErrorCode::Forbidden,
            KeyRotateError::Epoch { .. } => ErrorCode::StaleEpoch,
            KeyRotateError::Empty | KeyRotateError::DuplicateDevice => ErrorCode::BadRequest,
        }
    }
}

/// The relay's first check of a `key.rotate` by a member of `role` while the team is at
/// `current`: owner or admin only, the epoch exactly `current + 1`, at least one wrapped key and
/// at most one per holder (the same device id under two members is two devices). Size limits
/// are checked when the frame is decoded. The relay then hands the whole message to
/// `hangar_team_crypto::memberlog::LogVerifier::admit_rotation`, which verifies the signed rotate
/// epoch entry in `rotate.log` (signature, rights, team, index, `prev`, epoch) and that every
/// wrapped key names a device of the log; it stores the entry and the wrapped keys together or
/// neither.
pub fn check_rotate(role: Role, current: Epoch, rotate: &KeyRotate) -> Result<(), KeyRotateError> {
    if !is_owner_or_admin(role) {
        return Err(KeyRotateError::Forbidden);
    }
    let expected = current.0.checked_add(1).map(Epoch);
    if expected != Some(rotate.epoch) {
        return Err(KeyRotateError::Epoch {
            expected,
            got: rotate.epoch,
        });
    }
    if rotate.wrapped.is_empty() {
        return Err(KeyRotateError::Empty);
    }
    let mut seen = HashSet::with_capacity(rotate.wrapped.len());
    if !rotate.wrapped.iter().all(|w| seen.insert(&w.holder)) {
        return Err(KeyRotateError::DuplicateDevice);
    }
    Ok(())
}

fn is_owner_or_admin(role: Role) -> bool {
    matches!(role, Role::Owner | Role::Admin)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{DeviceId, MemberId, UnixMillis};
    use crate::wire::{Holder, WrappedKey};

    // Vectors computed independently with Python's standard library (`test_vectors.py`).
    const LOG_ENTRY_1: &str = "aGFuZ2FyIG1lbWJlciBsb2cgZW50cnkgMQ";
    const LOG_ENTRY_2: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8gISIjJCUmJw";
    const VECTOR_LOG_1: &str = "e0f7c0d8715c9d363bca28bc9942abeda40a14a5ad99a533906d1a4ad24b4739";
    const VECTOR_LOG_2: &str = "6bc9ecab92aaa4875f40ced4cac7ec979703a9b7b5d01f421b98900ba05f9225";

    fn hash(hex: &str) -> LogHash {
        LogHash::try_from(hex.to_owned()).unwrap()
    }

    fn text(value: &str) -> Ciphertext {
        Ciphertext::new(value).unwrap()
    }

    fn append(index: u64, prev: Option<LogHash>, entry: &str) -> MemberLogAppend {
        MemberLogAppend {
            index,
            prev,
            entry: text(entry),
        }
    }

    fn entry(index: u64, hash: LogHash, value: &str) -> MemberLogEntry {
        MemberLogEntry {
            index,
            hash,
            entry: text(value),
            by: Holder {
                member: MemberId::new("m1").unwrap(),
                device: DeviceId::new("d1").unwrap(),
            },
            at: UnixMillis(1),
        }
    }

    #[test]
    fn memberlog_hash_vectors() {
        assert_eq!(
            decode_base64url(LOG_ENTRY_1).unwrap(),
            b"hangar member log entry 1"
        );
        assert_eq!(
            decode_base64url(LOG_ENTRY_2).unwrap(),
            (0u8..40).collect::<Vec<_>>()
        );
        let first = entry_hash(1, None, &text(LOG_ENTRY_1)).unwrap();
        assert_eq!(first, hash(VECTOR_LOG_1));
        let second = entry_hash(2, Some(&first), &text(LOG_ENTRY_2)).unwrap();
        assert_eq!(second, hash(VECTOR_LOG_2));
        // Every field counts.
        assert_ne!(
            entry_hash(3, Some(&first), &text(LOG_ENTRY_2)).unwrap(),
            second
        );
        assert_ne!(entry_hash(2, None, &text(LOG_ENTRY_2)).unwrap(), second);
        assert_ne!(
            entry_hash(2, Some(&second), &text(LOG_ENTRY_2)).unwrap(),
            second
        );
    }

    #[test]
    fn the_relay_checks_the_chain() {
        let mut chain = MemberChain::new();
        assert_eq!(
            chain.append(&append(2, None, LOG_ENTRY_1)),
            Err(ChainError::Gap {
                expected: 1,
                got: 2
            })
        );
        assert_eq!(
            chain.append(&append(0, None, LOG_ENTRY_1)),
            Err(ChainError::Gap {
                expected: 1,
                got: 0
            })
        );
        // The first entry has no prev.
        assert_eq!(
            chain.append(&append(1, Some(hash(VECTOR_LOG_2)), LOG_ENTRY_1)),
            Err(ChainError::Prev)
        );
        let first = chain.append(&append(1, None, LOG_ENTRY_1)).unwrap();
        assert_eq!(first, hash(VECTOR_LOG_1));
        // Two devices race for index 2: the first wins, the second is told to fetch.
        let lost = append(2, Some(first), "AAAA");
        let second = chain.append(&append(2, Some(first), LOG_ENTRY_2)).unwrap();
        assert_eq!(second, hash(VECTOR_LOG_2));
        let error = chain.append(&lost).unwrap_err();
        assert_eq!(error, ChainError::Taken { index: 2 });
        assert_eq!(error.code(), ErrorCode::Conflict);
        // A wrong or missing prev at the right index.
        assert_eq!(
            chain.append(&append(3, Some(first), "AAAA")),
            Err(ChainError::Prev)
        );
        assert_eq!(
            chain.append(&append(3, None, "AAAA")),
            Err(ChainError::Prev)
        );
        assert_eq!(ChainError::Prev.code(), ErrorCode::BadRequest);
        // Non-canonical base64url (a set trailing bit) has no hash.
        assert_eq!(
            chain.append(&append(3, Some(second), "AB")),
            Err(ChainError::Entry)
        );
        assert_eq!(chain.head().map(|h| h.index), Some(2));
        let full = MemberChain::resume(Some(ChainHead {
            index: u64::MAX,
            hash: second,
        }));
        assert_eq!(
            check_append(full.head(), &append(1, Some(second), "AAAA")),
            Err(ChainError::Full)
        );
        assert_eq!(ChainError::Full.code(), ErrorCode::Limit);
    }

    #[test]
    fn clients_verify_what_the_relay_sends() {
        let mut chain = MemberChain::new();
        chain
            .verify(&entry(1, hash(VECTOR_LOG_1), LOG_ENTRY_1))
            .unwrap();
        // A relay that swaps the content but keeps the hash is caught.
        assert_eq!(
            chain.verify(&entry(2, hash(VECTOR_LOG_2), "AAAA")),
            Err(ChainError::Hash { index: 2 })
        );
        assert_eq!(
            chain.verify(&entry(3, hash(VECTOR_LOG_2), LOG_ENTRY_2)),
            Err(ChainError::Gap {
                expected: 2,
                got: 3
            })
        );
        chain
            .verify(&entry(2, hash(VECTOR_LOG_2), LOG_ENTRY_2))
            .unwrap();
        assert_eq!(
            chain.head(),
            Some(&ChainHead {
                index: 2,
                hash: hash(VECTOR_LOG_2)
            })
        );
        // Resuming from a trusted head works the same.
        let mut later = MemberChain::resume(Some(ChainHead {
            index: 1,
            hash: hash(VECTOR_LOG_1),
        }));
        later
            .verify(&entry(2, hash(VECTOR_LOG_2), LOG_ENTRY_2))
            .unwrap();
    }

    #[test]
    fn base64url_is_canonical() {
        for len in 0..10 {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 5) as u8).collect();
            let text = encode_base64url(&bytes);
            assert!(!text.contains('='));
            assert_eq!(decode_base64url(&text), Some(bytes), "{text}");
        }
        assert_eq!(decode_base64url("A"), None);
        assert_eq!(decode_base64url("AB"), None);
        assert_eq!(decode_base64url("AA=="), None);
        assert_eq!(decode_base64url("AA+/"), None);
        assert_eq!(decode_base64url("_-8"), Some(vec![0xff, 0xef]));
    }

    #[test]
    fn public_keys_are_32_bytes() {
        let key = PublicKey::from_bytes(core::array::from_fn(|i| i as u8));
        let text = key.to_base64url();
        assert_eq!(text, "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8");
        assert_eq!(PublicKey::try_from(text.clone()), Ok(key));
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(serde_json::from_str::<PublicKey>(&json).unwrap(), key);
        let ones = PublicKey::from_bytes([0xff; 32]);
        assert_eq!(ones.to_base64url(), format!("{}8", "_".repeat(42)));
        for bad in [
            String::new(),
            "A".repeat(42),
            "A".repeat(44),
            format!("{}=", "A".repeat(42)),
            format!("{}+", "A".repeat(42)),
            // 43 characters whose last one sets an unused bit.
            format!("{}_", "_".repeat(42)),
            // 32 bytes with padding.
            format!("{}=", "A".repeat(43)),
        ] {
            assert_eq!(
                PublicKey::try_from(bad.clone()),
                Err(InvalidPublicKey),
                "{bad}"
            );
        }
    }

    #[test]
    fn log_hash_text_has_one_spelling() {
        let value = "0f".repeat(32);
        assert_eq!(hash(&value).to_string(), value);
        assert!(LogHash::try_from("0F".repeat(32)).is_err());
        assert!(LogHash::try_from("0".repeat(63)).is_err());
    }

    fn rotate(epoch: u32, devices: &[&str]) -> KeyRotate {
        KeyRotate {
            epoch: Epoch(epoch),
            log: append(2, Some(hash(VECTOR_LOG_1)), "AAAA"),
            wrapped: devices
                .iter()
                .map(|d| WrappedKey {
                    holder: device("m1", d),
                    data: text("AAAA"),
                })
                .collect(),
        }
    }

    #[test]
    fn rotation_rules() {
        let current = Epoch(4);
        assert_eq!(
            check_rotate(Role::Owner, current, &rotate(5, &["d1", "d2"])),
            Ok(())
        );
        assert_eq!(
            check_rotate(Role::Admin, current, &rotate(5, &["d1"])),
            Ok(())
        );
        let error = check_rotate(Role::Member, current, &rotate(5, &["d1"])).unwrap_err();
        assert_eq!(error, KeyRotateError::Forbidden);
        assert_eq!(error.code(), ErrorCode::Forbidden);
        for epoch in [3, 4, 6] {
            let error = check_rotate(Role::Owner, current, &rotate(epoch, &["d1"])).unwrap_err();
            assert_eq!(
                error,
                KeyRotateError::Epoch {
                    expected: Some(Epoch(5)),
                    got: Epoch(epoch)
                }
            );
            assert_eq!(error.code(), ErrorCode::StaleEpoch);
        }
        assert_eq!(
            check_rotate(Role::Owner, Epoch(u32::MAX), &rotate(0, &["d1"])),
            Err(KeyRotateError::Epoch {
                expected: None,
                got: Epoch(0)
            })
        );
        assert_eq!(
            check_rotate(Role::Owner, current, &rotate(5, &[])),
            Err(KeyRotateError::Empty)
        );
        let error =
            check_rotate(Role::Owner, current, &rotate(5, &["d1", "d2", "d1"])).unwrap_err();
        assert_eq!(error, KeyRotateError::DuplicateDevice);
        assert_eq!(error.code(), ErrorCode::BadRequest);
        // The same device id under two members is two devices.
        let mut two_members = rotate(5, &["d1"]);
        two_members.wrapped.push(WrappedKey {
            holder: device("m2", "d1"),
            data: text("AAAA"),
        });
        assert_eq!(check_rotate(Role::Owner, current, &two_members), Ok(()));
    }

    fn device(member: &str, device: &str) -> Holder {
        Holder {
            member: MemberId::new(member).unwrap(),
            device: DeviceId::new(device).unwrap(),
        }
    }

    #[test]
    fn key_proofs_are_64_bytes() {
        let proof = KeyProof::from_bytes(core::array::from_fn(|i| i as u8));
        let text = proof.to_base64url();
        assert_eq!(text.len(), KEY_PROOF_CHARS);
        assert_eq!(KeyProof::try_from(text.clone()), Ok(proof));
        let json = serde_json::to_string(&proof).unwrap();
        assert_eq!(serde_json::from_str::<KeyProof>(&json).unwrap(), proof);
        for bad in [
            String::new(),
            "A".repeat(85),
            "A".repeat(87),
            format!("{}=", "A".repeat(85)),
            // 86 characters whose last one sets an unused bit.
            format!("{}_", "A".repeat(85)),
        ] {
            assert_eq!(
                KeyProof::try_from(bad.clone()),
                Err(InvalidKeyProof),
                "{bad}"
            );
        }
    }
}
