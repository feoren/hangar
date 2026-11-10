//! Notes per working folder (ADR 0004).
//!
//! Notes belong to a session's working folder, not to the session itself; on top of that there
//! is the General area without a folder. They are stored as real `.md` files in the app data
//! folder, never in the project:
//!
//! ```text
//! notes/
//!   general/                   the General area, without a folder
//!   <32 hex from SHA-256>/     per working folder, key from the normalized path
//!     folder.json              original path, title, order and stable IDs
//!     <title>.md               the notes, readable in Explorer as well
//!     .trash/                  deleted notes (moved, not deleted)
//!       trash.json             ID, title and time of each deleted note, for restoring
//! ```
//!
//! A note's ID is stored only in `folder.json`, so the file name can follow the title.
//! If a file is missing, its entry is dropped; a `.md` file added from outside is adopted with
//! a new ID. Contents are never logged. Permissions as for scrollback (persist.rs).

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::lock;
use crate::persist::{create_private_dir, normalize_key, read_limited, write_atomic};

pub const NOTES_DIR: &str = "notes";
pub const GENERAL_KEY: &str = "general";
const META_FILE: &str = "folder.json";
pub const TRASH_DIR: &str = ".trash";
/// Folder names before the English localization; [`NotesStore::migrate_legacy_layout`] moves them.
const LEGACY_GENERAL_KEY: &str = "allgemein";
const LEGACY_TRASH_DIR: &str = ".papierkorb";
const META_VERSION: u32 = 1;
pub const MAX_NOTE_BYTES: usize = 1024 * 1024;
const MAX_META_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_TITLE_CHARS: usize = 200;
/// Short enough that even deep profile paths on Windows stay below 260 characters.
const MAX_STEM_CHARS: usize = 80;
const FALLBACK_STEM: &str = "Note";
const TRASH_META_FILE: &str = "trash.json";
const TRASH_META_VERSION: u32 = 1;
/// At most this many hits per search; title hits first, then the newest notes.
pub const MAX_SEARCH_RESULTS: usize = 50;
pub const MAX_QUERY_CHARS: usize = 200;
/// A search reads at most this many note files, so a huge notes folder cannot stall it.
const MAX_SEARCH_FILES: usize = 5000;
/// A search reads at most this many bytes of note content in total.
const MAX_SEARCH_BYTES: usize = 64 * 1024 * 1024;
/// Characters of context before a hit in the excerpt.
const EXCERPT_CONTEXT: usize = 60;
const EXCERPT_MAX_CHARS: usize = 160;
/// Not usable as a file name on Windows, not even with an extension (`CON.md`).
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];
const FORBIDDEN_CHARS: &str = "<>:\"/\\|?*";

/// Argument `folder` of all note commands: `{ "path": "C:\\..." }` or `{ "path": null }` for
/// the General area.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoteFolder {
    #[serde(default)]
    pub path: Option<PathBuf>,
    /// The project's shared notes in the repository (project_notes.rs, ADR 0006) instead of
    /// the private ones of this folder.
    #[serde(default)]
    pub shared: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderInfo {
    pub folder_key: String,
    /// `null` for the General area.
    pub path: Option<PathBuf>,
    pub note_count: usize,
    /// Unix time in milliseconds of the newest note; `null` without notes.
    pub updated_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteInfo {
    pub id: String,
    pub title: String,
    /// Unix time in milliseconds (modification time of the file).
    pub updated_at: Option<u64>,
    pub order: u32,
}

/// A hit of [`NotesStore::search`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub folder_key: String,
    /// The scope's working folder, `null` for the General area.
    pub path: Option<PathBuf>,
    pub id: String,
    pub title: String,
    /// Text around the first hit in the content (whitespace collapsed), or the start of the
    /// content when only the title matches.
    pub excerpt: String,
    pub updated_at: Option<u64>,
    /// A shared note of the project (then `path` is the repository's main worktree).
    pub shared: bool,
}

/// A deleted note in the scope's trash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashInfo {
    pub id: String,
    pub title: String,
    /// Unix time in milliseconds of the deletion.
    pub deleted_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TrashMeta {
    version: u32,
    items: Vec<TrashEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TrashEntry {
    id: String,
    title: String,
    /// File name inside `.trash/`.
    file: String,
    deleted_at: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FolderMeta {
    version: u32,
    path: Option<PathBuf>,
    notes: Vec<NoteEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NoteEntry {
    id: String,
    title: String,
    /// Only the file name, without folder.
    file: String,
    order: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum NotesError {
    #[error("Invalid title: {0}")]
    InvalidTitle(&'static str),
    #[error("Invalid note ID: expected a UUID")]
    InvalidId,
    #[error("Note not found")]
    UnknownNote,
    #[error("Invalid folder: {0}")]
    InvalidFolder(&'static str),
    #[error("Note too large: {size} bytes, at most {max} allowed")]
    TooLarge { size: usize, max: usize },
    #[error("The note file is not UTF-8 text")]
    NotText,
    #[error("Invalid search: {0}")]
    InvalidQuery(&'static str),
    #[error(transparent)]
    Export(#[from] crate::export::ExportError),
    #[error("File {path} cannot be read or written: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Background task aborted: {0}")]
    Task(String),
    #[error(transparent)]
    Shared(#[from] crate::project_notes::ProjectNotesError),
}

impl Serialize for NotesError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> NotesError + '_ {
    move |source| NotesError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Path in comparison form: separators unified, `.` and doubled separators removed, no
/// trailing separator, `\\?\C:\` like `C:\`. On Windows also lowercased, because NTFS is
/// case-insensitive by default and dialogs, shells and user input
/// spell the same folder differently. `..` stays as is (no resolving without the
/// file system, symbolic links would make it wrong), and nothing is canonicalized either, because
/// the folder may be missing or sit on a slow network drive.
pub fn normalize_path(path: &Path) -> String {
    let windows = cfg!(windows);
    let separator = if windows { "\\" } else { "/" };
    let mut prefix = String::new();
    let mut rooted = false;
    let mut parts: Vec<String> = Vec::new();
    for component in path.components() {
        match component {
            Component::Prefix(p) => {
                prefix = match p.kind() {
                    Prefix::Disk(d) | Prefix::VerbatimDisk(d) => format!("{}:", char::from(d)),
                    Prefix::UNC(server, share) | Prefix::VerbatimUNC(server, share) => format!(
                        "\\\\{}\\{}",
                        server.to_string_lossy(),
                        share.to_string_lossy()
                    ),
                    _ => p.as_os_str().to_string_lossy().into_owned(),
                }
            }
            Component::RootDir => rooted = true,
            Component::CurDir => {}
            Component::ParentDir => parts.push("..".into()),
            Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
        }
    }
    let mut text = prefix;
    if rooted {
        text.push_str(separator);
    }
    text.push_str(&parts.join(separator));
    if windows {
        text.to_lowercase()
    } else {
        text
    }
}

/// Folder key: `general` or 32 hex digits (128 bits) from SHA-256 of the normalized
/// path. Stable across versions and machines, without path parts in the folder name.
pub fn folder_key(folder: &NoteFolder) -> Result<String, NotesError> {
    let Some(path) = &folder.path else {
        return Ok(GENERAL_KEY.to_owned());
    };
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return Err(NotesError::InvalidFolder("path must be absolute"));
    }
    let digest = Sha256::digest(normalize_path(path).as_bytes());
    Ok(digest.iter().take(16).map(|b| format!("{b:02x}")).collect())
}

pub fn is_folder_key(name: &str) -> bool {
    name == GENERAL_KEY || (name.len() == 32 && name.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Check and clean up a display title (outer whitespace removed).
pub fn validate_title(title: &str) -> Result<String, NotesError> {
    let title = title.trim();
    if title.is_empty() {
        return Err(NotesError::InvalidTitle("must not be empty"));
    }
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(NotesError::InvalidTitle("too long"));
    }
    if title.chars().any(char::is_control) {
        return Err(NotesError::InvalidTitle("contains control characters"));
    }
    Ok(title.to_owned())
}

/// File name (without `.md`) from a title, safe on all platforms: no path parts and no
/// forbidden characters, no dot or space at the end (Windows strips them), no
/// dot at the start (hidden, and `.trash` stays unambiguous), no reserved
/// device names, limited length.
pub fn file_stem(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .map(|c| {
            if c.is_control() || FORBIDDEN_CHARS.contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(MAX_STEM_CHARS)
        .collect();
    let mut stem = cleaned.trim_start().trim_end_matches(['.', ' ']).to_owned();
    if stem.starts_with('.') {
        stem.insert(0, '_');
    }
    if stem.is_empty() {
        return FALLBACK_STEM.to_owned();
    }
    let device = stem.split('.').next().unwrap_or_default().trim_end();
    if RESERVED_NAMES
        .iter()
        .any(|r| r.eq_ignore_ascii_case(device))
    {
        stem.insert(0, '_');
    }
    stem
}

/// A plain `.md` file name: a single path component, no folder, no `..`. Metadata on disk
/// (`folder.json`, `trash.json`) is a file like any other; an entry naming anything else must
/// never make the store read, rename or write outside its folder.
fn plain_markdown_name(name: &str) -> bool {
    let mut parts = Path::new(name).components();
    let single = matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(part)), None) if part == name
    );
    single && !name.contains(['/', '\\']) && is_markdown(name)
}

/// Reads at most `max` bytes from an open file instead of trusting a size read earlier.
/// `Ok(None)`: the file is larger than `max`. Only regular files: opening a FIFO or device
/// named like a note could block or never end.
fn read_capped(path: &Path, max: usize) -> io::Result<Option<Vec<u8>>> {
    use std::io::Read as _;
    if !fs::metadata(path)?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    file.take(limit).read_to_end(&mut bytes)?;
    Ok((bytes.len() <= max).then_some(bytes))
}

fn is_markdown(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("md"))
}

fn millis(time: SystemTime) -> Option<u64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
}

fn modified(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(millis)
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn check_id(id: &str) -> Result<String, NotesError> {
    normalize_key(id).map_err(|_| NotesError::InvalidId)
}

/// Storage of all notes. A lock just for notes keeps reading, changing and writing
/// `folder.json` together; it is independent of the session lock.
#[derive(Debug, Clone)]
pub struct NotesStore {
    root: PathBuf,
    guard: Arc<Mutex<()>>,
}

struct Folder {
    dir: PathBuf,
    meta: FolderMeta,
}

impl Folder {
    fn entry(&self, id: &str) -> Result<&NoteEntry, NotesError> {
        let id = check_id(id)?;
        self.meta
            .notes
            .iter()
            .find(|n| n.id == id)
            .ok_or(NotesError::UnknownNote)
    }

    fn file(&self, entry: &NoteEntry) -> PathBuf {
        self.dir.join(&entry.file)
    }

    /// Free file name for `title`; `except` is the note's own name when renaming.
    fn free_file_name(&self, title: &str, except: Option<&str>) -> String {
        let stem = file_stem(title);
        let taken: HashSet<String> = self
            .meta
            .notes
            .iter()
            .map(|n| n.file.to_lowercase())
            .filter(|f| except.is_none_or(|e| e.to_lowercase() != *f))
            .collect();
        let is_free = |name: &str| {
            let lower = name.to_lowercase();
            if except.is_some_and(|e| e.to_lowercase() == lower) {
                return true;
            }
            !taken.contains(&lower) && !self.dir.join(name).exists()
        };
        let mut candidate = format!("{stem}.md");
        let mut n = 2;
        while !is_free(&candidate) {
            candidate = format!("{stem} ({n}).md");
            n += 1;
        }
        candidate
    }

    fn next_order(&self) -> u32 {
        self.meta
            .notes
            .iter()
            .map(|n| n.order.saturating_add(1))
            .max()
            .unwrap_or(0)
    }

    fn save(&self) -> Result<(), NotesError> {
        let path = self.dir.join(META_FILE);
        let json = serde_json::to_vec_pretty(&self.meta)
            .map_err(|e| io_error(&path)(io::Error::new(io::ErrorKind::InvalidData, e)))?;
        write_atomic(&path, &json).map_err(io_error(&path))
    }

    /// Sync with the folder: missing files are dropped, foreign `.md` files are added.
    fn sync(&mut self) -> Result<bool, NotesError> {
        let before = self.meta.notes.len();
        let dir = self.dir.clone();
        self.meta.notes.retain(|n| dir.join(&n.file).is_file());
        let mut changed = self.meta.notes.len() != before;
        let known: HashSet<String> = self
            .meta
            .notes
            .iter()
            .map(|n| n.file.to_lowercase())
            .collect();
        let mut found: Vec<String> = Vec::new();
        for entry in fs::read_dir(&self.dir).map_err(io_error(&self.dir))? {
            let Ok(entry) = entry else { continue };
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            let is_file = entry.file_type().is_ok_and(|t| t.is_file());
            if is_file && is_markdown(&name) && !known.contains(&name.to_lowercase()) {
                found.push(name);
            }
        }
        found.sort();
        for name in found {
            let stem = Path::new(&name)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let title = validate_title(&stem).unwrap_or_else(|_| FALLBACK_STEM.to_owned());
            let order = self.next_order();
            self.meta.notes.push(NoteEntry {
                id: new_id(),
                title,
                file: name,
                order,
            });
            changed = true;
        }
        Ok(changed)
    }
}

fn log_migration_error(path: &Path, error: &io::Error) {
    eprintln!(
        "hangar: notes in {} were not moved to the new folder names: {error}",
        path.display()
    );
}

/// `name` in `dir` if free, otherwise `stem (2).ext`, `stem (3).ext` and so on.
fn free_name(dir: &Path, name: &str) -> String {
    if !dir.join(name).exists() {
        return name.to_owned();
    }
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map_or_else(|| name.to_owned(), |s| s.to_string_lossy().into_owned());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (2u32..)
        .map(|n| format!("{stem} ({n}){ext}"))
        .find(|candidate| !dir.join(candidate).exists())
        .unwrap_or_else(|| name.to_owned())
}

/// Moves the entries of `old` into the existing folder `new` without overwriting anything.
/// Subfolders are merged the same way. Returns the moved files as (old name, new name). Names
/// that are not valid UTF-8 and the names in `skip` stay in `old`.
fn merge_dir(old: &Path, new: &Path, skip: &[&str]) -> io::Result<Vec<(String, String)>> {
    let mut names: Vec<String> = fs::read_dir(old)?
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| !skip.contains(&n.as_str()))
        .collect();
    names.sort();
    let mut moved = Vec::new();
    for name in names {
        let from = old.join(&name);
        // Without following links: a linked folder moves as the link, never with its target.
        if fs::symlink_metadata(&from)?.is_dir() {
            move_dir(&from, &new.join(&name))?;
            continue;
        }
        let target = free_name(new, &name);
        fs::rename(&from, new.join(&target))?;
        moved.push((name, target));
    }
    Ok(moved)
}

/// Renames `old` to `new`, or merges it into `new` if that already exists. Missing `old` is
/// not an error. `old` is removed only once it is empty; `remove_dir` never deletes contents.
fn move_dir(old: &Path, new: &Path) -> io::Result<()> {
    if !old.is_dir() {
        return Ok(());
    }
    if !new.exists() {
        return fs::rename(old, new);
    }
    merge_dir(old, new, &[])?;
    let _ = fs::remove_dir(old);
    Ok(())
}

/// Like [`move_dir`], but on a merge the entries of the old `folder.json` move along, so the
/// moved notes keep their IDs, titles and relative order. An unreadable old `folder.json`, or
/// an unreadable one in `new`, stays untouched; the moved `.md` files are then adopted with a
/// new ID on the next sync, as for files added from outside.
fn migrate_general(old: &Path, new: &Path) -> io::Result<()> {
    if !old.is_dir() {
        return Ok(());
    }
    if !new.exists() {
        return fs::rename(old, new);
    }
    let moved = merge_dir(old, new, &[META_FILE])?;
    let old_meta_path = old.join(META_FILE);
    let new_meta_path = new.join(META_FILE);
    let old_meta = NotesStore::read_meta(old);
    let new_meta = if new_meta_path.exists() {
        NotesStore::read_meta(new)
    } else {
        Some(FolderMeta {
            version: META_VERSION,
            path: None,
            notes: Vec::new(),
        })
    };
    if let (Some(mut old_meta), Some(mut meta)) = (old_meta, new_meta) {
        let mut ids: HashSet<String> = meta.notes.iter().map(|n| n.id.clone()).collect();
        let mut order = meta
            .notes
            .iter()
            .map(|n| n.order.saturating_add(1))
            .max()
            .unwrap_or(0);
        old_meta.notes.sort_by_key(|n| n.order);
        for entry in old_meta.notes {
            let Some((_, file)) = moved.iter().find(|(from, _)| *from == entry.file) else {
                continue;
            };
            let id = if ids.insert(entry.id.clone()) {
                entry.id
            } else {
                let id = new_id();
                ids.insert(id.clone());
                id
            };
            meta.notes.push(NoteEntry {
                id,
                title: entry.title,
                file: file.clone(),
                order,
            });
            order = order.saturating_add(1);
        }
        let json = serde_json::to_vec_pretty(&meta)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        write_atomic(&new_meta_path, &json)?;
        // Everything it described is now in the new folder.json.
        fs::remove_file(&old_meta_path)?;
    }
    let _ = fs::remove_dir(old);
    Ok(())
}

/// Search text: trimmed, not longer than [`MAX_QUERY_CHARS`], no control characters. `None`
/// for an empty query, which finds nothing.
pub fn validate_query(query: &str) -> Result<Option<String>, NotesError> {
    let query = query.trim();
    if query.is_empty() {
        return Ok(None);
    }
    if query.chars().count() > MAX_QUERY_CHARS {
        return Err(NotesError::InvalidQuery("too long"));
    }
    if query.chars().any(char::is_control) {
        return Err(NotesError::InvalidQuery("contains control characters"));
    }
    Ok(Some(query.to_lowercase()))
}

/// Byte offset in `text` of the character at which `lower_pos` (an offset in
/// `text.to_lowercase()`) starts. Lowercasing can change byte lengths, so offsets differ.
fn original_offset(text: &str, lower_pos: usize) -> usize {
    let mut lowered = 0;
    for (index, c) in text.char_indices() {
        if lowered >= lower_pos {
            return index;
        }
        lowered += c.to_lowercase().map(char::len_utf8).sum::<usize>();
    }
    text.len()
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Up to [`EXCERPT_MAX_CHARS`] characters around the first match of `query_lower` in
/// `content`, whitespace collapsed; `…` marks a cut. Without a match the start of the content.
pub fn excerpt(content: &str, query_lower: &str) -> String {
    let lower = content.to_lowercase();
    let start = lower.find(query_lower).map_or(0, |pos| {
        let at = original_offset(content, pos);
        content[..at]
            .char_indices()
            .rev()
            .nth(EXCERPT_CONTEXT.saturating_sub(1))
            .map_or(0, |(index, _)| index)
    });
    let window: String = content[start..]
        .chars()
        .take(EXCERPT_MAX_CHARS * 2)
        .collect();
    let mut text: String = collapse_whitespace(&window)
        .chars()
        .take(EXCERPT_MAX_CHARS)
        .collect();
    let cut_end = content[start..].chars().count() > EXCERPT_MAX_CHARS
        && collapse_whitespace(&content[start..]).chars().count() > EXCERPT_MAX_CHARS;
    if cut_end {
        text.push('…');
    }
    if start > 0 && !content[..start].trim().is_empty() {
        text.insert(0, '…');
    }
    text
}

/// The private hits of [`NotesStore::search`] together with shared ones (`(title hit, hit)`), in
/// the same order: title hits first, then the newest, at most [`MAX_SEARCH_RESULTS`].
pub fn merge_hits(
    query: &str,
    private: Vec<SearchHit>,
    shared: Vec<(bool, SearchHit)>,
) -> Vec<SearchHit> {
    if shared.is_empty() {
        return private;
    }
    let query = validate_query(query).ok().flatten().unwrap_or_default();
    let mut hits: Vec<(bool, SearchHit)> = private
        .into_iter()
        .map(|hit| (hit.title.to_lowercase().contains(&query), hit))
        .chain(shared)
        .collect();
    hits.sort_by(|(a_title, a), (b_title, b)| {
        b_title
            .cmp(a_title)
            .then_with(|| b.updated_at.cmp(&a.updated_at))
    });
    hits.into_iter()
        .take(MAX_SEARCH_RESULTS)
        .map(|(_, hit)| hit)
        .collect()
}

/// Title of a trash file without a record: `<millis>-<file>.md` back to `<file>`.
fn trash_title(file: &str) -> (String, Option<u64>) {
    let stem = Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (stamp, rest) = match stem.split_once('-') {
        Some((digits, rest))
            if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) =>
        {
            (digits.parse().ok(), rest.to_owned())
        }
        _ => (None, stem.clone()),
    };
    let title = validate_title(&rest).unwrap_or_else(|_| FALLBACK_STEM.to_owned());
    (title, stamp)
}

impl NotesStore {
    /// `root` is the app data folder; the notes live below it in `notes/`.
    pub fn new(app_dir: &Path) -> Self {
        Self {
            root: app_dir.join(NOTES_DIR),
            guard: Arc::default(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn read_meta(dir: &Path) -> Option<FolderMeta> {
        let path = dir.join(META_FILE);
        let bytes = match read_limited(&path, MAX_META_BYTES) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return None,
            Err(reason) => {
                eprintln!("hangar: ignoring {}: {reason}", path.display());
                return None;
            }
        };
        match serde_json::from_slice::<FolderMeta>(&bytes) {
            Ok(mut meta) if meta.version == META_VERSION => {
                // An entry pointing elsewhere is dropped; a plain `.md` file of the folder comes
                // back on the next sync under a new ID.
                meta.notes.retain(|n| {
                    plain_markdown_name(&n.file) && check_id(&n.id).is_ok_and(|id| id == n.id)
                });
                Some(meta)
            }
            Ok(_) => {
                eprintln!("hangar: {} has an unknown version", path.display());
                None
            }
            Err(error) => {
                eprintln!(
                    "hangar: ignoring {}: JSON error ({:?}) on line {}",
                    path.display(),
                    error.classify(),
                    error.line()
                );
                None
            }
        }
    }

    /// Opens the scope. With `create` it is created; otherwise the result is `None` if it is missing.
    /// An unusable `folder.json` is set aside and rebuilt from the files.
    fn open(&self, folder: &NoteFolder, create: bool) -> Result<Option<Folder>, NotesError> {
        let dir = self.root.join(folder_key(folder)?);
        if !dir.is_dir() {
            if !create {
                return Ok(None);
            }
            create_private_dir(&dir).map_err(io_error(&dir))?;
        }
        let meta_path = dir.join(META_FILE);
        let (meta, mut changed) = match Self::read_meta(&dir) {
            Some(meta) => (meta, false),
            None => {
                if meta_path.exists() {
                    let aside = dir.join(format!(
                        "{META_FILE}.broken-{}",
                        millis(SystemTime::now()).unwrap_or(0)
                    ));
                    fs::rename(&meta_path, &aside).map_err(io_error(&meta_path))?;
                }
                let meta = FolderMeta {
                    version: META_VERSION,
                    path: folder.path.clone(),
                    notes: Vec::new(),
                };
                (meta, true)
            }
        };
        let mut opened = Folder { dir, meta };
        changed |= opened.sync()?;
        if changed {
            opened.save()?;
        }
        Ok(Some(opened))
    }

    fn opened(&self, folder: &NoteFolder) -> Result<Folder, NotesError> {
        self.open(folder, false)?.ok_or(NotesError::UnknownNote)
    }

    /// All scopes with notes; the General area always comes first, even without notes.
    pub fn list_folders(&self) -> Result<Vec<FolderInfo>, NotesError> {
        let _guard = lock(&self.guard);
        let mut general = FolderInfo {
            folder_key: GENERAL_KEY.to_owned(),
            path: None,
            note_count: 0,
            updated_at: None,
        };
        let mut others = Vec::new();
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(vec![general]),
            Err(error) => return Err(io_error(&self.root)(error)),
        };
        for entry in entries.flatten() {
            let Ok(key) = entry.file_name().into_string() else {
                continue;
            };
            let dir = entry.path();
            if !is_folder_key(&key) || !dir.is_dir() {
                continue;
            }
            // Read only: an overview must not rewrite anything. Without a readable folder.json the
            // original path is unknown; the scope shows up as soon as it is opened again.
            let Some(meta) = Self::read_meta(&dir) else {
                continue;
            };
            let files: Vec<PathBuf> = meta
                .notes
                .iter()
                .map(|n| dir.join(&n.file))
                .filter(|p| p.is_file())
                .collect();
            let info = FolderInfo {
                folder_key: key.clone(),
                path: meta.path,
                note_count: files.len(),
                updated_at: files.iter().filter_map(|p| modified(p)).max(),
            };
            if key == GENERAL_KEY {
                general = FolderInfo { path: None, ..info };
            } else if info.note_count > 0 {
                others.push(info);
            }
        }
        others.sort_by_key(|info| std::cmp::Reverse(info.updated_at));
        let mut list = vec![general];
        list.extend(others);
        Ok(list)
    }

    pub fn list_notes(&self, folder: &NoteFolder) -> Result<Vec<NoteInfo>, NotesError> {
        let _guard = lock(&self.guard);
        let Some(opened) = self.open(folder, false)? else {
            return Ok(Vec::new());
        };
        let mut notes: Vec<NoteInfo> = opened
            .meta
            .notes
            .iter()
            .map(|n| NoteInfo {
                id: n.id.clone(),
                title: n.title.clone(),
                updated_at: modified(&opened.file(n)),
                order: n.order,
            })
            .collect();
        notes.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.title.cmp(&b.title)));
        Ok(notes)
    }

    pub fn read_note(&self, folder: &NoteFolder, id: &str) -> Result<String, NotesError> {
        let _guard = lock(&self.guard);
        let opened = self.opened(folder)?;
        let path = opened.file(opened.entry(id)?);
        let Some(bytes) = read_capped(&path, MAX_NOTE_BYTES).map_err(io_error(&path))? else {
            // The size only names the problem; the content was never read past the limit.
            let size = fs::metadata(&path)
                .ok()
                .and_then(|m| usize::try_from(m.len()).ok())
                .unwrap_or(MAX_NOTE_BYTES + 1);
            return Err(NotesError::TooLarge {
                size,
                max: MAX_NOTE_BYTES,
            });
        };
        String::from_utf8(bytes).map_err(|_| NotesError::NotText)
    }

    pub fn write_note(
        &self,
        folder: &NoteFolder,
        id: &str,
        content: &str,
    ) -> Result<(), NotesError> {
        if content.len() > MAX_NOTE_BYTES {
            return Err(NotesError::TooLarge {
                size: content.len(),
                max: MAX_NOTE_BYTES,
            });
        }
        let _guard = lock(&self.guard);
        let opened = self.opened(folder)?;
        let path = opened.file(opened.entry(id)?);
        write_atomic(&path, content.as_bytes()).map_err(io_error(&path))
    }

    /// Creates an empty note and returns its ID.
    pub fn create_note(&self, folder: &NoteFolder, title: &str) -> Result<String, NotesError> {
        let title = validate_title(title)?;
        let _guard = lock(&self.guard);
        let mut opened = self
            .open(folder, true)?
            .ok_or(NotesError::InvalidFolder("cannot be created"))?;
        let file = opened.free_file_name(&title, None);
        let path = opened.dir.join(&file);
        write_atomic(&path, b"").map_err(io_error(&path))?;
        let id = new_id();
        let order = opened.next_order();
        opened.meta.notes.push(NoteEntry {
            id: id.clone(),
            title,
            file,
            order,
        });
        opened.save()?;
        Ok(id)
    }

    /// New title; the file is renamed along with it, the ID stays.
    pub fn rename_note(
        &self,
        folder: &NoteFolder,
        id: &str,
        title: &str,
    ) -> Result<(), NotesError> {
        let title = validate_title(title)?;
        let _guard = lock(&self.guard);
        let mut opened = self.opened(folder)?;
        let entry = opened.entry(id)?.clone();
        let file = opened.free_file_name(&title, Some(&entry.file));
        if file != entry.file {
            let from = opened.dir.join(&entry.file);
            fs::rename(&from, opened.dir.join(&file)).map_err(io_error(&from))?;
        }
        if let Some(note) = opened.meta.notes.iter_mut().find(|n| n.id == entry.id) {
            note.title = title;
            note.file = file;
        }
        opened.save()
    }

    /// Moves the file to `.trash/`; nothing is ever deleted for good.
    pub fn delete_note(&self, folder: &NoteFolder, id: &str) -> Result<(), NotesError> {
        let _guard = lock(&self.guard);
        let mut opened = self.opened(folder)?;
        let entry = opened.entry(id)?.clone();
        let trash = opened.dir.join(TRASH_DIR);
        create_private_dir(&trash).map_err(io_error(&trash))?;
        let stamp = millis(SystemTime::now()).unwrap_or(0);
        let from = opened.file(&entry);
        let file = free_name(&trash, &format!("{stamp}-{}", entry.file));
        let to = trash.join(&file);
        fs::rename(&from, &to).map_err(io_error(&from))?;
        opened.meta.notes.retain(|n| n.id != entry.id);
        opened.save()?;
        // Without the record the file is still adopted by `list_trash`, only under a new ID.
        let mut items = Self::read_trash(&trash);
        items.push(TrashEntry {
            id: entry.id,
            title: entry.title,
            file,
            deleted_at: Some(stamp),
        });
        if let Err(error) = Self::save_trash(&trash, items) {
            eprintln!("hangar: trash record not saved: {error}");
        }
        Ok(())
    }

    /// A record names a plain `.md` file inside `.trash/` and carries a valid ID and title.
    /// `trash.json` is a file on disk like any other: a record pointing elsewhere (`../x.md`)
    /// must never move a foreign file into the notes. A dropped record's file is adopted again
    /// by [`Self::synced_trash`] under a new ID.
    fn trusted_entry(entry: &TrashEntry) -> bool {
        plain_markdown_name(&entry.file)
            && check_id(&entry.id).is_ok_and(|id| id == entry.id)
            && validate_title(&entry.title).is_ok_and(|title| title == entry.title)
    }

    fn read_trash(trash: &Path) -> Vec<TrashEntry> {
        let path = trash.join(TRASH_META_FILE);
        match read_limited(&path, MAX_META_BYTES) {
            Ok(Some(bytes)) => match serde_json::from_slice::<TrashMeta>(&bytes) {
                Ok(meta) if meta.version == TRASH_META_VERSION => {
                    meta.items.into_iter().filter(Self::trusted_entry).collect()
                }
                _ => {
                    eprintln!("hangar: ignoring unreadable {}", path.display());
                    Vec::new()
                }
            },
            Ok(None) => Vec::new(),
            Err(reason) => {
                eprintln!("hangar: ignoring {}: {reason}", path.display());
                Vec::new()
            }
        }
    }

    fn save_trash(trash: &Path, items: Vec<TrashEntry>) -> Result<(), NotesError> {
        let path = trash.join(TRASH_META_FILE);
        let meta = TrashMeta {
            version: TRASH_META_VERSION,
            items,
        };
        let json = serde_json::to_vec_pretty(&meta)
            .map_err(|e| io_error(&path)(io::Error::new(io::ErrorKind::InvalidData, e)))?;
        write_atomic(&path, &json).map_err(io_error(&path))
    }

    /// The trash records matched with the files: records without a file are dropped, `.md`
    /// files without a record (deleted before records existed) are adopted with a new ID.
    fn synced_trash(trash: &Path) -> Result<Vec<TrashEntry>, NotesError> {
        let mut items = Self::read_trash(trash);
        let before = items.clone();
        items.retain(|item| trash.join(&item.file).is_file());
        let known: HashSet<String> = items.iter().map(|i| i.file.to_lowercase()).collect();
        let mut found: Vec<String> = fs::read_dir(trash)
            .map_err(io_error(trash))?
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| is_markdown(name) && !known.contains(&name.to_lowercase()))
            .collect();
        found.sort();
        for file in found {
            let (title, stamp) = trash_title(&file);
            let deleted_at = stamp.or_else(|| modified(&trash.join(&file)));
            items.push(TrashEntry {
                id: new_id(),
                title,
                file,
                deleted_at,
            });
        }
        if items != before {
            Self::save_trash(trash, items.clone())?;
        }
        Ok(items)
    }

    /// Deleted notes of the scope, the most recently deleted first; `[]` without a trash.
    pub fn list_trash(&self, folder: &NoteFolder) -> Result<Vec<TrashInfo>, NotesError> {
        let trash = self.root.join(folder_key(folder)?).join(TRASH_DIR);
        let _guard = lock(&self.guard);
        if !trash.is_dir() {
            return Ok(Vec::new());
        }
        let mut list: Vec<TrashInfo> = Self::synced_trash(&trash)?
            .into_iter()
            .map(|item| TrashInfo {
                id: item.id,
                title: item.title,
                deleted_at: item.deleted_at,
            })
            .collect();
        list.sort_by_key(|item| std::cmp::Reverse(item.deleted_at));
        Ok(list)
    }

    /// Moves a deleted note back to the end of the scope's list and returns its ID: the old
    /// one, unless a note with that ID exists again.
    pub fn restore_note(&self, folder: &NoteFolder, id: &str) -> Result<String, NotesError> {
        let id = check_id(id)?;
        let _guard = lock(&self.guard);
        let mut opened = self.opened(folder)?;
        let trash = opened.dir.join(TRASH_DIR);
        if !trash.is_dir() {
            return Err(NotesError::UnknownNote);
        }
        let mut items = Self::synced_trash(&trash)?;
        let index = items
            .iter()
            .position(|item| item.id == id)
            .ok_or(NotesError::UnknownNote)?;
        let item = items.remove(index);
        let file = opened.free_file_name(&item.title, None);
        let from = trash.join(&item.file);
        fs::rename(&from, opened.dir.join(&file)).map_err(io_error(&from))?;
        let restored = if opened.meta.notes.iter().any(|n| n.id == item.id) {
            new_id()
        } else {
            item.id
        };
        let order = opened.next_order();
        opened.meta.notes.push(NoteEntry {
            id: restored.clone(),
            title: item.title,
            file,
            order,
        });
        opened.save()?;
        Self::save_trash(&trash, items)?;
        Ok(restored)
    }

    /// Title and content of a note, for exporting it.
    pub fn note_for_export(
        &self,
        folder: &NoteFolder,
        id: &str,
    ) -> Result<(String, String), NotesError> {
        let content = self.read_note(folder, id)?;
        let _guard = lock(&self.guard);
        let opened = self.opened(folder)?;
        let title = opened.entry(id)?.title.clone();
        Ok((title, content))
    }

    /// Full path of the note's file, for showing it in the file manager.
    pub fn note_path(&self, folder: &NoteFolder, id: &str) -> Result<PathBuf, NotesError> {
        let _guard = lock(&self.guard);
        let opened = self.opened(folder)?;
        Ok(opened.file(opened.entry(id)?))
    }

    /// Selects the note's file in Explorer or Finder (Linux: opens its folder). The path comes
    /// from our own storage only; it starts as argv without a shell.
    pub fn reveal_note(&self, folder: &NoteFolder, id: &str) -> Result<(), NotesError> {
        let path = self.note_path(folder, id)?;
        crate::desktop::reveal(&path).map_err(io_error(&path))
    }

    /// Case-insensitive substring search in titles and contents of all scopes, read only (no
    /// folder is synced or rewritten). Files above [`MAX_NOTE_BYTES`] or not UTF-8 are skipped.
    /// Title hits come first, then by modification time, newest first.
    pub fn search(&self, query: &str) -> Result<Vec<SearchHit>, NotesError> {
        let Some(query) = validate_query(query)? else {
            return Ok(Vec::new());
        };
        // Only the metadata is read under the lock; the files are read afterwards, so a long
        // search never blocks saving a note.
        let scopes = {
            let _guard = lock(&self.guard);
            let entries = match fs::read_dir(&self.root) {
                Ok(entries) => entries,
                Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
                Err(error) => return Err(io_error(&self.root)(error)),
            };
            let mut keys: Vec<String> = entries
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|key| is_folder_key(key))
                .collect();
            keys.sort();
            keys.into_iter()
                .filter_map(|key| {
                    let dir = self.root.join(&key);
                    let meta = Self::read_meta(&dir)?;
                    let path = if key == GENERAL_KEY { None } else { meta.path };
                    Some((key, dir, path, meta.notes))
                })
                .collect::<Vec<_>>()
        };
        let mut hits: Vec<(bool, SearchHit)> = Vec::new();
        let mut files = 0usize;
        let mut budget = MAX_SEARCH_BYTES;
        'scopes: for (key, dir, path, notes) in scopes {
            for note in notes {
                if files >= MAX_SEARCH_FILES || budget == 0 {
                    break 'scopes;
                }
                files += 1;
                let file = dir.join(&note.file);
                let Ok(Some(bytes)) = read_capped(&file, MAX_NOTE_BYTES.min(budget)) else {
                    continue;
                };
                budget -= bytes.len();
                let Ok(content) = String::from_utf8(bytes) else {
                    continue;
                };
                let in_title = note.title.to_lowercase().contains(&query);
                if !in_title && !content.to_lowercase().contains(&query) {
                    continue;
                }
                hits.push((
                    in_title,
                    SearchHit {
                        folder_key: key.clone(),
                        path: path.clone(),
                        id: note.id,
                        title: note.title,
                        excerpt: excerpt(&content, &query),
                        updated_at: modified(&file),
                        shared: false,
                    },
                ));
            }
        }
        hits.sort_by(|(a_title, a), (b_title, b)| {
            b_title
                .cmp(a_title)
                .then_with(|| b.updated_at.cmp(&a.updated_at))
        });
        Ok(hits
            .into_iter()
            .take(MAX_SEARCH_RESULTS)
            .map(|(_, hit)| hit)
            .collect())
    }

    /// The given IDs move to the front in this order, all others keep their
    /// order behind them. Unknown IDs are an error.
    pub fn reorder_notes(&self, folder: &NoteFolder, ids: &[String]) -> Result<(), NotesError> {
        let ids: Vec<String> = ids
            .iter()
            .map(|id| check_id(id))
            .collect::<Result<_, _>>()?;
        let _guard = lock(&self.guard);
        let mut opened = self.opened(folder)?;
        if ids
            .iter()
            .any(|id| opened.meta.notes.iter().all(|n| &n.id != id))
        {
            return Err(NotesError::UnknownNote);
        }
        let mut rest: Vec<&mut NoteEntry> = Vec::new();
        let mut position = 0u32;
        let mut ordered: Vec<(usize, &mut NoteEntry)> = Vec::new();
        for note in opened.meta.notes.iter_mut() {
            match ids.iter().position(|id| *id == note.id) {
                Some(index) => ordered.push((index, note)),
                None => rest.push(note),
            }
        }
        ordered.sort_by_key(|(index, _)| *index);
        rest.sort_by_key(|n| n.order);
        for note in ordered.into_iter().map(|(_, n)| n).chain(rest) {
            note.order = position;
            position += 1;
        }
        opened.save()
    }

    /// Folder of the scope (created if needed), for example to open it in the file manager.
    pub fn folder_dir(&self, folder: &NoteFolder) -> Result<PathBuf, NotesError> {
        let _guard = lock(&self.guard);
        let opened = self
            .open(folder, true)?
            .ok_or(NotesError::InvalidFolder("cannot be created"))?;
        Ok(opened.dir)
    }

    /// One-time move from the German folder names (`allgemein/`, `.papierkorb/`) to the
    /// current ones. Runs at startup and is idempotent. Every step is a rename within the notes
    /// folder, nothing is overwritten, and only folders that are empty afterwards are removed. If
    /// both names exist, the contents are merged; a file whose name is taken keeps both versions,
    /// the moved one gets a suffix (the file already there stays, because `folder.json` refers to
    /// it by name). Errors are logged and never stop the app: whatever could not be moved stays in
    /// the old folder.
    pub fn migrate_legacy_layout(&self) {
        let _guard = lock(&self.guard);
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        let dirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .is_some_and(|n| is_folder_key(n) || n == LEGACY_GENERAL_KEY)
            })
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        // Trash first, so that merging the General area below already finds `.trash` on both sides.
        for dir in dirs {
            let old = dir.join(LEGACY_TRASH_DIR);
            if let Err(error) = move_dir(&old, &dir.join(TRASH_DIR)) {
                log_migration_error(&old, &error);
            }
        }
        let old = self.root.join(LEGACY_GENERAL_KEY);
        if let Err(error) = migrate_general(&old, &self.root.join(GENERAL_KEY)) {
            log_migration_error(&old, &error);
        }
    }

    /// Opens the folder in the system's file manager. The path only ever comes from our own
    /// storage, never directly from the frontend; it starts as argv without a shell.
    pub fn reveal(&self, folder: &NoteFolder) -> Result<(), NotesError> {
        let dir = self.folder_dir(folder)?;
        crate::desktop::open(&dir, crate::desktop::Target::Folder).map_err(io_error(&dir))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempNotes(NotesStore, PathBuf);

    impl TempNotes {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("hangar-notes-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            Self(NotesStore::new(&dir), dir)
        }
    }

    impl Drop for TempNotes {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.1);
        }
    }

    fn project() -> NoteFolder {
        let path = if cfg!(windows) {
            r"C:\Users\Test User\Project"
        } else {
            "/home/test/Project"
        };
        NoteFolder {
            path: Some(PathBuf::from(path)),
            ..NoteFolder::default()
        }
    }

    fn general() -> NoteFolder {
        NoteFolder {
            path: None,
            ..NoteFolder::default()
        }
    }

    #[test]
    fn folder_key_is_stable_and_normalized() {
        assert_eq!(folder_key(&general()).unwrap(), GENERAL_KEY);
        let key = folder_key(&project()).unwrap();
        assert_eq!(key.len(), 32);
        assert!(key.bytes().all(|b| b.is_ascii_hexdigit()));
        let same = |p: &str| {
            folder_key(&NoteFolder {
                path: Some(p.into()),
                ..NoteFolder::default()
            })
            .unwrap()
        };
        if cfg!(windows) {
            let base = same(r"C:\Users\Test User\Project");
            for variant in [
                r"c:\users\test user\project",
                r"C:\Users\Test User\Project\",
                "C:/Users/Test User/Project",
                r"C:\Users\.\Test User\\Project",
                r"\\?\C:\Users\Test User\Project",
            ] {
                assert_eq!(same(variant), base, "{variant}");
            }
            assert_ne!(same(r"C:\Users\Test User\Project2"), base);
            assert_ne!(same(r"D:\Users\Test User\Project"), base);
            assert_eq!(same(r"\\Server\Share\x"), same(r"\\?\UNC\server\share\X"));
        } else {
            let base = same("/home/test/Project");
            assert_eq!(same("/home/test/Project/"), base);
            assert_eq!(same("/home//test/./Project"), base);
            // Unix is case-sensitive.
            assert_ne!(same("/home/test/project"), base);
        }
        // Known value: the key must never change between versions.
        let fixed = if cfg!(windows) { r"C:\x" } else { "/x" };
        let expected: String = Sha256::digest(normalize_path(Path::new(fixed)).as_bytes())
            .iter()
            .take(16)
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(same(fixed), expected);
        assert_eq!(
            normalize_path(Path::new(fixed)),
            if cfg!(windows) { r"c:\x" } else { "/x" }
        );
        for bad in ["", "relative\\folder", "relative/folder"] {
            assert!(matches!(
                folder_key(&NoteFolder {
                    path: Some(bad.into()),
                    ..NoteFolder::default()
                }),
                Err(NotesError::InvalidFolder(_))
            ));
        }
    }

    #[test]
    fn titles_become_safe_file_names() {
        assert_eq!(file_stem("Ideas"), "Ideas");
        assert_eq!(file_stem("a/b\\c:d*e?f\"g<h>i|j"), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(file_stem("../../etc/passwd"), "_.._.._etc_passwd");
        assert_eq!(file_stem("..."), FALLBACK_STEM);
        assert_eq!(file_stem("End. . "), "End");
        assert_eq!(file_stem(".hidden"), "_.hidden");
        assert_eq!(file_stem("CON"), "_CON");
        assert_eq!(file_stem("nul.txt"), "_nul.txt");
        assert_eq!(file_stem("Com1 "), "_Com1");
        assert_eq!(file_stem("CONSOLE"), "CONSOLE");
        assert_eq!(file_stem("line\ttwo"), "line_two");
        assert_eq!(file_stem(&"ä".repeat(300)).chars().count(), MAX_STEM_CHARS);
        assert_eq!(file_stem("Café äöü 😀"), "Café äöü 😀");
        assert!(validate_title("  ").is_err());
        assert!(validate_title("a\nb").is_err());
        assert!(validate_title(&"x".repeat(MAX_TITLE_CHARS + 1)).is_err());
        assert_eq!(validate_title("  Title ").unwrap(), "Title");
    }

    #[test]
    fn create_write_read_rename_delete() {
        let t = TempNotes::new("crud");
        let folder = project();
        assert!(t.0.list_notes(&folder).unwrap().is_empty());
        let id = t.0.create_note(&folder, "Ideas").unwrap();
        assert!(normalize_key(&id).is_ok());
        let text = "# Ideas\n\n- äöü 😀\r\n";
        t.0.write_note(&folder, &id, text).unwrap();
        assert_eq!(t.0.read_note(&folder, &id).unwrap(), text);

        let dir = t.0.root().join(folder_key(&folder).unwrap());
        assert_eq!(fs::read_to_string(dir.join("Ideas.md")).unwrap(), text);

        // The same title gets its own file name, even with different casing.
        let second = t.0.create_note(&folder, "ideas").unwrap();
        assert!(dir.join("ideas (2).md").is_file());

        t.0.rename_note(&folder, &id, "Plan: Q4/2026").unwrap();
        assert!(!dir.join("Ideas.md").exists());
        assert!(dir.join("Plan_ Q4_2026.md").is_file());
        assert_eq!(t.0.read_note(&folder, &id).unwrap(), text);
        // Changing only the case works on Windows as well.
        t.0.rename_note(&folder, &second, "IDEAS (2)").unwrap();

        let notes = t.0.list_notes(&folder).unwrap();
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].id, id);
        assert_eq!(notes[0].title, "Plan: Q4/2026");
        assert_eq!(notes[1].title, "IDEAS (2)");
        assert!(notes[0].updated_at.is_some());

        t.0.delete_note(&folder, &id).unwrap();
        assert!(matches!(
            t.0.read_note(&folder, &id),
            Err(NotesError::UnknownNote)
        ));
        let trash: Vec<_> = fs::read_dir(dir.join(TRASH_DIR))
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .filter(|name| name != TRASH_META_FILE)
            .collect();
        assert_eq!(trash.len(), 1);
        assert!(trash[0].ends_with("-Plan_ Q4_2026.md"));
        // The trash does not show up as a note.
        assert_eq!(t.0.list_notes(&folder).unwrap().len(), 1);
    }

    #[test]
    fn limits_and_errors() {
        let t = TempNotes::new("limits");
        let folder = general();
        let id = t.0.create_note(&folder, "a").unwrap();
        let max = "x".repeat(MAX_NOTE_BYTES);
        t.0.write_note(&folder, &id, &max).unwrap();
        assert!(matches!(
            t.0.write_note(&folder, &id, &format!("{max}x")),
            Err(NotesError::TooLarge { .. })
        ));
        assert_eq!(t.0.read_note(&folder, &id).unwrap().len(), MAX_NOTE_BYTES);
        for bad in ["", "../folder", "x"] {
            assert!(matches!(
                t.0.read_note(&folder, bad),
                Err(NotesError::InvalidId)
            ));
        }
        let unknown = new_id();
        assert!(matches!(
            t.0.write_note(&folder, &unknown, "x"),
            Err(NotesError::UnknownNote)
        ));
        // Scope without notes: reading reports "not found", listing returns nothing.
        assert!(t.0.list_notes(&project()).unwrap().is_empty());
        assert!(matches!(
            t.0.read_note(&project(), &unknown),
            Err(NotesError::UnknownNote)
        ));
        assert!(matches!(
            t.0.create_note(&folder, " "),
            Err(NotesError::InvalidTitle(_))
        ));
        // Error texts never contain the content.
        let secret = "sk-secret";
        let error =
            t.0.write_note(&folder, &id, &format!("{secret}{max}"))
                .unwrap_err();
        assert!(!error.to_string().contains(secret));
    }

    #[test]
    fn reorder_and_append() {
        let t = TempNotes::new("order");
        let folder = general();
        let a = t.0.create_note(&folder, "a").unwrap();
        let b = t.0.create_note(&folder, "b").unwrap();
        let c = t.0.create_note(&folder, "c").unwrap();
        t.0.reorder_notes(&folder, &[c.clone(), a.clone()]).unwrap();
        let ids: Vec<String> =
            t.0.list_notes(&folder)
                .unwrap()
                .into_iter()
                .map(|n| n.id)
                .collect();
        assert_eq!(ids, [c.clone(), a.clone(), b.clone()]);
        let orders: Vec<u32> =
            t.0.list_notes(&folder)
                .unwrap()
                .iter()
                .map(|n| n.order)
                .collect();
        assert_eq!(orders, [0, 1, 2]);
        assert!(matches!(
            t.0.reorder_notes(&folder, &[new_id()]),
            Err(NotesError::UnknownNote)
        ));
        // New notes go to the end.
        let d = t.0.create_note(&folder, "d").unwrap();
        assert_eq!(t.0.list_notes(&folder).unwrap().last().unwrap().id, d);
    }

    #[test]
    fn sync_with_files_added_from_outside() {
        let t = TempNotes::new("sync");
        let folder = project();
        let id = t.0.create_note(&folder, "own").unwrap();
        let dir = t.0.root().join(folder_key(&folder).unwrap());
        fs::write(dir.join("From outside.md"), "hello").unwrap();
        fs::write(dir.join("not-markdown.txt"), "x").unwrap();
        fs::remove_file(dir.join("own.md")).unwrap();
        let notes = t.0.list_notes(&folder).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].title, "From outside");
        assert_ne!(notes[0].id, id);
        // The adopted ID stays stable.
        assert_eq!(t.0.list_notes(&folder).unwrap()[0].id, notes[0].id);
        assert_eq!(t.0.read_note(&folder, &notes[0].id).unwrap(), "hello");
    }

    #[test]
    fn broken_metadata_loses_no_notes() {
        let t = TempNotes::new("broken");
        let folder = project();
        let id = t.0.create_note(&folder, "important").unwrap();
        t.0.write_note(&folder, &id, "content").unwrap();
        let dir = t.0.root().join(folder_key(&folder).unwrap());
        fs::write(dir.join(META_FILE), "{ broken").unwrap();
        let notes = t.0.list_notes(&folder).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].title, "important");
        assert_eq!(t.0.read_note(&folder, &notes[0].id).unwrap(), "content");
        let aside = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .any(|n| n.starts_with("folder.json.broken-"));
        assert!(aside);
    }

    #[test]
    fn folder_overview() {
        let t = TempNotes::new("folders");
        let list = t.0.list_folders().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].folder_key, GENERAL_KEY);
        assert_eq!(list[0].path, None);
        assert_eq!(list[0].note_count, 0);

        t.0.create_note(&project(), "x").unwrap();
        t.0.create_note(&project(), "y").unwrap();
        t.0.create_note(&general(), "z").unwrap();
        // A scope without notes does not show up.
        let empty = NoteFolder {
            path: Some(std::env::temp_dir().join("hangar-empty")),
            ..NoteFolder::default()
        };
        let gone = t.0.create_note(&empty, "gone").unwrap();
        t.0.delete_note(&empty, &gone).unwrap();

        let list = t.0.list_folders().unwrap();
        assert_eq!(list.len(), 2, "{list:?}");
        assert_eq!(list[0].folder_key, GENERAL_KEY);
        assert_eq!(list[0].note_count, 1);
        assert_eq!(list[1].path, project().path);
        assert_eq!(list[1].note_count, 2);
        assert!(list[1].updated_at.is_some());
        let json = serde_json::to_value(&list[1]).unwrap();
        assert!(json["folderKey"].is_string());
        assert!(json["noteCount"].is_number());
        assert!(json.get("updatedAt").is_some());
    }

    /// Turns the current layout into the one written before the English folder names.
    fn make_legacy(root: &Path) {
        for entry in fs::read_dir(root).unwrap() {
            let dir = entry.unwrap().path();
            if dir.join(TRASH_DIR).is_dir() {
                fs::rename(dir.join(TRASH_DIR), dir.join(LEGACY_TRASH_DIR)).unwrap();
            }
        }
        fs::rename(root.join(GENERAL_KEY), root.join(LEGACY_GENERAL_KEY)).unwrap();
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn migration_on_fresh_install_changes_nothing() {
        let t = TempNotes::new("migrate-fresh");
        t.0.migrate_legacy_layout();
        assert!(!t.0.root().exists());
        let id = t.0.create_note(&general(), "x").unwrap();
        t.0.migrate_legacy_layout();
        assert_eq!(names(t.0.root()), [GENERAL_KEY]);
        assert_eq!(t.0.list_notes(&general()).unwrap()[0].id, id);
    }

    #[test]
    fn migration_moves_the_old_layout() {
        let t = TempNotes::new("migrate-old");
        let id = t.0.create_note(&general(), "Plan: Q4/2026").unwrap();
        t.0.write_note(&general(), &id, "content").unwrap();
        let gone = t.0.create_note(&general(), "gone").unwrap();
        t.0.delete_note(&general(), &gone).unwrap();
        let project_id = t.0.create_note(&project(), "p").unwrap();
        let gone = t.0.create_note(&project(), "q").unwrap();
        t.0.delete_note(&project(), &gone).unwrap();
        make_legacy(t.0.root());
        let project_key = folder_key(&project()).unwrap();
        assert!(t.0.root().join(LEGACY_GENERAL_KEY).is_dir());

        t.0.migrate_legacy_layout();

        let mut expected = vec![GENERAL_KEY.to_owned(), project_key.clone()];
        expected.sort();
        assert_eq!(names(t.0.root()), expected);
        let notes = t.0.list_notes(&general()).unwrap();
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].id, id);
        assert_eq!(notes[0].title, "Plan: Q4/2026");
        assert_eq!(t.0.read_note(&general(), &id).unwrap(), "content");
        for key in [GENERAL_KEY, project_key.as_str()] {
            let dir = t.0.root().join(key);
            let notes: Vec<String> = names(&dir.join(TRASH_DIR))
                .into_iter()
                .filter(|name| is_markdown(name))
                .collect();
            assert_eq!(notes.len(), 1, "{key}");
            assert!(!dir.join(LEGACY_TRASH_DIR).exists(), "{key}");
        }
        assert_eq!(t.0.list_notes(&project()).unwrap()[0].id, project_id);
    }

    #[test]
    fn migration_merges_both_layouts_without_losing_notes() {
        let t = TempNotes::new("migrate-both");
        let old_id = t.0.create_note(&general(), "same").unwrap();
        t.0.write_note(&general(), &old_id, "old").unwrap();
        let titled = t.0.create_note(&general(), "Plan: A").unwrap();
        make_legacy(t.0.root());
        let legacy = t.0.root().join(LEGACY_GENERAL_KEY);
        fs::create_dir_all(legacy.join(LEGACY_TRASH_DIR)).unwrap();
        fs::write(legacy.join(LEGACY_TRASH_DIR).join("1-x.md"), "old trash").unwrap();

        let new_id = t.0.create_note(&general(), "same").unwrap();
        t.0.write_note(&general(), &new_id, "new").unwrap();
        let dir = t.0.root().join(GENERAL_KEY);
        fs::create_dir_all(dir.join(TRASH_DIR)).unwrap();
        fs::write(dir.join(TRASH_DIR).join("1-x.md"), "new trash").unwrap();

        t.0.migrate_legacy_layout();

        assert!(!legacy.exists());
        let notes = t.0.list_notes(&general()).unwrap();
        assert_eq!(notes.len(), 3, "{notes:?}");
        // The note already in the new folder keeps its file; the moved one gets a suffix but
        // keeps its ID and title.
        assert_eq!(notes[0].id, new_id);
        assert_eq!(t.0.read_note(&general(), &new_id).unwrap(), "new");
        assert_eq!(t.0.read_note(&general(), &old_id).unwrap(), "old");
        assert_eq!(fs::read_to_string(dir.join("same.md")).unwrap(), "new");
        assert_eq!(fs::read_to_string(dir.join("same (2).md")).unwrap(), "old");
        assert!(notes.iter().any(|n| n.id == old_id && n.title == "same"));
        assert!(notes.iter().any(|n| n.id == titled && n.title == "Plan: A"));
        let trash = dir.join(TRASH_DIR);
        assert_eq!(names(&trash), ["1-x (2).md", "1-x.md"]);
        assert_eq!(
            fs::read_to_string(trash.join("1-x.md")).unwrap(),
            "new trash"
        );
        assert_eq!(
            fs::read_to_string(trash.join("1-x (2).md")).unwrap(),
            "old trash"
        );
    }

    #[test]
    fn migration_keeps_unreadable_legacy_metadata() {
        let t = TempNotes::new("migrate-broken");
        t.0.create_note(&general(), "a").unwrap();
        make_legacy(t.0.root());
        let legacy = t.0.root().join(LEGACY_GENERAL_KEY);
        fs::write(legacy.join(META_FILE), "{ broken").unwrap();
        t.0.create_note(&general(), "b").unwrap();

        t.0.migrate_legacy_layout();

        // The note moved and is adopted; the metadata that could not be merged stays behind,
        // and with it the old folder.
        assert_eq!(names(&legacy), [META_FILE]);
        let titles: Vec<String> =
            t.0.list_notes(&general())
                .unwrap()
                .into_iter()
                .map(|n| n.title)
                .collect();
        assert_eq!(titles, ["b", "a"]);
    }

    #[test]
    fn migration_is_idempotent() {
        let t = TempNotes::new("migrate-repeat");
        let id = t.0.create_note(&general(), "x").unwrap();
        let gone = t.0.create_note(&general(), "y").unwrap();
        t.0.delete_note(&general(), &gone).unwrap();
        make_legacy(t.0.root());

        t.0.migrate_legacy_layout();
        let first = t.0.list_notes(&general()).unwrap();
        let trash = names(&t.0.root().join(GENERAL_KEY).join(TRASH_DIR));
        t.0.migrate_legacy_layout();

        assert_eq!(t.0.list_notes(&general()).unwrap(), first);
        assert_eq!(first[0].id, id);
        assert_eq!(names(t.0.root()), [GENERAL_KEY]);
        assert_eq!(names(&t.0.root().join(GENERAL_KEY).join(TRASH_DIR)), trash);
    }

    #[test]
    fn search_across_folders() {
        let t = TempNotes::new("search");
        let a = t.0.create_note(&project(), "Deploy plan").unwrap();
        t.0.write_note(&project(), &a, "Steps:\n\n1. Build\n2. Ship it")
            .unwrap();
        let b = t.0.create_note(&general(), "Groceries").unwrap();
        t.0.write_note(&general(), &b, "Milk, bread\nand the DEPLOY checklist")
            .unwrap();
        let c = t.0.create_note(&general(), "Other").unwrap();
        t.0.write_note(&general(), &c, "nothing here").unwrap();

        let hits = t.0.search("deploy").unwrap();
        assert_eq!(hits.len(), 2, "{hits:?}");
        // The title hit comes first.
        assert_eq!(hits[0].id, a);
        assert_eq!(hits[0].path, project().path);
        assert_eq!(hits[0].folder_key, folder_key(&project()).unwrap());
        assert!(
            hits[0].excerpt.starts_with("Steps: 1. Build"),
            "{:?}",
            hits[0].excerpt
        );
        assert_eq!(hits[1].id, b);
        assert_eq!(hits[1].path, None);
        assert_eq!(hits[1].folder_key, GENERAL_KEY);
        assert!(hits[1].excerpt.contains("the DEPLOY checklist"));
        assert!(hits[1].updated_at.is_some());
        let json = serde_json::to_value(&hits[1]).unwrap();
        for field in ["folderKey", "path", "id", "title", "excerpt", "updatedAt"] {
            assert!(json.get(field).is_some(), "{field}");
        }

        // Case-insensitive beyond ASCII; regex characters are plain text.
        t.0.write_note(&general(), &c, "Größe ÄRGER (a+b)*")
            .unwrap();
        assert_eq!(t.0.search("ärger").unwrap()[0].id, c);
        assert_eq!(t.0.search("(a+b)*").unwrap()[0].id, c);
        assert!(t.0.search(".*").unwrap().is_empty());
        // Empty finds nothing; too long or with control characters is an error.
        assert!(t.0.search("   ").unwrap().is_empty());
        assert!(matches!(
            t.0.search(&"x".repeat(MAX_QUERY_CHARS + 1)),
            Err(NotesError::InvalidQuery(_))
        ));
        assert!(matches!(
            t.0.search("a\nb"),
            Err(NotesError::InvalidQuery(_))
        ));
        // A file above the size limit is skipped, not an error.
        let dir = t.0.root().join(GENERAL_KEY);
        fs::write(
            dir.join("Groceries.md"),
            "deploy ".repeat(MAX_NOTE_BYTES / 6),
        )
        .unwrap();
        let hits = t.0.search("deploy").unwrap();
        assert_eq!(hits.len(), 1);
        // A search changes nothing on disk.
        assert!(!dir.join(TRASH_DIR).exists());
    }

    #[test]
    fn metadata_naming_other_files_is_not_trusted() {
        let t = TempNotes::new("trusted");
        let folder = general();
        let id = t.0.create_note(&folder, "own").unwrap();
        let dir = t.0.root().join(GENERAL_KEY);
        fs::write(t.0.root().join("outside.md"), "foreign").unwrap();
        let mut meta: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dir.join(META_FILE)).unwrap()).unwrap();
        let notes = meta["notes"].as_array_mut().unwrap();
        for (n, file) in ["../outside.md", "sub/x.md", "x.txt", "C:\\x.md"]
            .iter()
            .enumerate()
        {
            notes.push(serde_json::json!({
                "id": format!("00000000-0000-4000-8000-00000000000{n}"),
                "title": "evil",
                "file": file,
                "order": 9
            }));
        }
        fs::write(dir.join(META_FILE), meta.to_string()).unwrap();
        let listed = t.0.list_notes(&folder).unwrap();
        assert_eq!(listed.len(), 1, "{listed:?}");
        assert_eq!(listed[0].id, id);
        for n in 0..4 {
            let foreign = format!("00000000-0000-4000-8000-00000000000{n}");
            assert!(t.0.read_note(&folder, &foreign).is_err());
        }
        assert!(t.0.search("foreign").unwrap().is_empty());
        assert!(plain_markdown_name("a b.md"));
        assert!(!plain_markdown_name(".."));
        assert!(!plain_markdown_name("a/b.md"));
        assert!(!plain_markdown_name("a\\b.md"));
    }

    #[test]
    fn capped_read_never_reads_past_the_limit() {
        let t = TempNotes::new("capped");
        let id = t.0.create_note(&general(), "big").unwrap();
        let path = t.0.note_path(&general(), &id).unwrap();
        fs::write(&path, "x".repeat(MAX_NOTE_BYTES + 1)).unwrap();
        assert!(matches!(
            t.0.read_note(&general(), &id),
            Err(NotesError::TooLarge { .. })
        ));
        assert_eq!(read_capped(&path, MAX_NOTE_BYTES).unwrap(), None);
        // A folder named like a note is never opened for reading.
        let folder = path.with_file_name("folder.md");
        fs::create_dir_all(&folder).unwrap();
        assert!(read_capped(&folder, MAX_NOTE_BYTES).is_err());
        assert_eq!(
            read_capped(&path, MAX_NOTE_BYTES + 1)
                .unwrap()
                .map(|b| b.len()),
            Some(MAX_NOTE_BYTES + 1)
        );
    }

    #[test]
    fn search_limits_results() {
        let t = TempNotes::new("search-limit");
        for n in 0..(MAX_SEARCH_RESULTS + 5) {
            t.0.create_note(&general(), &format!("hit {n}")).unwrap();
        }
        assert_eq!(t.0.search("HIT").unwrap().len(), MAX_SEARCH_RESULTS);
        assert!(t.0.search("hit").is_ok());
        assert!(NotesStore::new(&t.1.join("missing"))
            .search("x")
            .unwrap()
            .is_empty());
    }

    #[test]
    fn excerpts_cut_on_characters() {
        let long = format!("{}needle{}", "ä ".repeat(100), "ö".repeat(400));
        let text = excerpt(&long, "needle");
        assert!(text.starts_with('…'));
        assert!(text.ends_with('…'));
        assert!(text.contains("needle"));
        assert!(text.chars().count() <= EXCERPT_MAX_CHARS + 2);
        // Lowercasing that changes byte lengths does not shift the cut (İ lowercases to two chars).
        let tricky = format!("{}Needle", "İ".repeat(80));
        assert!(excerpt(&tricky, "needle").contains("Needle"));
        assert_eq!(excerpt("short\n\ttext", "zzz"), "short text");
    }

    #[test]
    fn trash_lists_and_restores() {
        let t = TempNotes::new("trash");
        let folder = project();
        assert!(t.0.list_trash(&folder).unwrap().is_empty());
        let id = t.0.create_note(&folder, "Plan: Q4/2026").unwrap();
        t.0.write_note(&folder, &id, "content").unwrap();
        let other = t.0.create_note(&folder, "stays").unwrap();
        t.0.delete_note(&folder, &id).unwrap();

        let trash = t.0.list_trash(&folder).unwrap();
        assert_eq!(trash.len(), 1);
        assert_eq!(trash[0].id, id);
        assert_eq!(trash[0].title, "Plan: Q4/2026");
        assert!(trash[0].deleted_at.is_some());
        let json = serde_json::to_value(&trash[0]).unwrap();
        assert!(json.get("deletedAt").is_some());

        // A new note took the file name meanwhile: the restored one gets its own.
        let squatter = t.0.create_note(&folder, "Plan: Q4/2026").unwrap();
        let restored = t.0.restore_note(&folder, &id).unwrap();
        assert_eq!(restored, id);
        assert_eq!(t.0.read_note(&folder, &id).unwrap(), "content");
        let notes = t.0.list_notes(&folder).unwrap();
        let ids: Vec<&str> = notes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, [other.as_str(), squatter.as_str(), id.as_str()]);
        assert!(t.0.list_trash(&folder).unwrap().is_empty());
        assert!(matches!(
            t.0.restore_note(&folder, &id),
            Err(NotesError::UnknownNote)
        ));
        assert!(matches!(
            t.0.restore_note(&folder, "../x"),
            Err(NotesError::InvalidId)
        ));
    }

    #[test]
    fn trash_adopts_files_without_record() {
        let t = TempNotes::new("trash-adopt");
        let folder = general();
        t.0.create_note(&folder, "keep").unwrap();
        let trash = t.0.root().join(GENERAL_KEY).join(TRASH_DIR);
        fs::create_dir_all(&trash).unwrap();
        // As written before trash.json existed.
        fs::write(trash.join("1700000000000-Old idea.md"), "old").unwrap();
        fs::write(trash.join("not a note.txt"), "x").unwrap();
        let list = t.0.list_trash(&folder).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Old idea");
        assert_eq!(list[0].deleted_at, Some(1_700_000_000_000));
        // The adopted ID stays stable and can be restored.
        assert_eq!(t.0.list_trash(&folder).unwrap()[0].id, list[0].id);
        let id = t.0.restore_note(&folder, &list[0].id).unwrap();
        assert_eq!(t.0.read_note(&folder, &id).unwrap(), "old");
        assert!(t.0.root().join(GENERAL_KEY).join("Old idea.md").is_file());
        // A broken record does not hide the files.
        let gone = t.0.create_note(&folder, "gone").unwrap();
        t.0.delete_note(&folder, &gone).unwrap();
        fs::write(trash.join(TRASH_META_FILE), "{ broken").unwrap();
        assert_eq!(t.0.list_trash(&folder).unwrap()[0].title, "gone");
    }

    #[test]
    fn trash_records_pointing_elsewhere_are_ignored() {
        let t = TempNotes::new("trash-outside");
        let folder = general();
        t.0.create_note(&folder, "keep").unwrap();
        let dir = t.0.root().join(GENERAL_KEY);
        let trash = dir.join(TRASH_DIR);
        fs::create_dir_all(&trash).unwrap();
        // A file next to the scope that a forged record tries to pull into the notes.
        fs::write(t.0.root().join("outside.md"), "foreign").unwrap();
        let forged = new_id();
        let sep = std::path::MAIN_SEPARATOR;
        let record = serde_json::json!({
            "version": TRASH_META_VERSION,
            "items": [
                { "id": forged, "title": "x", "file": format!("..{sep}..{sep}outside.md"), "deletedAt": 1 },
                { "id": "not-an-id", "title": "y", "file": "y.md", "deletedAt": 2 },
            ]
        });
        fs::write(trash.join(TRASH_META_FILE), record.to_string()).unwrap();
        fs::write(trash.join("y.md"), "kept").unwrap();
        let list = t.0.list_trash(&folder).unwrap();
        // Only the real file in the trash remains, adopted under a fresh ID.
        assert_eq!(list.len(), 1, "{list:?}");
        assert_ne!(list[0].id, "not-an-id");
        assert!(matches!(
            t.0.restore_note(&folder, &forged),
            Err(NotesError::UnknownNote)
        ));
        assert!(t.0.root().join("outside.md").is_file());
    }

    #[test]
    fn export_and_path_of_a_note() {
        let t = TempNotes::new("export");
        let id = t.0.create_note(&general(), "a/b").unwrap();
        t.0.write_note(&general(), &id, "text").unwrap();
        assert_eq!(
            t.0.note_for_export(&general(), &id).unwrap(),
            ("a/b".to_owned(), "text".to_owned())
        );
        let path = t.0.note_path(&general(), &id).unwrap();
        assert!(path.is_file());
        assert!(path.starts_with(t.0.root()));
        assert!(matches!(
            t.0.note_path(&general(), &new_id()),
            Err(NotesError::UnknownNote)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn unix_permissions_only_for_owner() {
        use std::os::unix::fs::PermissionsExt;
        let t = TempNotes::new("mode");
        let id = t.0.create_note(&general(), "x").unwrap();
        t.0.write_note(&general(), &id, "y").unwrap();
        let dir = t.0.root().join(GENERAL_KEY);
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&dir), 0o700);
        assert_eq!(mode(&dir.join("x.md")), 0o600);
        assert_eq!(mode(&dir.join(META_FILE)), 0o600);
    }
}
