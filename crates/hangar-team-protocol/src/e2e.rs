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
//!
//! | Intent | Allowed for | Refused with |
//! | --- | --- | --- |
//! | `memberlog.append` | owner, admin ([`check_append_role`]) | `forbidden` |
//! | `key.rotate` | owner, admin ([`check_rotate`]) | `forbidden` |
//! | `key.wrap` | owner, admin, or another device of the target device's member ([`check_wrap`]) | `forbidden`; `conflict` when the device already has a key in the epoch |
//! | `device.remove` | owner, admin, or another device of the target device's member ([`check_device_remove`]) | `forbidden`; `notFound` when the relay does not know the device |

use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::hash::{hex32, length_prefix, parse_hex32};
use crate::ids::{Ciphertext, Epoch};
use crate::wire::{
    DeviceRemove, ErrorCode, Holder, KeyRotate, KeyWrap, MemberLogAppend, MemberLogEntry, Role,
};

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
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// The relay's check of a `key.rotate` by a member of `role` while the team is at `current`:
/// owner or admin only, the epoch exactly `current + 1`, at least one wrapped key and at most
/// one per holder (the same device id under two members is two devices). Size limits are
/// checked when the frame is decoded. Whether the wrapped keys cover every device of the team
/// is the rotating client's job (it reads the membership log).
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

/// A member without owner or admin role tried to append to the membership log.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("only an owner or an admin appends to the membership log")]
pub struct MemberLogForbidden;

impl MemberLogForbidden {
    /// The `err` code the relay answers with.
    pub fn code(&self) -> ErrorCode {
        ErrorCode::Forbidden
    }
}

/// The relay's rights check of a `memberlog.append`, before [`check_append`]: only an owner or
/// an admin (`role` from the ticket) appends, since in step E every entry is signed by an admin
/// device and an entry from anyone else could never verify.
pub fn check_append_role(role: Role) -> Result<(), MemberLogForbidden> {
    if is_owner_or_admin(role) {
        Ok(())
    } else {
        Err(MemberLogForbidden)
    }
}

/// A `device.remove` the sender may not make.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("this device may not remove that device")]
pub struct DeviceRemoveForbidden;

impl DeviceRemoveForbidden {
    /// The `err` code the relay answers with.
    pub fn code(&self) -> ErrorCode {
        ErrorCode::Forbidden
    }
}

/// The relay's rights check of a `device.remove`; `role` and `sender` are the connection's (as
/// for [`check_wrap`]), `remove.holder` the target. An owner or admin removes any other device,
/// a member only another device of its own (a lost laptop, from the phone). No device removes
/// itself: it simply disconnects, and removing it for good is the website's business. Rights
/// come before the relay's lookup of the target, so a member does not learn whether another
/// member's device exists (`notFound` only after this check passed).
pub fn check_device_remove(
    role: Role,
    sender: &Holder,
    remove: &DeviceRemove,
) -> Result<(), DeviceRemoveForbidden> {
    let target = &remove.holder;
    if sender == target || (!is_owner_or_admin(role) && sender.member != target.member) {
        return Err(DeviceRemoveForbidden);
    }
    Ok(())
}

/// Why a `key.wrap` is refused.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum KeyWrapError {
    /// The sending device wraps for itself (it cannot hold a key it is still waiting for), or
    /// the sender is neither owner nor admin and the target device belongs to another member.
    #[error("only an owner, an admin or another device of the same member wraps the team key for a device")]
    Forbidden,
    /// The device already has a wrapped key in this epoch. A wrapped key is never replaced;
    /// a new key for a device comes only with a new epoch (`key.rotate`).
    #[error("the device already has a wrapped team key in this epoch")]
    AlreadyWrapped,
}

impl KeyWrapError {
    /// The `err` code the relay answers with.
    pub fn code(&self) -> ErrorCode {
        match self {
            KeyWrapError::Forbidden => ErrorCode::Forbidden,
            KeyWrapError::AlreadyWrapped => ErrorCode::Conflict,
        }
    }
}

/// The relay's rights check of a `key.wrap`, after the epoch check (`check_epoch`):
///
/// - `role` and `sender` are the connection's: the ticket's role, and the holder the relay
///   bound to it (ticket subject and `hello.device`).
/// - `wrap` is the message; its `wrapped.holder` names the target device with its member.
/// - `already_wrapped` is whether the relay already stores a wrapped key for that holder in
///   the current epoch.
///
/// An owner or admin may wrap for any other device, a member only for another device of its
/// own (adding its own new device). No device wraps for itself: one without the key has
/// nothing to wrap, and a wrapped key it stored for itself would block the real one for the
/// whole epoch. Rights are checked first, so a member who may not wrap does not learn whether
/// the device already has a key.
pub fn check_wrap(
    role: Role,
    sender: &Holder,
    wrap: &KeyWrap,
    already_wrapped: bool,
) -> Result<(), KeyWrapError> {
    let target = &wrap.wrapped.holder;
    if sender == target || (!is_owner_or_admin(role) && sender.member != target.member) {
        return Err(KeyWrapError::Forbidden);
    }
    if already_wrapped {
        return Err(KeyWrapError::AlreadyWrapped);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{DeviceId, MemberId, UnixMillis};
    use crate::wire::WrappedKey;

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
    fn only_owners_and_admins_append_to_the_member_log() {
        assert_eq!(check_append_role(Role::Owner), Ok(()));
        assert_eq!(check_append_role(Role::Admin), Ok(()));
        let error = check_append_role(Role::Member).unwrap_err();
        assert_eq!(error, MemberLogForbidden);
        assert_eq!(error.code(), ErrorCode::Forbidden);
    }

    fn wrap(target: &Holder) -> KeyWrap {
        KeyWrap {
            epoch: Epoch(1),
            wrapped: WrappedKey {
                holder: target.clone(),
                data: text("AAAA"),
            },
        }
    }

    #[test]
    fn key_wrap_rights() {
        let alice_laptop = device("alice", "laptop");
        let alice_phone = device("alice", "phone");
        let bob_laptop = device("bob", "laptop");
        // Owners and admins wrap for any device.
        for role in [Role::Owner, Role::Admin] {
            assert_eq!(
                check_wrap(role, &alice_laptop, &wrap(&bob_laptop), false),
                Ok(())
            );
            assert_eq!(
                check_wrap(role, &alice_laptop, &wrap(&alice_phone), false),
                Ok(())
            );
        }
        // A member adds its own new device.
        assert_eq!(
            check_wrap(Role::Member, &alice_laptop, &wrap(&alice_phone), false),
            Ok(())
        );
        // But not another member's, even one with the same device id.
        let error = check_wrap(Role::Member, &alice_laptop, &wrap(&bob_laptop), false).unwrap_err();
        assert_eq!(error, KeyWrapError::Forbidden);
        assert_eq!(error.code(), ErrorCode::Forbidden);
        // No device wraps for itself, whatever its role.
        for role in [Role::Owner, Role::Admin, Role::Member] {
            assert_eq!(
                check_wrap(role, &alice_laptop, &wrap(&alice_laptop), false),
                Err(KeyWrapError::Forbidden)
            );
        }
    }

    #[test]
    fn key_wrap_never_replaces_a_wrapped_key() {
        let alice_laptop = device("alice", "laptop");
        let alice_phone = device("alice", "phone");
        for role in [Role::Owner, Role::Admin, Role::Member] {
            let error = check_wrap(role, &alice_laptop, &wrap(&alice_phone), true).unwrap_err();
            assert_eq!(error, KeyWrapError::AlreadyWrapped);
            assert_eq!(error.code(), ErrorCode::Conflict);
        }
        // Rights come first: a stranger does not learn whether the device has a key.
        assert_eq!(
            check_wrap(
                Role::Member,
                &device("bob", "pc"),
                &wrap(&alice_phone),
                true
            ),
            Err(KeyWrapError::Forbidden)
        );
    }

    #[test]
    fn device_remove_rights() {
        let alice_laptop = device("alice", "laptop");
        let alice_phone = device("alice", "phone");
        let bob_laptop = device("bob", "laptop");
        let remove = |holder: &Holder| DeviceRemove {
            holder: holder.clone(),
        };
        // Owners and admins remove any other device.
        for role in [Role::Owner, Role::Admin] {
            assert_eq!(
                check_device_remove(role, &alice_laptop, &remove(&bob_laptop)),
                Ok(())
            );
            assert_eq!(
                check_device_remove(role, &alice_laptop, &remove(&alice_phone)),
                Ok(())
            );
        }
        // A member removes another device of its own.
        assert_eq!(
            check_device_remove(Role::Member, &alice_phone, &remove(&alice_laptop)),
            Ok(())
        );
        // But not another member's, even one with the same device id.
        let error =
            check_device_remove(Role::Member, &alice_laptop, &remove(&bob_laptop)).unwrap_err();
        assert_eq!(error, DeviceRemoveForbidden);
        assert_eq!(error.code(), ErrorCode::Forbidden);
        // No device removes itself, whatever its role.
        for role in [Role::Owner, Role::Admin, Role::Member] {
            assert_eq!(
                check_device_remove(role, &alice_laptop, &remove(&alice_laptop)),
                Err(DeviceRemoveForbidden)
            );
        }
    }
}
