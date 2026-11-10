//! Shared project notes, phase 1 (ADR 0006: `docs/wiki/pages/decisions/0006-encrypted-project-notes.md`).
//!
//! Notes that travel with the repository, encrypted, in its main worktree:
//!
//! ```text
//! .hangar/notes/
//!   .gitattributes   "* -text -diff -merge": bytes stay as written, no text diff, no merge
//!   README.md        what the folder is, no project data
//!   vault.json       format, random vault id, mode and (solo) the code slot
//!   <note id>.note   one encrypted file per note (format in `hangar_team_crypto::notes`)
//! ```
//!
//! Phase 1 is solo mode: the key ring lives in the OS keychain (service [`KEYCHAIN_SERVICE`],
//! account `notes/<vault id>`) and in `vault.json`'s code slot under the notes recovery code,
//! which setup shows once and which unlocks the notes on another clone. Team vaults
//! (`"mode": "team"`) show as locked until phase 2.
//!
//! Hangar writes a note only when its decrypted content changed, never re-encrypts untouched
//! notes, never commits and stages nothing except the files of a conflict it resolved. Deleting
//! moves the encrypted file into the app data folder (`notes/shared/<vault id>/.trash/`), so
//! Undo works as for private notes. Decrypted text is never written to disk. Contents, titles and
//! keys are never logged.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use hangar_team_crypto::notes::{
    self as crypto, KeyRing, NotesCryptoError, NotesRecoveryCode, VaultId, MAX_FILE_LEN,
};
use hangar_team_crypto::recovery::RecoveryError;
use hangar_team_crypto::OsEntropy;
use serde::{Deserialize, Serialize, Serializer};
use zeroize::Zeroizing;

use crate::git::{self, Git, GitError};
use crate::lock;
use crate::notes::{self, NoteFolder, NoteInfo, NotesError, NotesStore, SearchHit, TrashInfo};
use crate::persist::{create_private_dir, write_atomic};
use crate::secrets::{SecretError, SecretStore};

/// Keychain service of the notes key rings. One for release and debug builds alike: a key ring
/// is data of the person, not a device identity, and a dev build must open the same notes.
pub const KEYCHAIN_SERVICE: &str = "hangar-notes";
/// The folder inside the main worktree.
pub const NOTES_PATH: &str = ".hangar/notes";
/// Below the private notes folder of the app data: per vault, the trash of deleted shared notes.
pub const SHARED_TRASH_DIR: &str = "shared";
pub const VAULT_FILE: &str = "vault.json";
/// Suggested name and type of the file the recovery code is saved to.
pub const CODE_FILE_NAME: &str = "hangar-notes-recovery-code.txt";
pub const CODE_FILE_FILTER: (&str, &str) = ("Text", "txt");
/// How long a copied recovery code stays on the clipboard.
pub const CODE_CLIPBOARD_TTL: std::time::Duration = std::time::Duration::from_secs(60);
const README_FILE: &str = "README.md";
const ATTRIBUTES_FILE: &str = ".gitattributes";
const ATTRIBUTES: &str = "* -text -diff -merge\n";
const README: &str = "# Encrypted Hangar project notes\n\n\
Each `.note` file in this folder is one note written in Hangar, encrypted end to end. Hangar \
opens them with a key kept on your computer, or with the notes recovery code on another \
computer. Titles, texts and times are inside the encryption; editing these files by hand makes \
a note unreadable.\n";
const VAULT_FORMAT: u64 = 1;
const NOTE_EXTENSION: &str = ".note";
const META_VERSION: u64 = 1;
const MAX_VAULT_BYTES: usize = 64 * 1024;
const TRASH_DIR: &str = ".trash";
/// The label "other version" of a conflict copy's title: short, one line.
const MAX_LABEL_CHARS: usize = 40;
/// Notes a vault lists at most; a folder with more files is not read further.
const MAX_NOTES: usize = 5000;
/// Bytes one scan of a vault reads at most; beyond that, files count as unreadable.
const MAX_SCAN_BYTES: u64 = 256 * 1024 * 1024;
/// A prepared setup whose dialog was left open longer than this starts again.
const PENDING_TTL: std::time::Duration = std::time::Duration::from_secs(15 * 60);
/// Title of a note whose own title the app would refuse, until the window names it.
const FALLBACK_TITLE: &str = "Note";

#[derive(Debug, thiserror::Error)]
pub enum ProjectNotesError {
    #[error("Shared notes need a Git repository")]
    NotRepository,
    #[error("Shared notes are already turned on in this repository")]
    AlreadyOn,
    #[error("Shared notes are not turned on in this repository")]
    NotOn,
    #[error("Start turning on shared notes again")]
    NoSetup,
    #[error("Shared notes are locked on this computer")]
    Locked,
    #[error("vault.json is damaged")]
    Damaged,
    #[error("These notes were made with a newer version of Hangar")]
    NewerFormat,
    #[error("Shared notes of a team are not supported by this version of Hangar")]
    TeamVault,
    #[error("Note not found")]
    UnknownNote,
    #[error("The note cannot be opened")]
    Unreadable,
    #[error("Invalid title: {0}")]
    InvalidTitle(&'static str),
    #[error("Invalid label")]
    InvalidLabel,
    #[error("Note too large: {size} bytes, at most {max} allowed")]
    TooLarge { size: usize, max: usize },
    #[error("Shared notes are encrypted and have no readable file to show")]
    NoFile,
    #[error("Background task aborted: {0}")]
    Task(String),
    #[error("The notes folder is a link or lies outside the repository; Hangar does not use it")]
    UnsafeFolder,
    #[error("The keychain holds notes keys this version cannot read; they were left as they are")]
    KeychainUnusable,
    #[error("Too many notes keys for the keychain")]
    TooManyKeys,
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Keychain(#[from] SecretError),
    #[error("Encryption failed: {0}")]
    Crypto(#[from] NotesCryptoError),
    #[error("File {path} cannot be read or written: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl Serialize for ProjectNotesError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> ProjectNotesError + '_ {
    move |source| ProjectNotesError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn title_error(error: NotesError) -> ProjectNotesError {
    match error {
        NotesError::InvalidTitle(reason) => ProjectNotesError::InvalidTitle(reason),
        _ => ProjectNotesError::InvalidTitle("invalid"),
    }
}

// ---------------------------------------------------------------------------------------------
// States the window shows

/// Why the Shared group is locked; each reason has at most one action in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LockReason {
    /// Solo vault without its key on this computer: "Enter the notes recovery code".
    NeedsCode,
    /// A team vault (phase 2); no action.
    Team,
    /// `vault.json` cannot be read; no action.
    Damaged,
}

/// A conflict Hangar resolved: the note kept and the copy made of the other version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedConflict {
    pub title: String,
    /// Title of the copy; `None` when the other side had deleted the note or both were equal.
    pub copy: Option<String>,
}

/// State of the Shared group for one working folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SharedStatus {
    /// Not inside a Git repository (or no git): the window shows no Shared group.
    Unavailable,
    /// No `.hangar/notes/vault.json`: the setup line.
    Off,
    Locked {
        reason: LockReason,
    },
    /// `vault.json` of a newer format: read only, no action.
    NewerFormat,
    Unlocked {
        /// Changes below `.hangar/notes` that are not committed yet.
        uncommitted: usize,
        /// `.gitignore` keeps the folder out of Git.
        ignored: bool,
        /// Note files that do not open here (another key, another vault, damaged, newer).
        unreadable: usize,
        /// Conflicts resolved by this call.
        resolved: Vec<ResolvedConflict>,
    },
}

/// What typing a notes recovery code did. A wrong code is an answer, not an error, so the window
/// can say which kind of wrong in its own language.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum Unlock {
    Unlocked {
        status: SharedStatus,
    },
    /// Well formed, but it does not open this vault's code slot.
    WrongCode,
    /// The checksum does not match.
    Typo,
    /// A team recovery code was typed.
    TeamCode,
}

/// What saving the recovery code as a file did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "camelCase")]
pub enum SaveCode {
    Saved {
        path: String,
    },
    Cancelled,
    /// The chosen place is inside the repository or another Git work tree; nothing was written.
    InsideRepository,
}

// ---------------------------------------------------------------------------------------------
// vault.json

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultMode {
    Solo { code_slot: String },
    Team { team: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Vault {
    pub vault: VaultId,
    pub mode: VaultMode,
}

/// What `vault.json` holds, as far as this version understands it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultFile {
    Known(Vault),
    /// A newer format or an unknown mode.
    Newer,
    Damaged,
}

fn is_hex(text: &str, len: usize) -> bool {
    text.len() == len
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Reads `vault.json`. Unknown keys are tolerated (a later minor addition); a missing or
/// malformed required key is damage.
pub fn parse_vault(bytes: &[u8]) -> VaultFile {
    let Ok(serde_json::Value::Object(map)) = serde_json::from_slice::<serde_json::Value>(bytes)
    else {
        return VaultFile::Damaged;
    };
    let text = |key: &str| map.get(key).and_then(serde_json::Value::as_str);
    match map.get("format").and_then(serde_json::Value::as_u64) {
        Some(VAULT_FORMAT) => {}
        Some(format) if format > VAULT_FORMAT => return VaultFile::Newer,
        _ => return VaultFile::Damaged,
    }
    let Some(vault) = text("vault").and_then(VaultId::from_hex) else {
        return VaultFile::Damaged;
    };
    let mode = match text("mode") {
        Some("solo") => match text("codeSlot") {
            Some(slot) if !slot.is_empty() => VaultMode::Solo {
                code_slot: slot.to_owned(),
            },
            _ => return VaultFile::Damaged,
        },
        Some("team") => match text("team") {
            Some(team) if is_hex(team, 16) => VaultMode::Team {
                team: team.to_owned(),
            },
            _ => return VaultFile::Damaged,
        },
        Some(_) => return VaultFile::Newer,
        None => return VaultFile::Damaged,
    };
    VaultFile::Known(Vault { vault, mode })
}

/// `vault.json` as Hangar writes it: keys in a fixed order, pretty printed, LF, final newline.
pub fn vault_json(vault: &Vault) -> String {
    let quote = |text: &str| serde_json::Value::String(text.to_owned()).to_string();
    let (mode, extra) = match &vault.mode {
        VaultMode::Solo { code_slot } => {
            ("solo", format!("  \"codeSlot\": {}\n", quote(code_slot)))
        }
        VaultMode::Team { team } => ("team", format!("  \"team\": {}\n", quote(team))),
    };
    format!(
        "{{\n  \"format\": {VAULT_FORMAT},\n  \"vault\": {},\n  \"mode\": {},\n{extra}}}\n",
        quote(&vault.vault.to_hex()),
        quote(mode)
    )
}

// ---------------------------------------------------------------------------------------------
// The plaintext inside a note

/// The metadata line before the Markdown. Field order is the file order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteMeta {
    pub v: u64,
    pub title: String,
    /// Unix time in milliseconds.
    pub created: u64,
    pub updated: u64,
    /// Display name of the last editor in team mode; empty in solo mode.
    pub by: String,
}

/// A decrypted note.
#[derive(Clone, PartialEq, Eq)]
pub struct Plain {
    pub meta: NoteMeta,
    pub body: Zeroizing<String>,
}

impl std::fmt::Debug for Plain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Plain(..)")
    }
}

/// One JSON line, a line feed, then the Markdown.
pub fn encode_plain(plain: &Plain) -> Result<Zeroizing<Vec<u8>>, ProjectNotesError> {
    let line = serde_json::to_vec(&plain.meta).map_err(|_| NotesCryptoError::Malformed)?;
    let mut out = Zeroizing::new(Vec::with_capacity(line.len() + 1 + plain.body.len()));
    out.extend_from_slice(&line);
    out.push(b'\n');
    out.extend_from_slice(plain.body.as_bytes());
    Ok(out)
}

/// `untitled` is the title shown for a note whose own title the app would refuse.
pub fn decode_plain(bytes: &[u8], untitled: &str) -> Result<Plain, ProjectNotesError> {
    let split = bytes
        .iter()
        .position(|b| *b == b'\n')
        .ok_or(ProjectNotesError::Unreadable)?;
    let mut meta: NoteMeta =
        serde_json::from_slice(&bytes[..split]).map_err(|_| ProjectNotesError::Unreadable)?;
    if meta.v != META_VERSION {
        return Err(if meta.v > META_VERSION {
            ProjectNotesError::NewerFormat
        } else {
            ProjectNotesError::Unreadable
        });
    }
    // Any key holder can write a note; a title the app would refuse is shown as `untitled`.
    meta.title = notes::validate_title(&meta.title).unwrap_or_else(|_| untitled.to_owned());
    let body =
        std::str::from_utf8(&bytes[split + 1..]).map_err(|_| ProjectNotesError::Unreadable)?;
    Ok(Plain {
        meta,
        body: Zeroizing::new(body.to_owned()),
    })
}

// ---------------------------------------------------------------------------------------------
// Conflicts

/// What to write for a conflicted note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// Keep "ours" (stage 2) as it is.
    KeepOurs,
    /// Keep "theirs" (stage 3) under the note's id.
    KeepTheirs,
    /// Keep one under the id and add the other one as a new note with `copy_title`.
    KeepBoth { keep_ours: bool, copy_title: String },
}

/// `Title (other version, Name)` within the title limit; without a name `Title (other version)`.
pub fn copy_title(title: &str, label: &str, by: &str) -> String {
    let suffix = if by.trim().is_empty() {
        format!(" ({label})")
    } else {
        format!(" ({label}, {})", by.trim())
    };
    let room = notes::MAX_TITLE_CHARS.saturating_sub(suffix.chars().count());
    let base: String = title.chars().take(room).collect();
    let joined = format!("{}{suffix}", base.trim_end());
    let capped: String = joined.chars().take(notes::MAX_TITLE_CHARS).collect();
    notes::validate_title(&capped).unwrap_or_else(|_| label.to_owned())
}

/// Lossless resolution of one conflicted note: the version changed later stays under the id
/// (ours on a tie), the other becomes a new note named after its last editor; a side that
/// deleted the note loses to the side that kept it. Equal versions need no copy.
pub fn resolve(ours: Option<&Plain>, theirs: Option<&Plain>, label: &str) -> Option<Resolution> {
    match (ours, theirs) {
        (Some(_), None) => Some(Resolution::KeepOurs),
        (None, Some(_)) => Some(Resolution::KeepTheirs),
        (None, None) => None,
        (Some(ours), Some(theirs)) => {
            if ours.meta.title == theirs.meta.title && *ours.body == *theirs.body {
                return Some(Resolution::KeepOurs);
            }
            let keep_ours = ours.meta.updated >= theirs.meta.updated;
            let other = if keep_ours { theirs } else { ours };
            Some(Resolution::KeepBoth {
                keep_ours,
                copy_title: copy_title(&other.meta.title, label, &other.meta.by),
            })
        }
    }
}

/// Checks the label the window sends for conflict copies.
pub fn check_label(label: &str) -> Result<&str, ProjectNotesError> {
    let label = label.trim();
    if label.is_empty()
        || label.chars().count() > MAX_LABEL_CHARS
        || label
            .chars()
            .any(|c| c.is_control() || matches!(c, '(' | ')' | ','))
    {
        return Err(ProjectNotesError::InvalidLabel);
    }
    Ok(label)
}

// ---------------------------------------------------------------------------------------------
// Git

/// The first `worktree` entry of `git worktree list --porcelain`, unless it is bare.
pub fn main_worktree(output: &[u8]) -> Option<PathBuf> {
    let text = String::from_utf8_lossy(output);
    let mut lines = text.lines();
    let first = lines.next()?.strip_prefix("worktree ")?;
    for line in lines {
        if line.is_empty() {
            break;
        }
        if line == "bare" {
            return None;
        }
    }
    Some(git::native_path(first))
}

/// One record of `git status --porcelain=v1 -z`: the XY code and the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    pub code: String,
    pub path: String,
}

/// Which index stages a conflict of this XY code must have: (ours, theirs). `None` for codes
/// that are not two-sided conflicts (`DD` is handled on its own).
pub fn expected_stages(code: &str) -> Option<(bool, bool)> {
    match code {
        "UU" | "AA" => Some((true, true)),
        "UD" | "AU" => Some((true, false)),
        "DU" | "UA" => Some((false, true)),
        _ => None,
    }
}

impl StatusEntry {
    pub fn unmerged(&self) -> bool {
        matches!(
            self.code.as_str(),
            "DD" | "AU" | "UD" | "UA" | "DU" | "AA" | "UU"
        )
    }
}

pub fn parse_status(output: &[u8]) -> Vec<StatusEntry> {
    let mut entries = Vec::new();
    let mut records = output.split(|b| *b == 0);
    while let Some(record) = records.next() {
        if record.len() < 4 || record.get(2) != Some(&b' ') {
            continue;
        }
        let code = String::from_utf8_lossy(&record[..2]).into_owned();
        let path = String::from_utf8_lossy(&record[3..]).into_owned();
        // Renames and copies carry the old path as a second record.
        if code.starts_with(['R', 'C']) {
            records.next();
        }
        entries.push(StatusEntry { code, path });
    }
    entries
}

/// `<uuid>.note` directly in `.hangar/notes`; the note id, or `None`.
fn note_id_of(name: &str) -> Option<&str> {
    name.strip_suffix(NOTE_EXTENSION)
        .filter(|id| crypto::is_note_id(id))
}

fn note_path_id(path: &str) -> Option<&str> {
    note_id_of(path.strip_prefix(NOTES_PATH)?.strip_prefix('/')?)
}

// ---------------------------------------------------------------------------------------------
// The store

/// Where the shared notes of a working folder live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    /// The main worktree (or the folder's own work tree as a fallback).
    pub root: PathBuf,
    pub dir: PathBuf,
}

struct Pending {
    vault: VaultId,
    ring: KeyRing,
    code: NotesRecoveryCode,
    created: Instant,
}

impl Pending {
    fn expired(&self) -> bool {
        self.created.elapsed() > PENDING_TTL
    }
}

/// An unlocked vault: the key ring and where its files are.
struct Open {
    vault: Vault,
    ring: KeyRing,
    project: Project,
    untitled: String,
}

#[derive(Default)]
struct State {
    /// Unlocked key rings of this run, per vault.
    rings: HashMap<VaultId, KeyRing>,
    /// Working folder key to its project; only positive answers are kept.
    projects: HashMap<String, Project>,
    /// Vaults unlocked in this run, for search.
    known: HashMap<VaultId, Project>,
    /// Setups waiting for "I saved it", per notes folder.
    pending: HashMap<PathBuf, Pending>,
    /// Decrypted metadata per file, valid while modification time and length stay the same;
    /// `None` for a file that did not open, so it is not read again until it changes.
    meta: MetaCache,
    /// The window's title for notes without a usable one (set by every status call).
    untitled: Option<String>,
}

type MetaCache = HashMap<PathBuf, CachedMeta>;

/// A cache entry: file stamp, the vault and key ids it was decrypted with, and the result.
/// Another vault or another key set (a ring merged on unlock) makes it stale.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedMeta {
    modified: Option<SystemTime>,
    len: u64,
    context: String,
    meta: Option<NoteMeta>,
}

/// The vault id and the ring's key ids, as one comparable text.
fn cache_context(vault: &VaultId, ring: &KeyRing) -> String {
    let mut text = vault.to_hex();
    for id in ring.ids() {
        text.push(':');
        text.push_str(&id.to_hex());
    }
    text
}

/// The two sides of a conflict: ours (stage 2) and theirs (stage 3), `None` for a deleted side.
type Sides = (Option<Vec<u8>>, Option<Vec<u8>>);

/// What the keychain holds for a vault.
enum Stored {
    Nothing,
    Ring(KeyRing),
    /// Unreadable or of a newer format: never overwritten.
    Unusable,
}

/// Shared notes of every project. Cheap to clone; one lock for all files and caches.
#[derive(Clone)]
pub struct ProjectNotes {
    keychain: Arc<dyn SecretStore>,
    trash_root: PathBuf,
    state: Arc<Mutex<State>>,
    /// Held for a whole [`ProjectNotes::status`]; never taken while `state` is held.
    resolving: Arc<Mutex<()>>,
}

impl std::fmt::Debug for ProjectNotes {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectNotes")
            .field("trash_root", &self.trash_root)
            .finish_non_exhaustive()
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

fn keychain_account(vault: &VaultId) -> String {
    format!("notes/{}", vault.to_hex())
}

fn note_file(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}{NOTE_EXTENSION}"))
}

fn read_file(path: &Path) -> Result<Option<Vec<u8>>, ProjectNotesError> {
    use std::io::Read as _;
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(io_error(path)(error)),
    };
    // Only regular files: a link could lead out of the repository, a FIFO could block.
    if !meta.is_file() {
        return Err(ProjectNotesError::Unreadable);
    }
    let file = fs::File::open(path).map_err(io_error(path))?;
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_FILE_LEN).unwrap_or(u64::MAX) + 1;
    file.take(limit)
        .read_to_end(&mut bytes)
        .map_err(io_error(path))?;
    if bytes.len() > MAX_FILE_LEN {
        return Err(ProjectNotesError::Unreadable);
    }
    Ok(Some(bytes))
}

/// `.hangar` and `.hangar/notes` are real folders inside `root`, not links or junctions that lead
/// elsewhere. `Ok(false)` while the notes folder does not exist yet.
pub fn check_notes_dir(root: &Path) -> Result<bool, ProjectNotesError> {
    let hangar = root.join(".hangar");
    let mut exists = true;
    for dir in [&hangar, &hangar.join("notes")] {
        match fs::symlink_metadata(dir) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                return Err(ProjectNotesError::UnsafeFolder)
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                exists = false;
                break;
            }
            Err(error) => return Err(io_error(dir)(error)),
        }
    }
    if exists {
        // A junction or mount point that the metadata does not report as a link still resolves
        // to another place; the canonical paths tell.
        let real_root = fs::canonicalize(root).map_err(io_error(root))?;
        let real_dir = fs::canonicalize(hangar.join("notes")).map_err(io_error(&hangar))?;
        if !real_dir.starts_with(&real_root) {
            return Err(ProjectNotesError::UnsafeFolder);
        }
    }
    Ok(exists)
}

/// Whether a recovery code may be saved at `path`: not inside the repository and not inside any
/// other Git work tree, where it could be committed by accident.
pub fn code_path_allowed(root: &Path, path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(real_parent) = fs::canonicalize(parent) else {
        return false;
    };
    if let Ok(real_root) = fs::canonicalize(root) {
        if real_parent.starts_with(real_root) {
            return false;
        }
    }
    // Git may refuse a repository it does not trust (`safe.directory` on FAT, exFAT, network
    // drives or another account's folder) and then reports none; a `.git` entry on the way up
    // counts as a work tree whatever git says.
    if real_parent
        .ancestors()
        .any(|dir| fs::symlink_metadata(dir.join(".git")).is_ok())
    {
        return false;
    }
    match Git::locate() {
        Ok(git) => !matches!(
            git::toplevel(&git, &real_parent, Instant::now() + git::TIMEOUT),
            Ok(Some(_)) | Err(_)
        ),
        // Without git no work tree can be found; the repository itself is excluded above.
        Err(_) => true,
    }
}

fn stamp(path: &Path) -> Option<(Option<SystemTime>, u64)> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok(), meta.len()))
}

impl ProjectNotes {
    /// `app_dir` is the local app data folder; the trash goes to `notes/shared/` below it.
    pub fn new(app_dir: &Path, keychain: Arc<dyn SecretStore>) -> Self {
        Self {
            keychain,
            trash_root: app_dir.join(notes::NOTES_DIR).join(SHARED_TRASH_DIR),
            state: Arc::default(),
            resolving: Arc::default(),
        }
    }

    // ----- Locating the project ------------------------------------------------------------

    fn folder_path(folder: &NoteFolder) -> Result<&Path, ProjectNotesError> {
        folder
            .path
            .as_deref()
            .filter(|path| path.is_absolute())
            .ok_or(ProjectNotesError::NotRepository)
    }

    /// The project of a working folder: its clone's main worktree, so every task worktree sees
    /// the same notes. A bare clone, a missing main worktree or one on another share falls back
    /// to the folder's own work tree. `None` outside a repository or without git.
    pub fn locate(&self, folder: &Path) -> Result<Option<Project>, ProjectNotesError> {
        let key = git::path_key(folder);
        if let Some(project) = lock(&self.state).projects.get(&key) {
            if project.root.is_dir() {
                return Ok(Some(project.clone()));
            }
        }
        let git = match Git::locate() {
            Ok(git) => git,
            Err(GitError::NotFound) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        if git::check_folder(folder).is_err() {
            return Ok(None);
        }
        let deadline = Instant::now() + git::TIMEOUT;
        let Some(top) = git::toplevel(&git, folder, deadline)? else {
            return Ok(None);
        };
        let list = git.run(&top, ["worktree", "list", "--porcelain"], deadline, 1 << 20)?;
        let common = |dir: &Path| -> Option<PathBuf> {
            let output = git
                .run(
                    dir,
                    ["rev-parse", "--path-format=absolute", "--git-common-dir"],
                    deadline,
                    64 * 1024,
                )
                .ok()?;
            if !output.success {
                return None;
            }
            fs::canonicalize(git::native_path(&String::from_utf8_lossy(&output.stdout))).ok()
        };
        let own = common(&top);
        // The main worktree counts only if it is a real checkout of the same repository.
        let root = list
            .success
            .then(|| main_worktree(&list.stdout))
            .flatten()
            .filter(|main| {
                main.is_dir()
                    && git::gitdir_allowed(folder, main)
                    && fs::symlink_metadata(main.join(".git")).is_ok()
                    && own.is_some()
                    && common(main) == own
            })
            .unwrap_or(top);
        let project = Project {
            dir: root.join(".hangar").join("notes"),
            root,
        };
        lock(&self.state).projects.insert(key, project.clone());
        Ok(Some(project))
    }

    fn project(&self, folder: &NoteFolder) -> Result<Project, ProjectNotesError> {
        self.locate(Self::folder_path(folder)?)?
            .ok_or(ProjectNotesError::NotRepository)
    }

    fn read_vault(project: &Project) -> Result<Option<VaultFile>, ProjectNotesError> {
        if !check_notes_dir(&project.root)? {
            return Ok(None);
        }
        let path = project.dir.join(VAULT_FILE);
        match read_file(&path) {
            Ok(Some(bytes)) if bytes.len() <= MAX_VAULT_BYTES => Ok(Some(parse_vault(&bytes))),
            Ok(Some(_)) | Err(ProjectNotesError::Unreadable) => Ok(Some(VaultFile::Damaged)),
            Ok(None) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// The key ring of a vault: from this run's cache, else from the keychain. A keychain that
    /// fails counts as "no key" (the code still unlocks for this run); the reason goes to the log.
    fn ring(&self, vault: &VaultId) -> Option<KeyRing> {
        if let Some(ring) = lock(&self.state).rings.get(vault) {
            return Some(ring.clone());
        }
        match self.stored(vault) {
            Ok(Stored::Ring(ring)) => {
                lock(&self.state).rings.insert(*vault, ring.clone());
                Some(ring)
            }
            Ok(Stored::Nothing | Stored::Unusable) => None,
            Err(error) => {
                eprintln!("hangar: notes key not read from the keychain: {error}");
                None
            }
        }
    }

    /// The keychain entry of a vault; a read error is an error, not "nothing".
    fn stored(&self, vault: &VaultId) -> Result<Stored, ProjectNotesError> {
        let Some(text) = self.keychain.get(&keychain_account(vault))? else {
            return Ok(Stored::Nothing);
        };
        Ok(match KeyRing::from_keychain(&text) {
            Ok(ring) => Stored::Ring(ring),
            Err(_) => Stored::Unusable,
        })
    }

    /// Keeps the union of the stored, the cached and the new keys in the keychain, then in the
    /// cache. Refuses to overwrite an entry it cannot read or one of a newer format, stops on a
    /// keychain read error, and refuses a key ring that does not fit one keychain entry.
    fn remember(&self, vault: &VaultId, ring: &KeyRing) -> Result<(), ProjectNotesError> {
        let mut merged = match self.stored(vault)? {
            Stored::Unusable => return Err(ProjectNotesError::KeychainUnusable),
            Stored::Ring(stored) => stored,
            Stored::Nothing => ring.clone(),
        };
        if let Some(cached) = lock(&self.state).rings.get(vault).cloned() {
            merged.merge(&cached)?;
        }
        merged.merge(ring)?;
        let text = merged.to_keychain();
        if text.encode_utf16().count() > crate::secrets::MAX_VALUE_UNITS {
            return Err(ProjectNotesError::TooManyKeys);
        }
        self.keychain.set(&keychain_account(vault), &text)?;
        lock(&self.state).rings.insert(*vault, merged);
        Ok(())
    }

    /// The untitled title the window sent last.
    fn untitled(&self) -> String {
        lock(&self.state)
            .untitled
            .clone()
            .unwrap_or_else(|| FALLBACK_TITLE.to_owned())
    }

    fn open(&self, folder: &NoteFolder) -> Result<Open, ProjectNotesError> {
        let project = self.project(folder)?;
        let vault = match Self::read_vault(&project)? {
            None => return Err(ProjectNotesError::NotOn),
            Some(VaultFile::Newer) => return Err(ProjectNotesError::NewerFormat),
            Some(VaultFile::Damaged) => return Err(ProjectNotesError::Damaged),
            Some(VaultFile::Known(vault)) => vault,
        };
        if matches!(vault.mode, VaultMode::Team { .. }) {
            return Err(ProjectNotesError::TeamVault);
        }
        let ring = self.ring(&vault.vault).ok_or(ProjectNotesError::Locked)?;
        lock(&self.state).known.insert(vault.vault, project.clone());
        Ok(Open {
            vault,
            ring,
            project,
            untitled: self.untitled(),
        })
    }

    // ----- Status, setup, unlock -------------------------------------------------------------

    /// The Shared group's state. When unlocked, conflicted note files are resolved first
    /// (`label` names the copies: "Title (label, Name)") and staged; nothing else is staged.
    pub fn status(
        &self,
        folder: &NoteFolder,
        label: &str,
        untitled: &str,
    ) -> Result<SharedStatus, ProjectNotesError> {
        let label = check_label(label)?;
        let untitled = notes::validate_title(untitled).map_err(title_error)?;
        lock(&self.state).untitled = Some(untitled);
        let Some(project) = self.locate(Self::folder_path(folder)?)? else {
            return Ok(SharedStatus::Unavailable);
        };
        // The window asks on every focus: without the folder there is nothing to run git for.
        if !check_notes_dir(&project.root)? {
            return Ok(SharedStatus::Off);
        }
        // One status at a time: two calls must not resolve the same conflict twice.
        let _resolving = lock(&self.resolving);
        let git = Git::locate()?;
        let deadline = Instant::now() + git::TIMEOUT;
        let neutral = git::filter_overrides(&git, &project.root, deadline)?;
        let mut entries = self.git_status(&git, &project, &neutral, deadline)?;
        // A vault.json conflict first: it decides which vault the notes belong to.
        let mut resolved = Vec::new();
        let vault_path = format!("{NOTES_PATH}/{VAULT_FILE}");
        if let Some(entry) = entries
            .iter()
            .find(|e| e.unmerged() && e.path == vault_path)
            .cloned()
        {
            self.resolve_vault(&git, &project, &neutral, deadline, &entry.code)?;
            entries = self.git_status(&git, &project, &neutral, deadline)?;
        }
        let vault = match Self::read_vault(&project)? {
            None => return Ok(SharedStatus::Off),
            Some(VaultFile::Newer) => return Ok(SharedStatus::NewerFormat),
            Some(VaultFile::Damaged) => {
                return Ok(SharedStatus::Locked {
                    reason: LockReason::Damaged,
                })
            }
            Some(VaultFile::Known(vault)) => vault,
        };
        if matches!(vault.mode, VaultMode::Team { .. }) {
            return Ok(SharedStatus::Locked {
                reason: LockReason::Team,
            });
        }
        let Some(ring) = self.ring(&vault.vault) else {
            return Ok(SharedStatus::Locked {
                reason: LockReason::NeedsCode,
            });
        };
        lock(&self.state).known.insert(vault.vault, project.clone());
        let open = Open {
            vault,
            ring,
            project,
            untitled: self.untitled(),
        };
        let conflicted: Vec<StatusEntry> = entries
            .iter()
            .filter(|e| e.unmerged() && note_path_id(&e.path).is_some())
            .cloned()
            .collect();
        for entry in &conflicted {
            if entry.code == "DD" {
                // Both sides deleted the note (or renamed it away): nothing to keep.
                Self::forget_path(&git, &open.project.root, &neutral, deadline, &entry.path)?;
                continue;
            }
            if let Some(done) = self.resolve_note(&git, &open, &neutral, deadline, entry, label)? {
                resolved.push(done);
            }
        }
        if !conflicted.is_empty() {
            entries = self.git_status(&git, &open.project, &neutral, deadline)?;
        }
        let ignored = git
            .run(
                &open.project.root,
                neutral.iter().map(String::as_str).chain([
                    "check-ignore",
                    "-q",
                    "--",
                    &format!("{NOTES_PATH}/{VAULT_FILE}"),
                ]),
                deadline,
                4096,
            )?
            .success;
        let unreadable = Self::scan(&open, &mut lock(&self.state).meta)?.1;
        Ok(SharedStatus::Unlocked {
            uncommitted: entries.len(),
            ignored,
            unreadable,
            resolved,
        })
    }

    fn git_status(
        &self,
        git: &Git,
        project: &Project,
        neutral: &[String],
        deadline: Instant,
    ) -> Result<Vec<StatusEntry>, ProjectNotesError> {
        let output = git.run(
            &project.root,
            neutral.iter().map(String::as_str).chain([
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--ignore-submodules=all",
                "--",
                NOTES_PATH,
            ]),
            deadline,
            git::MAX_LIST_BYTES,
        )?;
        if !output.usable() {
            return Err(output.failure().into());
        }
        Ok(parse_status(&output.stdout))
    }

    /// The bytes of index stage `stage` (2: ours, 3: theirs), read only when the XY code says the
    /// stage exists. `None` when it could not be read whole (missing, failing, too long): the
    /// caller then leaves the conflict alone.
    fn stage(
        git: &Git,
        root: &Path,
        stage: u8,
        path: &str,
        deadline: Instant,
    ) -> Result<Option<Vec<u8>>, ProjectNotesError> {
        let output = git.run(
            root,
            ["cat-file", "blob", &format!(":{stage}:{path}")],
            deadline,
            MAX_FILE_LEN,
        )?;
        Ok((output.success && !output.truncated).then_some(output.stdout))
    }

    /// Both stages a conflict of `code` has: `Some((ours, theirs))`, each `None` when the code
    /// says the side deleted the file. `None` overall when an expected stage could not be read.
    fn stages(
        git: &Git,
        root: &Path,
        code: &str,
        path: &str,
        deadline: Instant,
    ) -> Result<Option<Sides>, ProjectNotesError> {
        let Some((want_ours, want_theirs)) = expected_stages(code) else {
            return Ok(None);
        };
        let mut sides = [None, None];
        for (slot, (wanted, stage)) in sides.iter_mut().zip([(want_ours, 2), (want_theirs, 3)]) {
            if !wanted {
                continue;
            }
            match Self::stage(git, root, stage, path, deadline)? {
                Some(bytes) => *slot = Some(bytes),
                None => return Ok(None),
            }
        }
        let [ours, theirs] = sides;
        Ok(Some((ours, theirs)))
    }

    /// `git rm --cached` for a path both sides deleted.
    fn forget_path(
        git: &Git,
        root: &Path,
        neutral: &[String],
        deadline: Instant,
        path: &str,
    ) -> Result<(), ProjectNotesError> {
        let output = git.run(
            root,
            neutral
                .iter()
                .map(String::as_str)
                .chain(["rm", "--cached", "-q", "--", path]),
            deadline,
            64 * 1024,
        )?;
        if !output.success {
            return Err(output.failure().into());
        }
        Ok(())
    }

    fn stage_paths(
        git: &Git,
        root: &Path,
        neutral: &[String],
        deadline: Instant,
        paths: &[&str],
    ) -> Result<(), ProjectNotesError> {
        let output = git.run(
            root,
            neutral
                .iter()
                .map(String::as_str)
                .chain(["add", "--"])
                .chain(paths.iter().copied()),
            deadline,
            64 * 1024,
        )?;
        if !output.success {
            return Err(output.failure().into());
        }
        Ok(())
    }

    fn resolve_note(
        &self,
        git: &Git,
        open: &Open,
        neutral: &[String],
        deadline: Instant,
        entry: &StatusEntry,
        label: &str,
    ) -> Result<Option<ResolvedConflict>, ProjectNotesError> {
        let path = entry.path.as_str();
        let Some(id) = note_path_id(path) else {
            return Ok(None);
        };
        let root = &open.project.root;
        let Some((ours_bytes, theirs_bytes)) =
            Self::stages(git, root, &entry.code, path, deadline)?
        else {
            return Ok(None);
        };
        let decrypt = |bytes: &Option<Vec<u8>>| -> Result<Option<Plain>, ()> {
            match bytes {
                None => Ok(None),
                Some(bytes) => {
                    let plain = crypto::open_note(&open.ring, &open.vault.vault, id, bytes)
                        .map_err(|_| ())?;
                    decode_plain(&plain, &open.untitled)
                        .map(Some)
                        .map_err(|_| ())
                }
            }
        };
        // A side that does not open here cannot be copied without loss; the conflict stays for
        // Git to show, and the working tree keeps ours.
        let (Ok(ours), Ok(theirs)) = (decrypt(&ours_bytes), decrypt(&theirs_bytes)) else {
            return Ok(None);
        };
        let Some(resolution) = resolve(ours.as_ref(), theirs.as_ref(), label) else {
            return Ok(None);
        };
        let _guard = lock(&self.state);
        let target = note_file(&open.project.dir, id);
        let mut staged = vec![path.to_owned()];
        let (kept, copy) = match &resolution {
            Resolution::KeepOurs => (ours.as_ref(), None),
            Resolution::KeepTheirs => (theirs.as_ref(), None),
            Resolution::KeepBoth {
                keep_ours,
                copy_title,
            } => {
                let (kept, other) = if *keep_ours {
                    (ours.as_ref(), theirs.as_ref())
                } else {
                    (theirs.as_ref(), ours.as_ref())
                };
                (kept, other.map(|other| (other, copy_title.clone())))
            }
        };
        let kept_bytes = match &resolution {
            Resolution::KeepTheirs => theirs_bytes.as_ref(),
            Resolution::KeepBoth {
                keep_ours: false, ..
            } => theirs_bytes.as_ref(),
            _ => ours_bytes.as_ref(),
        };
        // The kept version keeps its exact bytes: no new ciphertext for an unchanged note.
        if let Some(bytes) = kept_bytes {
            write_atomic(&target, bytes).map_err(io_error(&target))?;
        }
        let mut copy_title = None;
        if let Some((other, title)) = copy {
            let copy_id = uuid::Uuid::new_v4().to_string();
            let plain = Plain {
                meta: NoteMeta {
                    title: title.clone(),
                    ..other.meta.clone()
                },
                body: other.body.clone(),
            };
            self.write_plain(open, &copy_id, &plain)?;
            staged.push(format!("{NOTES_PATH}/{copy_id}{NOTE_EXTENSION}"));
            copy_title = Some(title);
        }
        let paths: Vec<&str> = staged.iter().map(String::as_str).collect();
        Self::stage_paths(git, root, neutral, deadline, &paths)?;
        Ok(kept.map(|kept| ResolvedConflict {
            title: kept.meta.title.clone(),
            copy: copy_title,
        }))
    }

    /// Two branches turned on notes. Resolved only when both sides are vaults of this format
    /// and this computer holds both key rings: then the vault whose id sorts first stays and the
    /// other vault's notes are re-encrypted into it. A side of a newer format, a damaged side, a
    /// side whose stage could not be read, or a vault whose key this computer lacks leaves the
    /// conflict to the person: a foreign vault must never win just by its id. One side deleted:
    /// the remaining vault stays if it is of this format.
    fn resolve_vault(
        &self,
        git: &Git,
        project: &Project,
        neutral: &[String],
        deadline: Instant,
        code: &str,
    ) -> Result<(), ProjectNotesError> {
        let path = format!("{NOTES_PATH}/{VAULT_FILE}");
        let Some((ours, theirs)) = Self::stages(git, &project.root, code, &path, deadline)? else {
            return Ok(());
        };
        let known = |bytes: Option<Vec<u8>>| -> Result<Option<(Vec<u8>, Vault)>, ()> {
            match bytes {
                None => Ok(None),
                Some(bytes) => match parse_vault(&bytes) {
                    VaultFile::Known(vault) => Ok(Some((bytes, vault))),
                    VaultFile::Newer | VaultFile::Damaged => Err(()),
                },
            }
        };
        let (Ok(ours), Ok(theirs)) = (known(ours), known(theirs)) else {
            return Ok(());
        };
        let (winner, loser) = match (ours, theirs) {
            (Some(a), Some(b)) if a.1.vault == b.1.vault => (a, None),
            (Some(a), Some(b)) => {
                let (Some(_), Some(_)) = (self.ring(&a.1.vault), self.ring(&b.1.vault)) else {
                    return Ok(());
                };
                if a.1.vault < b.1.vault {
                    (a, Some(b))
                } else {
                    (b, Some(a))
                }
            }
            (Some(one), None) | (None, Some(one)) => (one, None),
            (None, None) => return Ok(()),
        };
        let target = project.dir.join(VAULT_FILE);
        {
            let _guard = lock(&self.state);
            write_atomic(&target, &winner.0).map_err(io_error(&target))?;
        }
        let mut staged = vec![path.clone()];
        if let Some((_, loser)) = loser {
            if let (Some(from), Some(to)) = (self.ring(&loser.vault), self.ring(&winner.1.vault)) {
                staged.extend(self.move_vault_notes(project, &loser, &from, &winner.1, &to)?);
            }
        }
        let paths: Vec<&str> = staged.iter().map(String::as_str).collect();
        Self::stage_paths(git, &project.root, neutral, deadline, &paths)
    }

    /// Re-encrypts every note that opens in vault `from` into vault `to` (same id). Returns the
    /// repository paths written.
    fn move_vault_notes(
        &self,
        project: &Project,
        from_vault: &Vault,
        from: &KeyRing,
        to_vault: &Vault,
        to: &KeyRing,
    ) -> Result<Vec<String>, ProjectNotesError> {
        let _guard = lock(&self.state);
        let mut written = Vec::new();
        for (id, path) in Self::note_files(&project.dir)? {
            let Ok(Some(bytes)) = read_file(&path) else {
                continue;
            };
            let Ok(plain) = crypto::open_note(from, &from_vault.vault, &id, &bytes) else {
                continue;
            };
            let sealed = crypto::seal_note(to, &to_vault.vault, &id, &plain, &mut OsEntropy)?;
            write_atomic(&path, sealed.as_bytes()).map_err(io_error(&path))?;
            written.push(format!("{NOTES_PATH}/{id}{NOTE_EXTENSION}"));
        }
        Ok(written)
    }

    /// First step of turning shared notes on: a new vault, key ring and recovery code, kept in
    /// memory until [`Self::enable`]. Returns the code to show once. A setup already waiting
    /// keeps its code: a second dialog (or a development build running effects twice) must
    /// never show a code that is not the one `enable` seals.
    pub fn prepare(&self, folder: &NoteFolder) -> Result<Zeroizing<String>, ProjectNotesError> {
        let project = self.project(folder)?;
        if Self::read_vault(&project)?.is_some() {
            return Err(ProjectNotesError::AlreadyOn);
        }
        let mut state = lock(&self.state);
        state.pending.retain(|_, pending| !pending.expired());
        if let Some(pending) = state.pending.get(&project.dir) {
            return Ok(pending.code.reveal());
        }
        let pending = Pending {
            vault: VaultId::generate(&mut OsEntropy).map_err(NotesCryptoError::from)?,
            ring: KeyRing::generate(&mut OsEntropy).map_err(NotesCryptoError::from)?,
            code: NotesRecoveryCode::generate(&mut OsEntropy).map_err(NotesCryptoError::from)?,
            created: Instant::now(),
        };
        let code = pending.code.reveal();
        state.pending.insert(project.dir, pending);
        Ok(code)
    }

    /// The prepared code, for saving it as a file.
    pub fn pending_code(
        &self,
        folder: &NoteFolder,
    ) -> Result<Zeroizing<String>, ProjectNotesError> {
        let project = self.project(folder)?;
        lock(&self.state)
            .pending
            .get(&project.dir)
            .filter(|pending| !pending.expired())
            .map(|pending| pending.code.reveal())
            .ok_or(ProjectNotesError::NoSetup)
    }

    /// The setup dialog closed without turning notes on: the prepared key and code are dropped.
    pub fn cancel(&self, folder: &NoteFolder) -> Result<(), ProjectNotesError> {
        let project = self.project(folder)?;
        lock(&self.state).pending.remove(&project.dir);
        Ok(())
    }

    /// The project root, for checking where the recovery code may be saved.
    pub fn root(&self, folder: &NoteFolder) -> Result<PathBuf, ProjectNotesError> {
        Ok(self.project(folder)?.root)
    }

    /// Second step, after "I saved it": the key ring goes into the keychain, then the files are
    /// written. Nothing is staged or committed.
    pub fn enable(
        &self,
        folder: &NoteFolder,
        label: &str,
        untitled: &str,
    ) -> Result<SharedStatus, ProjectNotesError> {
        let project = self.project(folder)?;
        let pending = lock(&self.state)
            .pending
            .remove(&project.dir)
            .filter(|pending| !pending.expired())
            .ok_or(ProjectNotesError::NoSetup)?;
        // Until a file is written, a failure keeps the setup, so the dialog can try again with
        // the code the person already saved.
        let keep = |pending: Pending, error: ProjectNotesError| {
            lock(&self.state)
                .pending
                .insert(project.dir.clone(), pending);
            Err(error)
        };
        match Self::read_vault(&project) {
            Ok(None) => {}
            Ok(Some(_)) => return Err(ProjectNotesError::AlreadyOn),
            Err(error) => return keep(pending, error),
        }
        let code_slot = match crypto::seal_code_slot(
            &pending.ring,
            &pending.vault,
            &pending.code,
            &mut OsEntropy,
        ) {
            Ok(slot) => slot,
            Err(error) => return keep(pending, error.into()),
        };
        let vault = Vault {
            vault: pending.vault,
            mode: VaultMode::Solo { code_slot },
        };
        if let Err(error) = self.remember(&pending.vault, &pending.ring) {
            return keep(pending, error);
        }
        {
            let _guard = lock(&self.state);
            fs::create_dir_all(&project.dir).map_err(io_error(&project.dir))?;
            check_notes_dir(&project.root)?;
            for (name, content) in [(ATTRIBUTES_FILE, ATTRIBUTES), (README_FILE, README)] {
                let path = project.dir.join(name);
                if !path.exists() {
                    write_atomic(&path, content.as_bytes()).map_err(io_error(&path))?;
                }
            }
            let path = project.dir.join(VAULT_FILE);
            // create_new: another Hangar that turned notes on meanwhile keeps its vault.
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|error| match error.kind() {
                    io::ErrorKind::AlreadyExists => ProjectNotesError::AlreadyOn,
                    _ => io_error(&path)(error),
                })?;
            use std::io::Write as _;
            file.write_all(vault_json(&vault).as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(io_error(&path))?;
        }
        self.status(folder, label, untitled)
    }

    /// Unlocks a solo vault with its notes recovery code; the key ring is kept in the keychain.
    pub fn unlock(
        &self,
        folder: &NoteFolder,
        code: &str,
        label: &str,
        untitled: &str,
    ) -> Result<Unlock, ProjectNotesError> {
        let code = match NotesRecoveryCode::parse(code) {
            Ok(code) => code,
            Err(RecoveryError::Version(1)) => return Ok(Unlock::TeamCode),
            Err(RecoveryError::Checksum) => return Ok(Unlock::Typo),
            Err(_) => return Ok(Unlock::WrongCode),
        };
        let project = self.project(folder)?;
        let vault = match Self::read_vault(&project)? {
            None => return Err(ProjectNotesError::NotOn),
            Some(VaultFile::Newer) => return Err(ProjectNotesError::NewerFormat),
            Some(VaultFile::Damaged) => return Err(ProjectNotesError::Damaged),
            Some(VaultFile::Known(vault)) => vault,
        };
        let VaultMode::Solo { code_slot } = &vault.mode else {
            return Err(ProjectNotesError::TeamVault);
        };
        let ring = match crypto::open_code_slot(code_slot, &vault.vault, &code) {
            Ok(ring) => ring,
            Err(NotesCryptoError::NewerFormat(_)) => return Err(ProjectNotesError::NewerFormat),
            Err(_) => return Ok(Unlock::WrongCode),
        };
        if let Err(error) = self.remember(&vault.vault, &ring) {
            // Unlocked for this run anyway, the next start asks for the code again. Keys already
            // cached stay: the code's ring joins them by the same rules as in the keychain.
            eprintln!("hangar: notes key not stored in the keychain: {error}");
            let mut state = lock(&self.state);
            match state.rings.get_mut(&vault.vault) {
                Some(cached) => {
                    cached.merge(&ring)?;
                }
                None => {
                    state.rings.insert(vault.vault, ring);
                }
            }
        }
        Ok(Unlock::Unlocked {
            status: self.status(folder, label, untitled)?,
        })
    }

    // ----- Notes ---------------------------------------------------------------------------

    /// `(id, path)` of every note file, sorted by id. Caller holds the lock.
    fn note_files(dir: &Path) -> Result<Vec<(String, PathBuf)>, ProjectNotesError> {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(io_error(dir)(error)),
        };
        let mut files: Vec<(String, PathBuf)> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let id = note_id_of(&name)?.to_owned();
                Some((id, e.path()))
            })
            .collect();
        // Sorted first, so the cap always keeps the same files.
        files.sort();
        files.truncate(MAX_NOTES);
        Ok(files)
    }

    fn decrypt(open: &Open, id: &str, bytes: &[u8]) -> Result<Plain, ProjectNotesError> {
        let plain = crypto::open_note(&open.ring, &open.vault.vault, id, bytes).map_err(
            |error| match error {
                NotesCryptoError::NewerFormat(_) => ProjectNotesError::NewerFormat,
                _ => ProjectNotesError::Unreadable,
            },
        )?;
        decode_plain(&plain, &open.untitled)
    }

    /// Readable notes with their metadata, and the number of files that do not open. `cache`
    /// is the decrypted metadata of [`State`] (a file that did not open is remembered as such
    /// until it changes); the caller holds the lock. One scan reads at most
    /// [`MAX_SCAN_BYTES`], and a file whose header names a format or key this computer cannot
    /// use is not read further.
    fn scan(
        open: &Open,
        cache: &mut MetaCache,
    ) -> Result<(Vec<(String, NoteMeta)>, usize), ProjectNotesError> {
        let mut notes = Vec::new();
        let mut unreadable = 0;
        let mut budget = MAX_SCAN_BYTES;
        let context = cache_context(&open.vault.vault, &open.ring);
        for (id, path) in Self::note_files(&open.project.dir)? {
            let Some((modified, len)) = stamp(&path) else {
                continue;
            };
            let cached = cache
                .get(&path)
                .filter(|c| c.modified == modified && c.len == len && c.context == context)
                .map(|c| c.meta.clone());
            let meta = match cached {
                Some(meta) => meta,
                None if len > budget => {
                    unreadable += 1;
                    continue;
                }
                None => {
                    budget -= len;
                    let meta = Self::header_usable(open, &path)
                        .then(|| read_file(&path).ok().flatten())
                        .flatten()
                        .and_then(|bytes| Self::decrypt(open, &id, &bytes).ok())
                        .map(|plain| plain.meta);
                    cache.insert(
                        path.clone(),
                        CachedMeta {
                            modified,
                            len,
                            context: context.clone(),
                            meta: meta.clone(),
                        },
                    );
                    meta
                }
            };
            match meta {
                Some(meta) => notes.push((id, meta)),
                None => unreadable += 1,
            }
        }
        Ok((notes, unreadable))
    }

    /// The first line of a note file names format 1 and a key of the ring.
    fn header_usable(open: &Open, path: &Path) -> bool {
        use std::io::{BufRead as _, Read as _};
        let Ok(file) = fs::File::open(path) else {
            return false;
        };
        let mut line = Vec::new();
        if std::io::BufReader::new(file.take(128))
            .read_until(b'\n', &mut line)
            .is_err()
        {
            return false;
        }
        // `note_header` wants two lines; the second is checked when the whole file is read.
        line.extend_from_slice(b"A\n");
        crypto::note_header(&line).is_ok_and(|header| open.ring.get(header.key).is_some())
    }

    fn read_plain(open: &Open, id: &str) -> Result<Plain, ProjectNotesError> {
        let id = Self::check_id(id)?;
        let path = note_file(&open.project.dir, &id);
        let bytes = read_file(&path)?.ok_or(ProjectNotesError::UnknownNote)?;
        Self::decrypt(open, &id, &bytes)
    }

    fn write_plain(&self, open: &Open, id: &str, plain: &Plain) -> Result<(), ProjectNotesError> {
        let encoded = encode_plain(plain)?;
        let sealed =
            crypto::seal_note(&open.ring, &open.vault.vault, id, &encoded, &mut OsEntropy)?;
        let path = note_file(&open.project.dir, id);
        write_atomic(&path, sealed.as_bytes()).map_err(io_error(&path))
    }

    fn check_id(id: &str) -> Result<String, ProjectNotesError> {
        let id = id.to_ascii_lowercase();
        if crypto::is_note_id(&id) {
            Ok(id)
        } else {
            Err(ProjectNotesError::UnknownNote)
        }
    }

    pub fn list_notes(&self, folder: &NoteFolder) -> Result<Vec<NoteInfo>, ProjectNotesError> {
        let open = self.open(folder)?;
        let (mut notes, _) = Self::scan(&open, &mut lock(&self.state).meta)?;
        notes.sort_by(|a, b| {
            b.1.updated
                .cmp(&a.1.updated)
                .then_with(|| a.1.title.cmp(&b.1.title))
        });
        Ok(notes
            .into_iter()
            .map(|(id, meta)| NoteInfo {
                id,
                title: meta.title,
                updated_at: Some(meta.updated),
                order: 0,
            })
            .collect())
    }

    pub fn read_note(&self, folder: &NoteFolder, id: &str) -> Result<String, ProjectNotesError> {
        let open = self.open(folder)?;
        let _guard = lock(&self.state);
        Ok(Self::read_plain(&open, id)?.body.to_string())
    }

    /// Writes only when the text changed: encryption makes every write a whole new file.
    pub fn write_note(
        &self,
        folder: &NoteFolder,
        id: &str,
        content: &str,
    ) -> Result<(), ProjectNotesError> {
        if content.len() > notes::MAX_NOTE_BYTES {
            return Err(ProjectNotesError::TooLarge {
                size: content.len(),
                max: notes::MAX_NOTE_BYTES,
            });
        }
        let open = self.open(folder)?;
        let _guard = lock(&self.state);
        let mut plain = Self::read_plain(&open, id)?;
        if plain.body.as_str() == content {
            return Ok(());
        }
        plain.body = Zeroizing::new(content.to_owned());
        plain.meta.updated = now_millis().max(plain.meta.updated);
        self.write_plain(&open, &Self::check_id(id)?, &plain)
    }

    pub fn create_note(
        &self,
        folder: &NoteFolder,
        title: &str,
    ) -> Result<String, ProjectNotesError> {
        self.create_with(folder, title, "")
    }

    fn create_with(
        &self,
        folder: &NoteFolder,
        title: &str,
        body: &str,
    ) -> Result<String, ProjectNotesError> {
        let title = notes::validate_title(title).map_err(title_error)?;
        if body.len() > notes::MAX_NOTE_BYTES {
            return Err(ProjectNotesError::TooLarge {
                size: body.len(),
                max: notes::MAX_NOTE_BYTES,
            });
        }
        let open = self.open(folder)?;
        let _guard = lock(&self.state);
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_millis();
        let plain = Plain {
            meta: NoteMeta {
                v: META_VERSION,
                title,
                created: now,
                updated: now,
                by: String::new(),
            },
            body: Zeroizing::new(body.to_owned()),
        };
        self.write_plain(&open, &id, &plain)?;
        Ok(id)
    }

    pub fn rename_note(
        &self,
        folder: &NoteFolder,
        id: &str,
        title: &str,
    ) -> Result<(), ProjectNotesError> {
        let title = notes::validate_title(title).map_err(title_error)?;
        let open = self.open(folder)?;
        let _guard = lock(&self.state);
        let mut plain = Self::read_plain(&open, id)?;
        if plain.meta.title == title {
            return Ok(());
        }
        plain.meta.title = title;
        plain.meta.updated = now_millis().max(plain.meta.updated);
        self.write_plain(&open, &Self::check_id(id)?, &plain)
    }

    fn trash_dir(&self, vault: &VaultId) -> PathBuf {
        self.trash_root.join(vault.to_hex()).join(TRASH_DIR)
    }

    /// Moves the encrypted file out of the repository into the local trash.
    pub fn delete_note(&self, folder: &NoteFolder, id: &str) -> Result<(), ProjectNotesError> {
        let open = self.open(folder)?;
        let id = Self::check_id(id)?;
        let _guard = lock(&self.state);
        let from = note_file(&open.project.dir, &id);
        let bytes = read_file(&from)?.ok_or(ProjectNotesError::UnknownNote)?;
        let trash = self.trash_dir(&open.vault.vault);
        create_private_dir(&trash).map_err(io_error(&trash))?;
        // Copy and remove: the repository and the app data usually lie on different drives.
        let to = trash.join(format!("{}-{id}{NOTE_EXTENSION}", now_millis()));
        write_atomic(&to, &bytes).map_err(io_error(&to))?;
        fs::remove_file(&from).map_err(io_error(&from))
    }

    /// `(deleted at, id, path)` of the trash files, newest first, one per note id.
    fn trash_files(trash: &Path) -> Vec<(u64, String, PathBuf)> {
        let Ok(entries) = fs::read_dir(trash) else {
            return Vec::new();
        };
        let mut files: Vec<(u64, String, PathBuf)> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let (stamp, rest) = name.split_once('-')?;
                let stamp: u64 = stamp.parse().ok()?;
                Some((stamp, note_id_of(rest)?.to_owned(), e.path()))
            })
            .collect();
        files.sort_by_key(|file| std::cmp::Reverse(file.0));
        let mut seen = std::collections::HashSet::new();
        files.retain(|(_, id, _)| seen.insert(id.clone()));
        files
    }

    pub fn list_trash(&self, folder: &NoteFolder) -> Result<Vec<TrashInfo>, ProjectNotesError> {
        let open = self.open(folder)?;
        let _guard = lock(&self.state);
        Ok(Self::trash_files(&self.trash_dir(&open.vault.vault))
            .into_iter()
            .filter_map(|(deleted_at, id, path)| {
                let bytes = read_file(&path).ok()??;
                let plain = Self::decrypt(&open, &id, &bytes).ok()?;
                Some(TrashInfo {
                    id,
                    title: plain.meta.title,
                    deleted_at: Some(deleted_at),
                })
            })
            .collect())
    }

    /// Back into the repository; under a new id if the old one is taken again.
    pub fn restore_note(&self, folder: &NoteFolder, id: &str) -> Result<String, ProjectNotesError> {
        let open = self.open(folder)?;
        let id = Self::check_id(id)?;
        let _guard = lock(&self.state);
        let (_, _, from) = Self::trash_files(&self.trash_dir(&open.vault.vault))
            .into_iter()
            .find(|(_, trashed, _)| *trashed == id)
            .ok_or(ProjectNotesError::UnknownNote)?;
        let bytes = read_file(&from)?.ok_or(ProjectNotesError::UnknownNote)?;
        fs::create_dir_all(&open.project.dir).map_err(io_error(&open.project.dir))?;
        check_notes_dir(&open.project.root)?;
        let target = note_file(&open.project.dir, &id);
        let restored = if target.exists() {
            // The id is bound into the ciphertext: a new id means a new encryption.
            let plain = Self::decrypt(&open, &id, &bytes)?;
            let fresh = uuid::Uuid::new_v4().to_string();
            self.write_plain(&open, &fresh, &plain)?;
            fresh
        } else {
            write_atomic(&target, &bytes).map_err(io_error(&target))?;
            id
        };
        fs::remove_file(&from).map_err(io_error(&from))?;
        Ok(restored)
    }

    /// Title and Markdown, for exporting and moving.
    pub fn note_for_export(
        &self,
        folder: &NoteFolder,
        id: &str,
    ) -> Result<(String, String), ProjectNotesError> {
        let open = self.open(folder)?;
        let _guard = lock(&self.state);
        let plain = Self::read_plain(&open, id)?;
        Ok((plain.meta.title, plain.body.to_string()))
    }

    /// Search in every vault unlocked in this run, decrypted in memory only.
    pub fn search(&self, query: &str) -> Result<Vec<(bool, SearchHit)>, ProjectNotesError> {
        // The private search has refused an invalid query already.
        let Some(query) = notes::validate_query(query).ok().flatten() else {
            return Ok(Vec::new());
        };
        let known: Vec<(VaultId, Project)> = lock(&self.state)
            .known
            .iter()
            .map(|(vault, project)| (*vault, project.clone()))
            .collect();
        let mut hits = Vec::new();
        // One budget for the whole search, as for a scan.
        let mut budget = MAX_SCAN_BYTES;
        for (vault_id, project) in known {
            let Some(ring) = self.ring(&vault_id) else {
                continue;
            };
            let vault = match Self::read_vault(&project) {
                Ok(Some(VaultFile::Known(vault))) if vault.vault == vault_id => vault,
                _ => continue,
            };
            let open = Open {
                vault,
                ring,
                project,
                untitled: String::new(),
            };
            let open = Open {
                untitled: self.untitled(),
                ..open
            };
            let _guard = lock(&self.state);
            for (id, path) in Self::note_files(&open.project.dir)? {
                let len = stamp(&path).map_or(0, |(_, len)| len);
                if len > budget {
                    break;
                }
                budget -= len;
                if !Self::header_usable(&open, &path) {
                    continue;
                }
                let Ok(Some(bytes)) = read_file(&path) else {
                    continue;
                };
                let Ok(plain) = Self::decrypt(&open, &id, &bytes) else {
                    continue;
                };
                let in_title = plain.meta.title.to_lowercase().contains(&query);
                if !in_title && !plain.body.to_lowercase().contains(&query) {
                    continue;
                }
                hits.push((
                    in_title,
                    SearchHit {
                        folder_key: notes::GENERAL_KEY.to_owned(),
                        path: Some(open.project.root.clone()),
                        id,
                        title: plain.meta.title.clone(),
                        excerpt: notes::excerpt(&plain.body, &query),
                        updated_at: Some(plain.meta.updated),
                        shared: true,
                    },
                ));
            }
        }
        Ok(hits)
    }
}

/// Moves a note between the private notes of a folder and its shared notes (either way):
/// created in the target with title and text, then deleted from the source (into its trash).
/// Returns the new id.
pub fn move_note(
    private: &NotesStore,
    shared: &ProjectNotes,
    from: &NoteFolder,
    id: &str,
    to: &NoteFolder,
) -> Result<String, NotesError> {
    let (title, body) = if from.shared {
        shared.note_for_export(from, id)?
    } else {
        private.note_for_export(from, id)?
    };
    let new_id = if to.shared {
        shared.create_with(to, &title, &body)?
    } else {
        let new_id = private.create_note(to, &title)?;
        private.write_note(to, &new_id, &body)?;
        new_id
    };
    if from.shared {
        shared.delete_note(from, id)?;
    } else {
        private.delete_note(from, id)?;
    }
    Ok(new_id)
}

#[cfg(test)]
mod tests;
