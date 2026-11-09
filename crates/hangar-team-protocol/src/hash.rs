//! Keyed hashes for content-free matching. Clients send HMAC-SHA256 values of task ids and paths
//! under a key only the team knows; the server compares them for equality and learns neither
//! names nor paths.
//!
//! Keys: each epoch's secret (step E holds it) is expanded with HKDF-SHA256 into separate keys
//! with distinct labels: the matching key for these hashes and the sealing key for content. The
//! epoch secret itself never keys a value the server sees, and a weakness in one use never
//! carries over to the other.
//!
//! What the server still learns per changed file: whether it lies in a folder at all, which
//! changed files share their first folder, how many files a session changes, and that a value
//! repeats over time within one epoch. Deeper folder levels are not sent (data minimization; a
//! finer area needs a protocol version that adds them). A member who leaves keeps the keys of
//! the epochs they saw; a new epoch gives new values.
//!
//! Hash input: a domain label and each field, every one prefixed by its length as a 32-bit big
//! endian number, so no two different inputs can produce the same byte string.

use std::fmt;

use hkdf::Hkdf;
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;
use zeroize::Zeroizing;

use crate::ids::{Epoch, TeamId};
use crate::limits::{MAX_PATH_BYTES, MAX_SEGMENT_BYTES, MAX_TASK_ID_BYTES};
use crate::text::is_hidden;

type HmacSha256 = Hmac<Sha256>;

const INFO_MATCH_KEY: &[u8] = b"hangar-team/v1/match-key";
const INFO_SEAL_KEY: &[u8] = b"hangar-team/v1/seal-key";
const LABEL_TASK: &[u8] = b"hangar-team/v1/task";
const LABEL_REPO: &[u8] = b"hangar-team/v1/repo";
const LABEL_AREA: &[u8] = b"hangar-team/v1/area";
const LABEL_FILE: &[u8] = b"hangar-team/v1/file";

/// Why a value cannot be hashed.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NormalizeError {
    #[error("the value is empty")]
    Empty,
    #[error("the value is longer than {max} bytes")]
    TooLong { max: usize },
    #[error("the value contains a control, format or separator character")]
    Hidden,
    #[error("the path is absolute")]
    Absolute,
    #[error("the path leaves its repository through `..`")]
    ParentSegment,
    #[error("a path segment is longer than {max} bytes")]
    SegmentTooLong { max: usize },
    #[error("a repository id is the root commit as 40 or 64 hex digits")]
    RepoId,
}

/// A key could not be set up (HMAC takes keys of any length and HKDF expands 32 bytes, so this
/// does not happen with the crate versions in use; kept as an error instead of a panic).
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("the team keys could not be set up")]
pub struct KeyError;

/// A keyed hash: 32 bytes, 64 lowercase hex digits on the wire.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct KeyedHash([u8; 32]);

impl KeyedHash {
    /// Wraps 32 bytes (tests and storage).
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The raw 32 bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The 64 lowercase hex digits the wire carries.
    pub fn to_hex(&self) -> String {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let mut out = String::with_capacity(64);
        for byte in self.0 {
            out.push(char::from(DIGITS[usize::from(byte >> 4)]));
            out.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
        }
        out
    }
}

/// Only lowercase hex, so one value has exactly one spelling and the server can compare text.
fn hex_value(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        _ => None,
    }
}

/// Not 64 lowercase hex digits.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("a keyed hash is 64 lowercase hex digits")]
pub struct InvalidHash;

impl TryFrom<String> for KeyedHash {
    type Error = InvalidHash;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let digits = value.as_bytes();
        if digits.len() != 64 {
            return Err(InvalidHash);
        }
        let mut bytes = [0u8; 32];
        let (pairs, _) = digits.as_chunks::<2>();
        for (byte, pair) in bytes.iter_mut().zip(pairs) {
            let high = hex_value(pair[0]).ok_or(InvalidHash)?;
            let low = hex_value(pair[1]).ok_or(InvalidHash)?;
            *byte = (high << 4) | low;
        }
        Ok(Self(bytes))
    }
}

impl From<KeyedHash> for String {
    fn from(value: KeyedHash) -> String {
        value.to_hex()
    }
}

impl fmt::Debug for KeyedHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "KeyedHash({})", self.to_hex())
    }
}

impl fmt::Display for KeyedHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// How paths of a repository compare. Every member of a team must use the same rule for a
/// repository, whatever their platform, or the same file gets different hashes; the rule
/// therefore belongs to the team's repository settings, not to the local file system. The rule
/// is part of the hash input, so two members with different settings never match by accident
/// (a folded `src/a` and a case-sensitive `src/a` are different claims).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum CaseRule {
    /// Paths that differ only in case are one file. The default: teams mix Windows, macOS and
    /// Linux, and a false "same file" only shows a warning, while a missed one hides a conflict.
    #[default]
    Fold,
    /// Paths compare byte for byte (for repositories that hold names differing only in case).
    Sensitive,
}

impl CaseRule {
    fn label(self) -> &'static [u8] {
        match self {
            CaseRule::Fold => b"fold",
            CaseRule::Sensitive => b"sensitive",
        }
    }
}

/// Unicode normalization form C: macOS file systems may hand out decomposed names (NFD) where
/// Windows and Linux keep what was typed (usually NFC), so the same name is unified first.
fn nfc(value: &str) -> String {
    value.nfc().collect()
}

/// A task id ready for hashing: surrounding whitespace removed, Unicode NFC, ASCII letters in
/// lower case (forge owners and repositories are case-insensitive), no control, format or
/// separator characters. `Debug` does not show it: task ids can name customers or projects.
#[derive(Clone, PartialEq, Eq)]
pub struct TaskId(String);

impl fmt::Debug for TaskId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TaskId(..)")
    }
}

impl TaskId {
    /// Checks and normalizes a task id as the app or a forge names it.
    pub fn normalize(raw: &str) -> Result<Self, NormalizeError> {
        let trimmed = raw.trim();
        if trimmed.chars().any(is_hidden) {
            return Err(NormalizeError::Hidden);
        }
        let id = nfc(trimmed).to_ascii_lowercase();
        if id.is_empty() {
            return Err(NormalizeError::Empty);
        }
        if id.len() > MAX_TASK_ID_BYTES {
            return Err(NormalizeError::TooLong {
                max: MAX_TASK_ID_BYTES,
            });
        }
        Ok(Self(id))
    }

    /// The normalized id.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A repository's identity: its root commit (the oldest commit without parents; with several,
/// the smallest id), the same for every clone and every forge, in lower case hex. SHA-1 (40
/// digits) and SHA-256 (64 digits) repositories are both accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoId(String);

impl RepoId {
    /// Checks a root commit id and puts it in lower case.
    pub fn new(root_commit: &str) -> Result<Self, NormalizeError> {
        let id = root_commit.trim().to_ascii_lowercase();
        let hex = id.bytes().all(|b| hex_value(b).is_some());
        if !hex || (id.len() != 40 && id.len() != 64) {
            return Err(NormalizeError::RepoId);
        }
        Ok(Self(id))
    }

    /// The id in lower case hex.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A path relative to the repository's top folder, normalized: Unicode NFC, `/` separators (a
/// `\` counts as one), no empty or `.` segments, no `..`, not absolute, case folded per
/// [`CaseRule`], no control, format or separator characters.
///
/// Callers take paths from git's own output (`git status -z`, `git diff --name-only -z`), which
/// is always relative to the top folder with `/`; raw input from an agent's hook is resolved
/// against the work tree first. The checks here are a second line, not a path resolver.
/// `Debug` does not show the path.
#[derive(Clone, PartialEq, Eq)]
pub struct RepoPath {
    segments: Vec<String>,
    rule: CaseRule,
}

impl fmt::Debug for RepoPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RepoPath(..)")
    }
}

impl RepoPath {
    /// Checks and normalizes a path relative to the repository's top folder.
    pub fn normalize(raw: &str, rule: CaseRule) -> Result<Self, NormalizeError> {
        if raw.chars().any(is_hidden) {
            return Err(NormalizeError::Hidden);
        }
        let unified = nfc(raw).replace('\\', "/");
        if unified.starts_with('/') || has_drive(&unified) {
            return Err(NormalizeError::Absolute);
        }
        let mut segments = Vec::new();
        for segment in unified.split('/') {
            match segment {
                "" | "." => {}
                ".." => return Err(NormalizeError::ParentSegment),
                _ => {
                    let segment = match rule {
                        // Lower case can leave a name outside NFC; normalize once more.
                        CaseRule::Fold => nfc(&segment.to_lowercase()),
                        CaseRule::Sensitive => segment.to_owned(),
                    };
                    if segment.len() > MAX_SEGMENT_BYTES {
                        return Err(NormalizeError::SegmentTooLong {
                            max: MAX_SEGMENT_BYTES,
                        });
                    }
                    segments.push(segment);
                }
            }
        }
        if segments.is_empty() {
            return Err(NormalizeError::Empty);
        }
        let path = Self { segments, rule };
        if path.as_string().len() > MAX_PATH_BYTES {
            return Err(NormalizeError::TooLong {
                max: MAX_PATH_BYTES,
            });
        }
        Ok(path)
    }

    /// The normalized path, segments joined by `/`.
    pub fn as_string(&self) -> String {
        self.segments.join("/")
    }

    /// The first folder (step A's area); `None` for a file at the top.
    fn area(&self) -> Option<&str> {
        match self.segments.as_slice() {
            [first, _, ..] => Some(first),
            _ => None,
        }
    }

    /// The rule the path was normalized with.
    pub fn rule(&self) -> CaseRule {
        self.rule
    }
}

/// `C:` alone or `C:/...`: a Windows drive, never a repository path. A name like `a:b` is an
/// ordinary file name on Linux and macOS and stays allowed; Windows cannot check it out, but a
/// name that only exists on some platforms is a hash that simply matches nothing there.
fn has_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.len() == 2 || bytes[2] == b'/')
}

/// The hashes of one changed file: its first folder (`area`, `None` for a file at the top) and
/// the whole path. Equal `area` is the same area of step A, an equal `file` the same file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathHashes {
    pub area: Option<KeyedHash>,
    pub file: KeyedHash,
}

/// The team's matching key of one epoch. Never serialized, never printed; the hash state is
/// wiped when it is dropped.
#[derive(Clone)]
pub struct MatchKey {
    mac: HmacSha256,
}

impl fmt::Debug for MatchKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MatchKey(..)")
    }
}

impl MatchKey {
    /// Uses `key` as it is: only inside the crate (tests), so a caller cannot key hashes with
    /// the epoch secret by mistake.
    pub(crate) fn from_bytes(key: &[u8; 32]) -> Result<Self, KeyError> {
        let mac = HmacSha256::new_from_slice(key).map_err(|_| KeyError)?;
        Ok(Self { mac })
    }

    fn hash(&self, label: &[u8], fields: &[&[u8]]) -> KeyedHash {
        let mut mac = self.mac.clone();
        for part in std::iter::once(label).chain(fields.iter().copied()) {
            mac.update(&length_prefix(part));
            mac.update(part);
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&mac.finalize().into_bytes());
        KeyedHash(bytes)
    }

    /// The keyed hash of a task id (`task.reserve` and the other task messages).
    pub fn task(&self, task: &TaskId) -> KeyedHash {
        self.hash(LABEL_TASK, &[task.as_str().as_bytes()])
    }

    /// The keyed hash of a repository (`claim.set`'s `repo`).
    pub fn repo(&self, repo: &RepoId) -> KeyedHash {
        self.hash(LABEL_REPO, &[repo.as_str().as_bytes()])
    }

    /// The keyed hashes of a changed file in `repo`: its first folder and the whole path.
    pub fn path(&self, repo: &RepoId, path: &RepoPath) -> PathHashes {
        let repo = repo.as_str().as_bytes();
        let rule = path.rule.label();
        let area = path
            .area()
            .map(|area| self.hash(LABEL_AREA, &[repo, rule, area.as_bytes()]));
        let file = self.hash(LABEL_FILE, &[repo, rule, path.as_string().as_bytes()]);
        PathHashes { area, file }
    }
}

/// Lengths are bounded far below 4 GiB by the limits; saturate rather than wrap.
fn length_prefix(part: &[u8]) -> [u8; 4] {
    u32::try_from(part.len()).unwrap_or(u32::MAX).to_be_bytes()
}

/// The key step E seals content with. Wiped when dropped, never printed.
#[derive(Clone)]
pub struct SealKey(Zeroizing<[u8; 32]>);

impl fmt::Debug for SealKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SealKey(..)")
    }
}

impl SealKey {
    /// The raw key for the cipher.
    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }
}

/// The keys of one team epoch, derived from its secret with HKDF-SHA256 (RFC 5869, no salt),
/// one per use: `info` is the use's label, the team and the epoch, each length-prefixed.
#[derive(Debug, Clone)]
pub struct EpochKeys {
    match_key: MatchKey,
    seal_key: SealKey,
}

impl EpochKeys {
    /// Derives both keys of `epoch` from the epoch's secret (the caller wipes its own copy).
    pub fn derive(epoch_secret: &[u8; 32], team: &TeamId, epoch: Epoch) -> Result<Self, KeyError> {
        let hkdf = Hkdf::<Sha256>::new(None, epoch_secret);
        let expand = |label: &[u8]| -> Result<Zeroizing<[u8; 32]>, KeyError> {
            let team = team.as_str().as_bytes();
            let epoch = epoch.0.to_be_bytes();
            let mut info = Vec::new();
            for part in [label, team, &epoch] {
                info.extend_from_slice(&length_prefix(part));
                info.extend_from_slice(part);
            }
            let mut okm = Zeroizing::new([0u8; 32]);
            hkdf.expand(&info, okm.as_mut()).map_err(|_| KeyError)?;
            Ok(okm)
        };
        let match_key = MatchKey::from_bytes(&*expand(INFO_MATCH_KEY)?)?;
        let seal_key = SealKey(expand(INFO_SEAL_KEY)?);
        Ok(Self {
            match_key,
            seal_key,
        })
    }

    /// The key for keyed hashes.
    pub fn match_key(&self) -> &MatchKey {
        &self.match_key
    }

    /// The key for sealed content.
    pub fn seal_key(&self) -> &SealKey {
        &self.seal_key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(value: &str) -> KeyedHash {
        KeyedHash::try_from(value.to_owned()).unwrap()
    }

    fn key() -> MatchKey {
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::try_from(i).unwrap();
        }
        MatchKey::from_bytes(&bytes).unwrap()
    }

    const REPO: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

    fn fold(path: &str) -> RepoPath {
        RepoPath::normalize(path, CaseRule::Fold).unwrap()
    }

    #[test]
    fn hmac_sha256_matches_rfc_4231() {
        // RFC 4231, test case 2, so the primitive itself is anchored.
        let mut mac = HmacSha256::new_from_slice(b"Jefe").unwrap();
        mac.update(b"what do ya want for nothing?");
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&mac.finalize().into_bytes());
        assert_eq!(
            KeyedHash(bytes).to_hex(),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn hkdf_sha256_matches_rfc_5869() {
        // RFC 5869, test case 3 (no salt, no info), so the derivation is anchored too.
        let hkdf = Hkdf::<Sha256>::new(None, &[0x0b; 22]);
        let mut okm = [0u8; 42];
        hkdf.expand(&[], &mut okm).unwrap();
        let hex: String = okm.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "8da4e775a563c18f715f802a063c5a31b8a11f5c5ee1879ec3454e5f3c738d2d\
             9d201395faa4b61a96c8"
        );
    }

    // Vectors computed independently with Python's standard library (`test_vectors.py`).
    #[test]
    fn test_vectors() {
        let key = key();
        let repo = RepoId::new(REPO).unwrap();
        assert_eq!(
            key.task(&TaskId::normalize("  GH:Owner/Repo#12 ").unwrap()),
            hex(VECTOR_TASK)
        );
        assert_eq!(key.repo(&repo), hex(VECTOR_REPO));
        let hashes = key.path(&repo, &fold("Src\\Auth/./Login.rs"));
        assert_eq!(hashes.area, Some(hex(VECTOR_AREA)));
        assert_eq!(hashes.file, hex(VECTOR_FILE));
        let sensitive = RepoPath::normalize("src/auth/login.rs", CaseRule::Sensitive).unwrap();
        assert_eq!(key.path(&repo, &sensitive).file, hex(VECTOR_FILE_SENSITIVE));
        // Decomposed input (as macOS may store it) gives the composed value.
        assert_eq!(
            key.path(&repo, &fold("Cafe\u{301}/Menu.md")).file,
            hex(VECTOR_FILE_NFC)
        );
        let team = TeamId::new("team_1").unwrap();
        let keys = EpochKeys::derive(&[7u8; 32], &team, Epoch(3)).unwrap();
        assert_eq!(
            keys.match_key().task(&TaskId::normalize("t-1").unwrap()),
            hex(VECTOR_DERIVED_TASK)
        );
        assert_eq!(
            KeyedHash(*keys.seal_key().expose()),
            hex(VECTOR_DERIVED_SEAL_KEY)
        );
    }

    const VECTOR_TASK: &str = "a29fbd80b5010a12fe0b2e996feefa819e9c87ceb202e62a02b5869883e52d87";
    const VECTOR_REPO: &str = "8e06e77b4a0768b732de07088f8d92987847f7ce97e11791a93409882613c6fb";
    const VECTOR_AREA: &str = "6a1b7ec0e74c561ccd95bb2a7ed7418bd1e539c18621d11b76d2c2ad3385e5e8";
    const VECTOR_FILE: &str = "59bc01200147955b395873dff84f6304bfa27cca47b6ca154eabca6e0c168eaa";
    const VECTOR_FILE_SENSITIVE: &str =
        "16b18bbaf0707b99de7c54faf0dd49e366b4a46d90d336b07d7694a6d777838c";
    const VECTOR_FILE_NFC: &str =
        "293d4c7098d484faf7c0e354831cd8662b7d3c998677b24094137ec36147b282";
    const VECTOR_DERIVED_TASK: &str =
        "00e4e33da0609cacabc94dc4822294fe0532f3901aef6e222c3b6213cb100cee";
    const VECTOR_DERIVED_SEAL_KEY: &str =
        "f90bd8ae0f3ede5516786e3dd6382a14de6fad73988749845234046cf52f36d7";

    #[test]
    fn same_area_and_same_file_by_equality() {
        let key = key();
        let repo = RepoId::new(REPO).unwrap();
        let path = |p: &str| key.path(&repo, &fold(p));
        let a = path("src/auth/login.rs");
        let b = path("SRC/auth/session.rs");
        let c = path("src/ui/view.rs");
        let d = path("docs/auth/login.rs");
        assert_eq!(a.area, b.area);
        assert_ne!(a.file, b.file);
        assert_eq!(a.area, c.area);
        assert_ne!(a.area, d.area);
        // A folder and a file of the same name never share a hash.
        assert_ne!(path("src/x").area, Some(path("src").file));
        // Root files have no area.
        assert_eq!(path("README.md").area, None);
        // Another repository, another key or another case rule gives other values.
        let other = RepoId::new(&"a".repeat(64)).unwrap();
        assert_ne!(a.file, key.path(&other, &fold("src/auth/login.rs")).file);
        let mut other_key = [1u8; 32];
        other_key[0] = 2;
        let other_key = MatchKey::from_bytes(&other_key).unwrap();
        assert_ne!(other_key.path(&repo, &fold("src/auth/login.rs")), a);
        let sensitive = RepoPath::normalize("src/auth/login.rs", CaseRule::Sensitive).unwrap();
        assert_ne!(key.path(&repo, &sensitive), a);
    }

    #[test]
    fn epochs_give_different_keys_and_uses_are_separate() {
        let team = TeamId::new("team_1").unwrap();
        let task = TaskId::normalize("t-1").unwrap();
        let one = EpochKeys::derive(&[7u8; 32], &team, Epoch(1)).unwrap();
        let two = EpochKeys::derive(&[7u8; 32], &team, Epoch(2)).unwrap();
        assert_ne!(one.match_key().task(&task), two.match_key().task(&task));
        assert_ne!(one.seal_key().expose(), two.seal_key().expose());
        // The matching key is not the sealing key.
        let as_match = MatchKey::from_bytes(one.seal_key().expose()).unwrap();
        assert_ne!(as_match.task(&task), one.match_key().task(&task));
        let other_team = TeamId::new("team_2").unwrap();
        let three = EpochKeys::derive(&[7u8; 32], &other_team, Epoch(1)).unwrap();
        assert_ne!(one.seal_key().expose(), three.seal_key().expose());
        // Keys never show in debug output.
        assert_eq!(
            format!("{one:?}"),
            "EpochKeys { match_key: MatchKey(..), seal_key: SealKey(..) }"
        );
    }

    #[test]
    fn paths_are_normalized() {
        let fold = |p: &str| RepoPath::normalize(p, CaseRule::Fold).map(|p| p.as_string());
        assert_eq!(fold("a\\B//c/./D.rs"), Ok("a/b/c/d.rs".to_owned()));
        assert_eq!(fold("src/"), Ok("src".to_owned()));
        assert_eq!(fold("ÄÖ/Straße.md"), Ok("äö/straße.md".to_owned()));
        assert_eq!(fold("Cafe\u{301}/x"), Ok("caf\u{e9}/x".to_owned()));
        assert_eq!(
            RepoPath::normalize("Src/A.rs", CaseRule::Sensitive).map(|p| p.as_string()),
            Ok("Src/A.rs".to_owned())
        );
        assert_eq!(fold("../x"), Err(NormalizeError::ParentSegment));
        assert_eq!(fold("a/../x"), Err(NormalizeError::ParentSegment));
        assert_eq!(fold("/etc/x"), Err(NormalizeError::Absolute));
        assert_eq!(fold("\\\\host\\share\\x"), Err(NormalizeError::Absolute));
        assert_eq!(fold("C:/x"), Err(NormalizeError::Absolute));
        assert_eq!(fold("C:\\x"), Err(NormalizeError::Absolute));
        assert_eq!(fold("c:"), Err(NormalizeError::Absolute));
        // Ordinary names on Linux and macOS (git never prints a drive).
        assert_eq!(fold("a:b"), Ok("a:b".to_owned()));
        assert_eq!(fold("c:x"), Ok("c:x".to_owned()));
        assert_eq!(fold("ab:/x"), Ok("ab:/x".to_owned()));
        for hidden in [
            "a\nb",
            "a\u{200b}b",
            "a\u{202e}b.rs",
            "a\u{2028}b",
            "\u{feff}a",
        ] {
            assert_eq!(fold(hidden), Err(NormalizeError::Hidden), "{hidden:?}");
        }
        assert_eq!(fold("./"), Err(NormalizeError::Empty));
        assert_eq!(fold(""), Err(NormalizeError::Empty));
        assert_eq!(
            fold(&"a".repeat(MAX_SEGMENT_BYTES + 1)),
            Err(NormalizeError::SegmentTooLong {
                max: MAX_SEGMENT_BYTES
            })
        );
        let long = vec!["abcdefgh"; 600].join("/");
        assert_eq!(
            fold(&long),
            Err(NormalizeError::TooLong {
                max: MAX_PATH_BYTES
            })
        );
    }

    #[test]
    fn task_ids_and_repo_ids_are_normalized() {
        assert_eq!(
            TaskId::normalize(" Linear:ABC-12 ").unwrap().as_str(),
            "linear:abc-12"
        );
        assert_eq!(
            TaskId::normalize("Cafe\u{301}").unwrap(),
            TaskId::normalize("Caf\u{e9}").unwrap()
        );
        assert_eq!(TaskId::normalize("  "), Err(NormalizeError::Empty));
        assert_eq!(TaskId::normalize("a\u{7}"), Err(NormalizeError::Hidden));
        assert_eq!(TaskId::normalize("a\u{200d}b"), Err(NormalizeError::Hidden));
        assert_eq!(TaskId::normalize("a\u{2029}b"), Err(NormalizeError::Hidden));
        assert!(TaskId::normalize(&"x".repeat(MAX_TASK_ID_BYTES + 1)).is_err());
        assert_eq!(RepoId::new(&REPO.to_uppercase()).unwrap().as_str(), REPO);
        assert_eq!(RepoId::new("abc"), Err(NormalizeError::RepoId));
        assert_eq!(RepoId::new(&"g".repeat(40)), Err(NormalizeError::RepoId));
    }

    #[test]
    fn names_never_show_in_debug_output() {
        let task = TaskId::normalize("acme-secret-project#1").unwrap();
        assert_eq!(format!("{task:?}"), "TaskId(..)");
        let path = fold("clients/acme/contract.md");
        assert_eq!(format!("{path:?}"), "RepoPath(..)");
    }

    #[test]
    fn keyed_hash_text_has_one_spelling() {
        let value = "00".repeat(31) + "ff";
        let parsed = hex(&value);
        assert_eq!(parsed.to_string(), value);
        assert!(KeyedHash::try_from("00".repeat(31) + "FF").is_err());
        assert!(KeyedHash::try_from("0".repeat(63)).is_err());
        assert!(KeyedHash::try_from("zz".repeat(32)).is_err());
        let json = serde_json::to_string(&parsed).unwrap();
        assert_eq!(json, format!("\"{value}\""));
    }
}
