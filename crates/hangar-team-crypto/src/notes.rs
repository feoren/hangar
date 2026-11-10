//! Encrypted project notes (ADR 0006, `docs/wiki/pages/decisions/0006-encrypted-project-notes.md`):
//! the notes key ring, one sealed file per note, the notes recovery code and the code slot that
//! opens the key ring with it. Pure like the rest of the crate; the app reads and writes the files.
//!
//! # Note file (format 1)
//!
//! ```text
//! hangar-note 1 <key id, 16 lower case hex digits>\n
//! <base64url without padding: nonce (24 bytes) || XChaCha20-Poly1305 ciphertext and tag>\n
//!
//! file key = HKDF-SHA256(salt none, ikm = notes key,
//!                        info = field("hangar-notes/v1/file") || field(vault) || field(note id))
//! aad      = field("hangar-notes/v1/note") || field(0x01) || field(vault) || field(note id)
//!            || field(key id)
//! padded   = plaintext length (u32 big endian) || plaintext || zero bytes, to a multiple of 256
//! ```
//!
//! `vault` is the 16 random bytes of the vault id, `note id` the note's lower case UUID as text,
//! `key id` the 8 bytes of the key's id. Binding vault and note id means a file renamed to another
//! note, or copied into another repository, does not open; binding the key id means the header
//! cannot point at another key. Padding hides the length to 256 bytes; it is checked to be zero.
//!
//! # Notes recovery code
//!
//! The team recovery code's format ([`crate::recovery`]) with version byte `0xA8` (the code
//! starts with `N`) and the checksum label `hangar-notes/v1/recovery-code`, so a team code is
//! refused as a notes code and the other way round.
//!
//! # Code slot
//!
//! ```text
//! slot = base64url( 0x01 || nonce (24 bytes) || XChaCha20-Poly1305(key, nonce, key ring, aad) )
//! key  = HKDF-SHA256(salt none, ikm = code entropy,
//!                    info = field("hangar-notes/v1/code-slot") || field(vault))
//! aad  = field("hangar-notes/v1/code-slot") || field(0x01) || field(vault)
//! key ring = 0x01 || count (u8) || count x ( key id (8 bytes) || key (32 bytes) ), oldest first
//! ```
//!
//! 128 random bits need no password hashing: the code is drawn by the machine, not chosen.

use std::fmt;

use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use data_encoding::BASE64URL_NOPAD;
use hangar_team_protocol::hash::hkdf_sha256;
use thiserror::Error;
use zeroize::Zeroizing;

use crate::encoding::{constant_time_eq, framed, Writer};
use crate::entropy::{Entropy, EntropyError};
use crate::identity::array24;
use crate::recovery::{decode_code, encode_code, RecoveryError, ENTROPY_LEN};

const LABEL_FILE_KEY: &[u8] = b"hangar-notes/v1/file";
const LABEL_NOTE: &[u8] = b"hangar-notes/v1/note";
const LABEL_CODE_SLOT: &[u8] = b"hangar-notes/v1/code-slot";
const LABEL_RECOVERY_CODE: &[u8] = b"hangar-notes/v1/recovery-code";
/// Version byte of the notes recovery code: its first character is `N`.
const CODE_VERSION: u8 = 0xA8;
/// The note file format this crate writes and reads.
pub const NOTE_FORMAT: u8 = 1;
const SLOT_VERSION: u8 = 1;
const RING_VERSION: u8 = 1;
const FILE_MAGIC: &str = "hangar-note";
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const KEY_LEN: usize = 32;
const KEY_ID_LEN: usize = 8;
const PAD_BLOCK: usize = 256;
/// The largest plaintext a note carries: the app's 1 MiB of Markdown plus its metadata line.
pub const MAX_PLAINTEXT: usize = 1024 * 1024 + 64 * 1024;
/// The largest note file that can hold [`MAX_PLAINTEXT`]; anything longer is not a note.
pub const MAX_FILE_LEN: usize = 2 * 1024 * 1024;
/// Keys a ring holds at most. Solo mode never adds one; team mode adds one per removal.
pub const MAX_KEYS: usize = 64;

/// Why a note, a key ring or a code slot was not sealed or opened.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NotesCryptoError {
    /// Not a file, ring or slot of this format.
    #[error("the encrypted notes data is malformed")]
    Malformed,
    /// A newer format than this version knows (the number it names).
    #[error("notes format {0} is newer than this version of Hangar")]
    NewerFormat(u8),
    /// The file names a key the ring does not hold.
    #[error("the note was encrypted with a key this computer does not have")]
    UnknownKey(KeyId),
    /// The ciphertext does not open: a changed, truncated, renamed or transplanted file, or a
    /// wrong code for a code slot.
    #[error("the note does not decrypt")]
    Decrypt,
    /// The note id is not a lower case UUID.
    #[error("the note id is not a lower case UUID")]
    NoteId,
    /// The plaintext is larger than [`MAX_PLAINTEXT`], or the ring larger than [`MAX_KEYS`].
    #[error("the note is too large")]
    TooLarge,
    /// No randomness.
    #[error(transparent)]
    Entropy(#[from] EntropyError),
}

/// The random id of a vault (one `.hangar/notes` folder), 32 hex digits in `vault.json`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VaultId([u8; 16]);

impl VaultId {
    /// A new random vault id.
    pub fn generate(entropy: &mut impl Entropy) -> Result<Self, EntropyError> {
        let mut bytes = [0u8; 16];
        entropy.fill(&mut bytes)?;
        Ok(Self(bytes))
    }

    /// Reads 32 lower case hex digits.
    pub fn from_hex(text: &str) -> Option<Self> {
        hex_array(text).map(Self)
    }

    /// 32 lower case hex digits.
    pub fn to_hex(&self) -> String {
        hex(&self.0)
    }
}

impl fmt::Debug for VaultId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "VaultId({})", self.to_hex())
    }
}

/// The public id of one notes key, written in every note file's header.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct KeyId([u8; KEY_ID_LEN]);

impl KeyId {
    /// Reads 16 lower case hex digits.
    pub fn from_hex(text: &str) -> Option<Self> {
        hex_array(text).map(Self)
    }

    /// 16 lower case hex digits.
    pub fn to_hex(&self) -> String {
        hex(&self.0)
    }
}

impl fmt::Debug for KeyId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyId({})", self.to_hex())
    }
}

/// One random 256-bit notes key with its id. Wiped on drop, hidden from `Debug`.
#[derive(Clone)]
pub struct NotesKey {
    id: KeyId,
    key: Zeroizing<[u8; KEY_LEN]>,
}

impl NotesKey {
    /// A new random key: 8 bytes of id, then 32 bytes of key, from `entropy`.
    pub fn generate(entropy: &mut impl Entropy) -> Result<Self, EntropyError> {
        let mut id = [0u8; KEY_ID_LEN];
        entropy.fill(&mut id)?;
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        entropy.fill(key.as_mut())?;
        Ok(Self { id: KeyId(id), key })
    }

    /// The key's id.
    pub fn id(&self) -> KeyId {
        self.id
    }

    fn same_key(&self, other: &Self) -> bool {
        constant_time_eq(self.key.as_ref(), other.key.as_ref())
    }

    fn file_cipher(
        &self,
        vault: &VaultId,
        note_id: &str,
    ) -> Result<XChaCha20Poly1305, NotesCryptoError> {
        let hkdf = hkdf_sha256(self.key.as_ref());
        let mut file_key = Zeroizing::new([0u8; KEY_LEN]);
        hkdf.expand(
            &framed(&[LABEL_FILE_KEY, &vault.0, note_id.as_bytes()]),
            file_key.as_mut(),
        )
        .map_err(|_| NotesCryptoError::Malformed)?;
        XChaCha20Poly1305::new_from_slice(file_key.as_ref())
            .map_err(|_| NotesCryptoError::Malformed)
    }
}

impl fmt::Debug for NotesKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NotesKey({}, ..)", self.id.to_hex())
    }
}

/// Every notes key a project ever had, oldest first. Writes use the newest; reads use the key a
/// file names. Keys are only ever added, so an old note stays readable.
#[derive(Clone)]
pub struct KeyRing {
    keys: Vec<NotesKey>,
}

impl fmt::Debug for KeyRing {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ids: Vec<String> = self.keys.iter().map(|k| k.id.to_hex()).collect();
        f.debug_struct("KeyRing")
            .field("ids", &ids)
            .finish_non_exhaustive()
    }
}

impl KeyRing {
    /// A ring with one new random key.
    pub fn generate(entropy: &mut impl Entropy) -> Result<Self, EntropyError> {
        Ok(Self {
            keys: vec![NotesKey::generate(entropy)?],
        })
    }

    /// The key new writes use.
    pub fn newest(&self) -> &NotesKey {
        // A ring is never empty: every constructor puts at least one key in, and nothing removes.
        &self.keys[self.keys.len() - 1]
    }

    /// The key with this id.
    pub fn get(&self, id: KeyId) -> Option<&NotesKey> {
        self.keys.iter().find(|key| key.id == id)
    }

    /// The ids, oldest first.
    pub fn ids(&self) -> Vec<KeyId> {
        self.keys.iter().map(|key| key.id).collect()
    }

    /// Adds a new random key, which becomes the newest.
    pub fn add_generated(&mut self, entropy: &mut impl Entropy) -> Result<KeyId, NotesCryptoError> {
        if self.keys.len() >= MAX_KEYS {
            return Err(NotesCryptoError::TooLarge);
        }
        let key = NotesKey::generate(entropy)?;
        let id = key.id;
        self.keys.push(key);
        Ok(id)
    }

    /// Takes in every key of `other` this ring lacks (a device keeps the union of what it
    /// accepted). Keys keep their order; new ones go behind. `Ok(true)` when something was
    /// added. Two keys with one id but different bytes are refused: one of them is forged.
    pub fn merge(&mut self, other: &KeyRing) -> Result<bool, NotesCryptoError> {
        let mut added = false;
        for key in &other.keys {
            match self.get(key.id) {
                Some(known) if known.same_key(key) => {}
                Some(_) => return Err(NotesCryptoError::Malformed),
                None => {
                    if self.keys.len() >= MAX_KEYS {
                        return Err(NotesCryptoError::TooLarge);
                    }
                    self.keys.push(key.clone());
                    added = true;
                }
            }
        }
        Ok(added)
    }

    /// The keychain text: `v1 <key id>:<key hex> ...`, oldest first.
    pub fn to_keychain(&self) -> Zeroizing<String> {
        let mut out = Zeroizing::new(String::with_capacity(3 + self.keys.len() * 82));
        out.push_str("v1");
        for key in &self.keys {
            out.push(' ');
            out.push_str(&key.id.to_hex());
            out.push(':');
            for byte in key.key.iter() {
                push_hex(&mut out, *byte);
            }
        }
        out
    }

    /// Reads [`Self::to_keychain`]'s text.
    pub fn from_keychain(text: &str) -> Result<Self, NotesCryptoError> {
        let mut parts = text.split(' ');
        match parts.next().and_then(|v| v.strip_prefix('v')) {
            Some("1") => {}
            // A later Hangar wrote it: the caller must not overwrite what it cannot read.
            Some(v) if !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()) => {
                return Err(NotesCryptoError::NewerFormat(v.parse().unwrap_or(u8::MAX)));
            }
            _ => return Err(NotesCryptoError::Malformed),
        }
        let mut keys = Vec::new();
        for part in parts {
            let (id, key) = part.split_once(':').ok_or(NotesCryptoError::Malformed)?;
            let id = KeyId::from_hex(id).ok_or(NotesCryptoError::Malformed)?;
            let key = Zeroizing::new(hex_array::<KEY_LEN>(key).ok_or(NotesCryptoError::Malformed)?);
            keys.push(NotesKey { id, key });
        }
        Self::checked(keys)
    }

    /// The binary form inside a code slot.
    fn encode(&self) -> Zeroizing<Vec<u8>> {
        let mut out = Zeroizing::new(Vec::with_capacity(
            2 + self.keys.len() * (KEY_ID_LEN + KEY_LEN),
        ));
        out.push(RING_VERSION);
        // At most MAX_KEYS (64), checked by every constructor and by `add_generated`.
        out.push(u8::try_from(self.keys.len()).unwrap_or(u8::MAX));
        for key in &self.keys {
            out.extend_from_slice(&key.id.0);
            out.extend_from_slice(key.key.as_ref());
        }
        out
    }

    fn decode(bytes: &[u8]) -> Result<Self, NotesCryptoError> {
        let (&version, rest) = bytes.split_first().ok_or(NotesCryptoError::Malformed)?;
        if version != RING_VERSION {
            return Err(NotesCryptoError::NewerFormat(version));
        }
        let (&count, rest) = rest.split_first().ok_or(NotesCryptoError::Malformed)?;
        if rest.len() != usize::from(count) * (KEY_ID_LEN + KEY_LEN) {
            return Err(NotesCryptoError::Malformed);
        }
        let keys = rest
            .as_chunks::<{ KEY_ID_LEN + KEY_LEN }>()
            .0
            .iter()
            .map(|chunk| {
                let mut id = [0u8; KEY_ID_LEN];
                id.copy_from_slice(&chunk[..KEY_ID_LEN]);
                let mut key = Zeroizing::new([0u8; KEY_LEN]);
                key.copy_from_slice(&chunk[KEY_ID_LEN..]);
                NotesKey { id: KeyId(id), key }
            })
            .collect();
        Self::checked(keys)
    }

    fn checked(keys: Vec<NotesKey>) -> Result<Self, NotesCryptoError> {
        if keys.is_empty() {
            return Err(NotesCryptoError::Malformed);
        }
        if keys.len() > MAX_KEYS {
            return Err(NotesCryptoError::TooLarge);
        }
        for (index, key) in keys.iter().enumerate() {
            if keys[..index].iter().any(|earlier| earlier.id == key.id) {
                return Err(NotesCryptoError::Malformed);
            }
        }
        Ok(Self { keys })
    }
}

/// What a note file says about itself in plaintext.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteHeader {
    /// The key that encrypted it.
    pub key: KeyId,
}

/// A note id as the file name carries it: a canonical lower case UUID.
pub fn is_note_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(b),
        })
}

/// The two lines of a note file. A CR before a line feed is tolerated (an editor or a Git setup
/// that ignored `.gitattributes`); both lines are bound by the cipher anyway.
fn split_file(file: &[u8]) -> Result<(&str, &str), NotesCryptoError> {
    let text = std::str::from_utf8(file).map_err(|_| NotesCryptoError::Malformed)?;
    let text = text.strip_suffix('\n').ok_or(NotesCryptoError::Malformed)?;
    let text = text.strip_suffix('\r').unwrap_or(text);
    let (header, body) = text.split_once('\n').ok_or(NotesCryptoError::Malformed)?;
    let header = header.strip_suffix('\r').unwrap_or(header);
    if body.contains(['\n', '\r']) {
        return Err(NotesCryptoError::Malformed);
    }
    Ok((header, body))
}

/// Reads a note file's header: its format and key. A newer format is reported as such, so the
/// app can say "made with a newer Hangar" instead of "damaged".
pub fn note_header(file: &[u8]) -> Result<NoteHeader, NotesCryptoError> {
    if file.len() > MAX_FILE_LEN {
        return Err(NotesCryptoError::TooLarge);
    }
    let (header, _) = split_file(file)?;
    let mut words = header.split(' ');
    if words.next() != Some(FILE_MAGIC) {
        return Err(NotesCryptoError::Malformed);
    }
    let format: u8 = words
        .next()
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|v| v.parse().ok())
        .ok_or(NotesCryptoError::Malformed)?;
    if format != NOTE_FORMAT {
        return Err(if format > NOTE_FORMAT {
            NotesCryptoError::NewerFormat(format)
        } else {
            NotesCryptoError::Malformed
        });
    }
    let key = words
        .next()
        .and_then(KeyId::from_hex)
        .ok_or(NotesCryptoError::Malformed)?;
    if words.next().is_some() {
        return Err(NotesCryptoError::Malformed);
    }
    Ok(NoteHeader { key })
}

fn note_aad(vault: &VaultId, note_id: &str, key: KeyId) -> Vec<u8> {
    let mut aad = Writer::new();
    aad.field(LABEL_NOTE)
        .u8(NOTE_FORMAT)
        .field(&vault.0)
        .field(note_id.as_bytes())
        .field(&key.0);
    aad.into_bytes()
}

/// Length prefix, plaintext and zero bytes up to the next multiple of [`PAD_BLOCK`].
fn pad(plaintext: &[u8]) -> Result<Zeroizing<Vec<u8>>, NotesCryptoError> {
    if plaintext.len() > MAX_PLAINTEXT {
        return Err(NotesCryptoError::TooLarge);
    }
    let used = 4 + plaintext.len();
    let total = used.div_ceil(PAD_BLOCK) * PAD_BLOCK;
    let mut out = Zeroizing::new(Vec::with_capacity(total));
    let len = u32::try_from(plaintext.len()).map_err(|_| NotesCryptoError::TooLarge)?;
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(plaintext);
    out.resize(total, 0);
    Ok(out)
}

/// The plaintext inside [`pad`]'s output; padding that is not all zeros, or a length that does
/// not fit, is refused.
fn unpad(padded: &[u8]) -> Result<Zeroizing<Vec<u8>>, NotesCryptoError> {
    if padded.is_empty() || !padded.len().is_multiple_of(PAD_BLOCK) {
        return Err(NotesCryptoError::Malformed);
    }
    let (prefix, rest) = padded
        .split_first_chunk::<4>()
        .ok_or(NotesCryptoError::Malformed)?;
    let len =
        usize::try_from(u32::from_be_bytes(*prefix)).map_err(|_| NotesCryptoError::Malformed)?;
    if len > rest.len() || rest.len() - len >= PAD_BLOCK {
        return Err(NotesCryptoError::Malformed);
    }
    let (plain, padding) = rest.split_at(len);
    if padding.iter().any(|b| *b != 0) {
        return Err(NotesCryptoError::Malformed);
    }
    Ok(Zeroizing::new(plain.to_vec()))
}

/// Encrypts a note's plaintext with the ring's newest key into the text of `<note id>.note`.
pub fn seal_note(
    ring: &KeyRing,
    vault: &VaultId,
    note_id: &str,
    plaintext: &[u8],
    entropy: &mut impl Entropy,
) -> Result<String, NotesCryptoError> {
    if !is_note_id(note_id) {
        return Err(NotesCryptoError::NoteId);
    }
    let padded = pad(plaintext)?;
    let key = ring.newest();
    let mut nonce = [0u8; NONCE_LEN];
    entropy.fill(&mut nonce)?;
    let sealed = key
        .file_cipher(vault, note_id)?
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &padded,
                aad: &note_aad(vault, note_id, key.id),
            },
        )
        .map_err(|_| NotesCryptoError::Malformed)?;
    let mut data = Vec::with_capacity(NONCE_LEN + sealed.len());
    data.extend_from_slice(&nonce);
    data.extend_from_slice(&sealed);
    Ok(format!(
        "{FILE_MAGIC} {NOTE_FORMAT} {}\n{}\n",
        key.id.to_hex(),
        BASE64URL_NOPAD.encode(&data)
    ))
}

/// Decrypts a note file. Fails unless the file was sealed for exactly this vault and note id
/// with a key of the ring, and nothing in it changed.
pub fn open_note(
    ring: &KeyRing,
    vault: &VaultId,
    note_id: &str,
    file: &[u8],
) -> Result<Zeroizing<Vec<u8>>, NotesCryptoError> {
    if !is_note_id(note_id) {
        return Err(NotesCryptoError::NoteId);
    }
    let header = note_header(file)?;
    let (_, body) = split_file(file)?;
    let key = ring
        .get(header.key)
        .ok_or(NotesCryptoError::UnknownKey(header.key))?;
    let data = BASE64URL_NOPAD
        .decode(body.as_bytes())
        .map_err(|_| NotesCryptoError::Malformed)?;
    if data.len() < NONCE_LEN + TAG_LEN {
        return Err(NotesCryptoError::Malformed);
    }
    let (nonce, sealed) = data.split_at(NONCE_LEN);
    let padded = Zeroizing::new(
        key.file_cipher(vault, note_id)?
            .decrypt(
                &XNonce::from(array24(nonce)),
                Payload {
                    msg: sealed,
                    aad: &note_aad(vault, note_id, key.id),
                },
            )
            .map_err(|_| NotesCryptoError::Decrypt)?,
    );
    unpad(&padded)
}

/// The notes recovery code (solo mode): 128 random bits that open the vault's code slot. Wiped
/// on drop; `Debug` hides it and it has no `Display`: [`NotesRecoveryCode::reveal`] gives the
/// text to show once.
#[derive(Clone)]
pub struct NotesRecoveryCode(Zeroizing<[u8; ENTROPY_LEN]>);

impl PartialEq for NotesRecoveryCode {
    fn eq(&self, other: &Self) -> bool {
        constant_time_eq(self.0.as_ref(), other.0.as_ref())
    }
}

impl Eq for NotesRecoveryCode {}

impl fmt::Debug for NotesRecoveryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NotesRecoveryCode(..)")
    }
}

impl NotesRecoveryCode {
    /// A new random code.
    pub fn generate(entropy: &mut impl Entropy) -> Result<Self, EntropyError> {
        let mut bytes = Zeroizing::new([0u8; ENTROPY_LEN]);
        entropy.fill(bytes.as_mut())?;
        Ok(Self(bytes))
    }

    /// The code as text: 8 groups of 4 characters, starting with `N`.
    pub fn reveal(&self) -> Zeroizing<String> {
        encode_code(LABEL_RECOVERY_CODE, CODE_VERSION, &self.0)
    }

    /// Reads a typed code. A team recovery code fails with `RecoveryError::Version(1)`.
    pub fn parse(text: &str) -> Result<Self, RecoveryError> {
        decode_code(text, LABEL_RECOVERY_CODE, CODE_VERSION).map(Self)
    }

    fn slot_cipher(&self, vault: &VaultId) -> Result<XChaCha20Poly1305, NotesCryptoError> {
        let hkdf = hkdf_sha256(self.0.as_ref());
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        hkdf.expand(&framed(&[LABEL_CODE_SLOT, &vault.0]), key.as_mut())
            .map_err(|_| NotesCryptoError::Malformed)?;
        XChaCha20Poly1305::new_from_slice(key.as_ref()).map_err(|_| NotesCryptoError::Malformed)
    }
}

fn slot_aad(vault: &VaultId) -> Vec<u8> {
    let mut aad = Writer::new();
    aad.field(LABEL_CODE_SLOT).u8(SLOT_VERSION).field(&vault.0);
    aad.into_bytes()
}

/// Seals the key ring under the recovery code for `vault.json`'s `codeSlot`.
pub fn seal_code_slot(
    ring: &KeyRing,
    vault: &VaultId,
    code: &NotesRecoveryCode,
    entropy: &mut impl Entropy,
) -> Result<String, NotesCryptoError> {
    let mut nonce = [0u8; NONCE_LEN];
    entropy.fill(&mut nonce)?;
    let plain = ring.encode();
    let sealed = code
        .slot_cipher(vault)?
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: &plain,
                aad: &slot_aad(vault),
            },
        )
        .map_err(|_| NotesCryptoError::Malformed)?;
    let mut data = Vec::with_capacity(1 + NONCE_LEN + sealed.len());
    data.push(SLOT_VERSION);
    data.extend_from_slice(&nonce);
    data.extend_from_slice(&sealed);
    Ok(BASE64URL_NOPAD.encode(&data))
}

/// Opens a code slot. A wrong code (or a slot of another vault) fails as
/// [`NotesCryptoError::Decrypt`].
pub fn open_code_slot(
    slot: &str,
    vault: &VaultId,
    code: &NotesRecoveryCode,
) -> Result<KeyRing, NotesCryptoError> {
    let data = BASE64URL_NOPAD
        .decode(slot.as_bytes())
        .map_err(|_| NotesCryptoError::Malformed)?;
    let (&version, rest) = data.split_first().ok_or(NotesCryptoError::Malformed)?;
    if version != SLOT_VERSION {
        return Err(NotesCryptoError::NewerFormat(version));
    }
    if rest.len() < NONCE_LEN + TAG_LEN {
        return Err(NotesCryptoError::Malformed);
    }
    let (nonce, sealed) = rest.split_at(NONCE_LEN);
    let plain = Zeroizing::new(
        code.slot_cipher(vault)?
            .decrypt(
                &XNonce::from(array24(nonce)),
                Payload {
                    msg: sealed,
                    aad: &slot_aad(vault),
                },
            )
            .map_err(|_| NotesCryptoError::Decrypt)?,
    );
    KeyRing::decode(&plain)
}

fn push_hex(out: &mut String, byte: u8) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    out.push(char::from(DIGITS[usize::from(byte >> 4)]));
    out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        push_hex(&mut out, *byte);
    }
    out
}

/// Exactly `2 * N` lower case hex digits.
fn hex_array<const N: usize>(text: &str) -> Option<[u8; N]> {
    let bytes = text.as_bytes();
    if bytes.len() != N * 2 {
        return None;
    }
    let digit = |b: u8| match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        _ => None,
    };
    let mut out = [0u8; N];
    for (index, pair) in bytes.as_chunks::<2>().0.iter().enumerate() {
        out[index] = (digit(pair[0])? << 4) | digit(pair[1])?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entropy::{FixedEntropy, OsEntropy};

    const NOTE: &str = "0f8e2c1a-5b6d-4e7f-8a9b-0c1d2e3f4a5b";
    const OTHER_NOTE: &str = "1f8e2c1a-5b6d-4e7f-8a9b-0c1d2e3f4a5b";

    // Test vectors: `test_vectors.py` recomputes every `VECTOR_*` constant of this file too.
    const VECTOR_NOTE_ID: &str = "00112233-4455-4677-8899-aabbccddeeff";
    const VECTOR_NOTE_PLAINTEXT: &str = "{\"v\":1,\"title\":\"Plan\",\"created\":1762770000000,\"updated\":1762770000000,\"by\":\"\"}\n# Plan\n";
    const VECTOR_NOTE_FILE: &str = "hangar-note 1 3031323334353637\ncHFyc3R1dnd4eXp7fH1-f4CBgoOEhYaHVVcb1iMFYbDuyqNY_m42F1mcGjtbU45gmts72kIGRcJwbk221znv_Bay0h-rMdX0HPx64EOLPjvpJkd0h_ZnZnBkWd0QEa4VOFrLY8wdZiaJztTLEtVtfKnDr5lW4gZWFsB8gbsPN9TOAK5IYWAT62XPbOeKFRMUgSUAYI6sHWvO4COdocc-6sOs4OL1Vl7hX4323ZWc6rQ-L1RUQPFZkENAB3etrmFTbwjjCeLaseslc25KX4w0D-NXRkFT5qrJt4CmtBKPeJWQ8jLMI-BRse54AQUj7oOZM0oAAENKCdYwsf4WO006d2DeUsIKRZBKxKTB5JGC5buzuF195FkyXeW9OxnR9h4S74CyNgUIsJI\n";
    const VECTOR_NOTES_CODE: &str = "N289-34MK-JJAS-D5WR-K6D9-Q74X-KTFN-7EZ3";
    const VECTOR_NOTES_CODE_SLOT: &str = "AbCxsrO0tba3uLm6u7y9vr_AwcLDxMXGx0bpvJbLSUbivz_MPfpNIhFcAkRjuIMog6Olf1RwlQYplwUqSscF-caFaoqSBGNShGUcti_GHvxSPFc";
    const VECTOR_NOTES_KEYCHAIN: &str =
        "v1 3031323334353637:38393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f5051525354555657";

    fn vault() -> VaultId {
        VaultId::generate(&mut OsEntropy).expect("rng")
    }

    fn ring() -> KeyRing {
        KeyRing::generate(&mut OsEntropy).expect("rng")
    }

    #[test]
    fn note_round_trips_and_is_padded() {
        let (ring, vault) = (ring(), vault());
        for plain in [&b""[..], b"x", &[7u8; 251], &[7u8; 252], &[7u8; 253]] {
            let file = seal_note(&ring, &vault, NOTE, plain, &mut OsEntropy).expect("seal");
            assert!(file.starts_with(&format!("hangar-note 1 {}\n", ring.newest().id().to_hex())));
            assert!(file.ends_with('\n'));
            assert_eq!(file.lines().count(), 2);
            let opened = open_note(&ring, &vault, NOTE, file.as_bytes()).expect("open");
            assert_eq!(opened.as_slice(), plain);
            let body = file.lines().nth(1).expect("body");
            let bytes = BASE64URL_NOPAD.decode(body.as_bytes()).expect("b64").len();
            assert_eq!(
                (bytes - NONCE_LEN - TAG_LEN) % PAD_BLOCK,
                0,
                "{}",
                plain.len()
            );
        }
        // 252 bytes fit one block with the length prefix, 253 need two.
        let one = seal_note(&ring, &vault, NOTE, &[1u8; 252], &mut OsEntropy).expect("seal");
        let two = seal_note(&ring, &vault, NOTE, &[1u8; 253], &mut OsEntropy).expect("seal");
        assert!(two.len() > one.len());
        // Every write draws a fresh nonce.
        let again = seal_note(&ring, &vault, NOTE, &[1u8; 252], &mut OsEntropy).expect("seal");
        assert_ne!(one, again);
    }

    #[test]
    fn tampered_renamed_and_transplanted_files_do_not_open() {
        let (ring, vault) = (ring(), vault());
        let file = seal_note(&ring, &vault, NOTE, b"secret plan", &mut OsEntropy).expect("seal");
        // Another note id (a renamed file) or another vault (a copied file).
        assert_eq!(
            open_note(&ring, &vault, OTHER_NOTE, file.as_bytes()).unwrap_err(),
            NotesCryptoError::Decrypt
        );
        assert_eq!(
            open_note(&ring, &self::vault(), NOTE, file.as_bytes()).unwrap_err(),
            NotesCryptoError::Decrypt
        );
        // A flipped ciphertext character.
        let mut chars: Vec<char> = file.chars().collect();
        let at = file.find('\n').expect("lf") + 10;
        chars[at] = if chars[at] == 'A' { 'B' } else { 'A' };
        let flipped: String = chars.into_iter().collect();
        assert_eq!(
            open_note(&ring, &vault, NOTE, flipped.as_bytes()).unwrap_err(),
            NotesCryptoError::Decrypt
        );
        // Truncated.
        let cut = format!("{}\n", &file[..file.len() - 20]);
        assert!(open_note(&ring, &vault, NOTE, cut.as_bytes()).is_err());
        assert_eq!(
            open_note(&ring, &vault, NOTE, &file.as_bytes()[..file.len() - 1]).unwrap_err(),
            NotesCryptoError::Malformed
        );
        // The header pointed at another key of the same ring.
        let mut two = ring.clone();
        let second = two.add_generated(&mut OsEntropy).expect("add");
        let first = ring.newest().id().to_hex();
        let pointed = file.replacen(&first, &second.to_hex(), 1);
        assert_eq!(
            open_note(&two, &vault, NOTE, pointed.as_bytes()).unwrap_err(),
            NotesCryptoError::Decrypt
        );
        // CRLF line ends still open (bound by the cipher anyway).
        let crlf = file.replace('\n', "\r\n");
        assert_eq!(
            open_note(&ring, &vault, NOTE, crlf.as_bytes())
                .expect("crlf")
                .as_slice(),
            b"secret plan"
        );
    }

    #[test]
    fn wrong_key_unknown_key_and_newer_format() {
        let vault = vault();
        let file = seal_note(&ring(), &vault, NOTE, b"x", &mut OsEntropy).expect("seal");
        let other = ring();
        assert!(matches!(
            open_note(&other, &vault, NOTE, file.as_bytes()),
            Err(NotesCryptoError::UnknownKey(_))
        ));
        let newer = file.replacen("hangar-note 1 ", "hangar-note 2 ", 1);
        assert_eq!(
            note_header(newer.as_bytes()).unwrap_err(),
            NotesCryptoError::NewerFormat(2)
        );
        for bad in [
            "hangar-note 0 0011223344556677\nAAAA\n",
            "hangar-note 1 00112233445566\nAAAA\n",
            "hangar-note 1 0011223344556677 x\nAAAA\n",
            "hangar-note 1 0011223344556677\nAAAA",
            "hangar-note 1 0011223344556677\nAA\nAA\n",
            "other 1 0011223344556677\nAAAA\n",
            "hangar-note 1 0011223344556677\n",
        ] {
            assert_eq!(
                note_header(bad.as_bytes()).unwrap_err(),
                NotesCryptoError::Malformed,
                "{bad:?}"
            );
        }
        assert_eq!(
            seal_note(
                &other,
                &vault,
                "0F8E2C1A-5B6D-4E7F-8A9B-0C1D2E3F4A5B",
                b"x",
                &mut OsEntropy
            )
            .unwrap_err(),
            NotesCryptoError::NoteId
        );
        assert_eq!(
            seal_note(
                &other,
                &vault,
                NOTE,
                &vec![0u8; MAX_PLAINTEXT + 1],
                &mut OsEntropy
            )
            .unwrap_err(),
            NotesCryptoError::TooLarge
        );
    }

    #[test]
    fn padding_is_checked() {
        let padded = pad(b"abc").expect("pad");
        assert_eq!(padded.len(), PAD_BLOCK);
        assert_eq!(unpad(&padded).expect("unpad").as_slice(), b"abc");
        let mut dirty = padded.to_vec();
        dirty[PAD_BLOCK - 1] = 1;
        assert_eq!(unpad(&dirty).unwrap_err(), NotesCryptoError::Malformed);
        let mut long = padded.to_vec();
        long[..4].copy_from_slice(&300u32.to_be_bytes());
        assert_eq!(unpad(&long).unwrap_err(), NotesCryptoError::Malformed);
        // A whole block of padding more than needed is not canonical.
        let mut extra = padded.to_vec();
        extra.resize(2 * PAD_BLOCK, 0);
        assert_eq!(unpad(&extra).unwrap_err(), NotesCryptoError::Malformed);
        assert_eq!(
            unpad(&padded[..PAD_BLOCK - 1]).unwrap_err(),
            NotesCryptoError::Malformed
        );
    }

    #[test]
    fn key_ring_keychain_text_and_merge() {
        let mut ring = ring();
        ring.add_generated(&mut OsEntropy).expect("add");
        let text = ring.to_keychain();
        assert!(text.starts_with("v1 "));
        assert_eq!(text.split(' ').count(), 3);
        let back = KeyRing::from_keychain(&text).expect("parse");
        assert_eq!(back.ids(), ring.ids());
        assert_eq!(back.newest().id(), ring.newest().id());
        // Everything below is refused.
        let one = text.split(' ').nth(1).expect("key").to_owned();
        for bad in [
            "v1".to_owned(),
            "x2 ".to_owned() + &one,
            format!("v1 {one} {one}"),
            format!("v1 {}", one.to_uppercase()),
            format!("v1 {}", &one[..one.len() - 2]),
        ] {
            assert!(KeyRing::from_keychain(&bad).is_err(), "{bad}");
        }
        assert_eq!(
            KeyRing::from_keychain(&format!("v2 {one}")).unwrap_err(),
            NotesCryptoError::NewerFormat(2)
        );
        // Merge keeps the union, never takes a key away and refuses a forged twin.
        let mut small = KeyRing::from_keychain(&format!("v1 {one}")).expect("one");
        assert!(small.merge(&ring).expect("merge"));
        assert_eq!(small.ids(), ring.ids());
        assert!(!small.merge(&ring).expect("again"));
        let forged = format!("v1 {}:{}", &one[..16], "00".repeat(32));
        let forged = KeyRing::from_keychain(&forged).expect("forged");
        assert_eq!(
            small.merge(&forged).unwrap_err(),
            NotesCryptoError::Malformed
        );
        assert!(!format!("{ring:?}").contains(&one[17..]));
    }

    #[test]
    fn code_slot_opens_with_the_code_only() {
        let (ring, vault) = (ring(), vault());
        let code = NotesRecoveryCode::generate(&mut OsEntropy).expect("rng");
        let slot = seal_code_slot(&ring, &vault, &code, &mut OsEntropy).expect("seal");
        let opened = open_code_slot(&slot, &vault, &code).expect("open");
        assert_eq!(opened.ids(), ring.ids());
        let wrong = NotesRecoveryCode::generate(&mut OsEntropy).expect("rng");
        assert_eq!(
            open_code_slot(&slot, &vault, &wrong).unwrap_err(),
            NotesCryptoError::Decrypt
        );
        assert_eq!(
            open_code_slot(&slot, &self::vault(), &code).unwrap_err(),
            NotesCryptoError::Decrypt
        );
        assert_eq!(
            open_code_slot("!!", &vault, &code).unwrap_err(),
            NotesCryptoError::Malformed
        );
        // The code reads back, and a team recovery code is not a notes code.
        let text = code.reveal();
        assert!(text.starts_with('N'), "{}", *text);
        assert_eq!(
            NotesRecoveryCode::parse(&text.to_lowercase()).expect("parse"),
            code
        );
        let team = crate::recovery::RecoveryCode::generate(&mut OsEntropy)
            .expect("rng")
            .reveal();
        assert_eq!(
            NotesRecoveryCode::parse(&team),
            Err(RecoveryError::Version(1))
        );
        assert!(crate::recovery::RecoveryCode::parse(&text).is_err());
        assert_eq!(format!("{code:?}"), "NotesRecoveryCode(..)");
    }

    #[test]
    fn ids_read_back() {
        let vault = vault();
        assert_eq!(VaultId::from_hex(&vault.to_hex()), Some(vault));
        assert_eq!(VaultId::from_hex(&vault.to_hex().to_uppercase()), None);
        assert_eq!(VaultId::from_hex("00"), None);
        assert!(is_note_id(NOTE));
        assert!(!is_note_id("0f8e2c1a5b6d4e7f8a9b0c1d2e3f4a5b0000"));
        assert!(!is_note_id("../../../../../../../../../../etc/x"));
    }

    struct Vectors {
        file: String,
        code: String,
        slot: String,
        keychain: String,
    }

    fn compute_vectors() -> Vectors {
        let vault = VaultId::generate(&mut FixedEntropy::counting(0x20, 16)).expect("rng");
        let ring = KeyRing::generate(&mut FixedEntropy::counting(0x30, 40)).expect("rng");
        let file = seal_note(
            &ring,
            &vault,
            VECTOR_NOTE_ID,
            VECTOR_NOTE_PLAINTEXT.as_bytes(),
            &mut FixedEntropy::counting(0x70, 24),
        )
        .expect("seal");
        let code = NotesRecoveryCode::generate(&mut FixedEntropy::counting(0x90, 16)).expect("rng");
        let slot = seal_code_slot(&ring, &vault, &code, &mut FixedEntropy::counting(0xb0, 24))
            .expect("slot");
        Vectors {
            file,
            code: code.reveal().to_string(),
            slot,
            keychain: ring.to_keychain().to_string(),
        }
    }

    #[test]
    fn test_vectors() {
        let v = compute_vectors();
        let pairs = [
            (
                "VECTOR_NOTE_FILE",
                VECTOR_NOTE_FILE,
                v.file.replace('\n', "\\n"),
            ),
            ("VECTOR_NOTES_CODE", VECTOR_NOTES_CODE, v.code),
            ("VECTOR_NOTES_CODE_SLOT", VECTOR_NOTES_CODE_SLOT, v.slot),
            ("VECTOR_NOTES_KEYCHAIN", VECTOR_NOTES_KEYCHAIN, v.keychain),
        ];
        let mut wrong = Vec::new();
        for (name, expected, got) in pairs {
            if expected.replace('\n', "\\n") != got {
                wrong.push(format!("const {name}: &str = \"{got}\";"));
            }
        }
        assert!(wrong.is_empty(), "vectors differ:\n{}", wrong.join("\n"));
    }

    #[test]
    fn test_vectors_open() {
        let vault = VaultId::generate(&mut FixedEntropy::counting(0x20, 16)).expect("rng");
        let code = NotesRecoveryCode::parse(VECTOR_NOTES_CODE).expect("code");
        let ring = open_code_slot(VECTOR_NOTES_CODE_SLOT, &vault, &code).expect("slot");
        let plain =
            open_note(&ring, &vault, VECTOR_NOTE_ID, VECTOR_NOTE_FILE.as_bytes()).expect("open");
        assert_eq!(plain.as_slice(), VECTOR_NOTE_PLAINTEXT.as_bytes());
        assert_eq!(*ring.to_keychain(), VECTOR_NOTES_KEYCHAIN);
    }
}
