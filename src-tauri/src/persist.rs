//! Workspace and scrollback across a restart of Hangar.
//!
//! Running processes do not survive closing the app (no background service). Only what the
//! frontend needs for restoring is stored:
//! - `workspace.json`: which sessions were open and which one was active ([`Workspace`])
//! - `scrollback/<key>.txt`: the scrollback per session, serialized by the frontend
//!
//! Writing is atomic (temporary file, `sync_all`, then `rename`), so a crash in the middle of
//! saving never leaves half a file behind. Contents are never logged: scrollback may contain
//! secrets (printed tokens, pasted keys). The key shapes the secret guard knows are masked
//! before a scrollback is written ([`Store::save_scrollback`]); others are stored as printed.
//!
//! Permissions: on Unix folders 0700 and files 0600. On Windows the files live in the
//! user profile (`%LOCALAPPDATA%\<identifier>`) and inherit its ACL (user, SYSTEM,
//! Administrators); Hangar sets no ACLs of its own there.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize, Serializer};

use crate::worktree::Worktree;

pub const WORKSPACE_FILE: &str = "workspace.json";
pub const SCROLLBACK_DIR: &str = "scrollback";
const SCROLLBACK_EXT: &str = "txt";
const TEMP_EXT: &str = "tmp";
/// Current schema of `workspace.json`. Any other version is not loaded.
pub const WORKSPACE_VERSION: u32 = 1;
/// Upper limit per scrollback in UTF-8 bytes. Trimming older content is the frontend's job.
pub const MAX_SCROLLBACK_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_SESSIONS: usize = 512;
/// Length of one queued message in UTF-16 units: the editor cuts there (`MAX_QUEUED_CHARS` in
/// `messageQueue.ts`) and counts the way JavaScript does, so every text it accepts is stored and
/// nothing longer is. A prompt that long is pasted from a file anyway.
pub const MAX_QUEUED_MESSAGE_UNITS: usize = 20_000;
/// Room for the sessions themselves, plus every session's queued message at its longest with
/// each UTF-16 unit escaped as `\uXXXX` (six bytes). A workspace that passes [`validate`] thus
/// always fits, and a crafted file stays bounded.
pub const MAX_WORKSPACE_BYTES: usize = 1024 * 1024 + MAX_SESSIONS * MAX_QUEUED_MESSAGE_UNITS * 6;
/// One emoji with modifiers and joiners fits; a sentence does not.
pub const MAX_EMOJI_BYTES: usize = 16;

/// Content of `workspace.json` and argument of `save_workspace`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Workspace {
    /// Must be [`WORKSPACE_VERSION`].
    pub version: u32,
    pub sessions: Vec<SavedSession>,
    /// Key of the last active session; must appear in `sessions`.
    #[serde(default)]
    pub active_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedSession {
    /// UUID assigned by the frontend; stays the same across restarts (unlike the session id).
    pub key: String,
    pub profile_id: String,
    pub cwd: PathBuf,
    pub title: String,
    /// Position in the sidebar.
    pub order: u32,
    /// Session marker: `#rrggbb`. Absent in files from before 0.3.0.
    #[serde(default)]
    pub color: Option<String>,
    /// Session marker: at most [`MAX_EMOJI_BYTES`] of UTF-8.
    #[serde(default)]
    pub emoji: Option<String>,
    /// Own worktree of the session, removed when the session is closed.
    #[serde(default)]
    pub worktree: Option<Worktree>,
    /// Conversation id of the agent CLI (`SessionInfo.cliSessionId`), a UUID. Absent in files
    /// from before 0.3.0 and for programs without `session_id_args`.
    #[serde(default)]
    pub cli_session_id: Option<String>,
    /// The sub-agent definition the session runs as (`SessionInfo.subAgent`), started as that
    /// one again on restore. Absent in files from before 0.9.0.
    #[serde(default)]
    pub sub_agent: Option<String>,
    /// The next instruction Hangar sends once the agent is done (`messageQueue.ts`): not blank,
    /// at most [`MAX_QUEUED_MESSAGE_UNITS`]. Absent in files from before 0.14.0 and left out when
    /// there is none, so such a workspace still reads in an older Hangar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_message: Option<String>,
    /// The model the session started with (`SessionInfo.model`, ADR 0022), started with it again
    /// on restore. Absent in older files and left out when there is none, so such a workspace
    /// still reads in an older Hangar.
    /// An invalid value is dropped on reading, the session stays ([`crate::start_model::lenient_stored`]).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::start_model::lenient_stored"
    )]
    pub model: Option<crate::start_model::StartModel>,
}

#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    #[error("Invalid session key: expected a UUID")]
    InvalidKey,
    #[error("Scrollback too large: {size} bytes, at most {max} allowed")]
    TooLarge { size: usize, max: usize },
    #[error("Only the main window may do this")]
    WrongWindow,
    #[error("Invalid workspace: {0}")]
    InvalidWorkspace(&'static str),
    #[error("File {path} cannot be read or written: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Background task aborted: {0}")]
    Task(String),
}

/// Errors reach the frontend as readable text. No error text contains file contents.
impl Serialize for PersistError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> PersistError + '_ {
    move |source| PersistError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Canonical UUID (8-4-4-4-12 hex digits), upper or lower case. Everything else, above all
/// path separators, `..` and drive letters, is rejected. The result is lowercase because
/// Windows file names are case-insensitive.
pub fn normalize_key(key: &str) -> Result<String, PersistError> {
    let bytes = key.as_bytes();
    let valid = bytes.len() == 36
        && bytes.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
    if valid {
        Ok(key.to_ascii_lowercase())
    } else {
        Err(PersistError::InvalidKey)
    }
}

/// Checks the contract of [`Workspace`]. Applies on saving and on loading.
pub fn validate(workspace: &Workspace) -> Result<(), PersistError> {
    let invalid = |reason| Err(PersistError::InvalidWorkspace(reason));
    if workspace.version != WORKSPACE_VERSION {
        return invalid("unknown schema version");
    }
    if workspace.sessions.len() > MAX_SESSIONS {
        return invalid("too many sessions");
    }
    let mut keys = std::collections::HashSet::new();
    let mut cli_ids = std::collections::HashSet::new();
    for session in &workspace.sessions {
        let key = normalize_key(&session.key)
            .or(Err(PersistError::InvalidWorkspace("key is not a UUID")))?;
        if !keys.insert(key) {
            return invalid("key appears more than once");
        }
        if session.profile_id.trim().is_empty() {
            return invalid("profileId must not be empty");
        }
        if session.color.as_deref().is_some_and(|c| !is_hex_color(c)) {
            return invalid("color must be #rrggbb");
        }
        if let Some(emoji) = &session.emoji {
            if emoji.is_empty()
                || emoji.len() > MAX_EMOJI_BYTES
                || emoji.chars().any(char::is_control)
            {
                return invalid("emoji must be 1 to 16 bytes without control characters");
            }
        }
        if let Some(id) = &session.cli_session_id {
            let Ok(id) = normalize_key(id) else {
                return invalid("cliSessionId is not a UUID");
            };
            // Two entries would resume one conversation in two processes.
            if !cli_ids.insert(id) {
                return invalid("cliSessionId appears more than once");
            }
        }
        if let Some(worktree) = &session.worktree {
            if crate::worktree::check_shape(worktree).is_err() {
                return invalid("worktree is not one Hangar created");
            }
        }
        if session
            .sub_agent
            .as_deref()
            .is_some_and(|name| !crate::agent_defs::valid_name(name))
        {
            return invalid("subAgent is no valid sub-agent name");
        }
        if let Some(text) = &session.queued_message {
            if text.trim().is_empty() || text.encode_utf16().count() > MAX_QUEUED_MESSAGE_UNITS {
                return invalid("queuedMessage must be neither blank nor too long");
            }
        }
    }
    if let Some(active) = &workspace.active_key {
        let active = normalize_key(active).or(Err(PersistError::InvalidWorkspace(
            "activeKey is not a UUID",
        )))?;
        if !keys.contains(&active) {
            return invalid("activeKey belongs to no session");
        }
    }
    Ok(())
}

/// `#rrggbb` with hex digits in either case.
pub(crate) fn is_hex_color(color: &str) -> bool {
    color.len() == 7
        && color.starts_with('#')
        && color.bytes().skip(1).all(|b| b.is_ascii_hexdigit())
}

/// File storage in the app data folder. Cheap to clone, so commands can hand the work to a
/// blocking thread.
#[derive(Debug, Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn workspace_path(&self) -> PathBuf {
        self.dir.join(WORKSPACE_FILE)
    }

    fn scrollback_dir(&self) -> PathBuf {
        self.dir.join(SCROLLBACK_DIR)
    }

    fn scrollback_path(&self, key: &str) -> Result<PathBuf, PersistError> {
        let key = normalize_key(key)?;
        Ok(self
            .scrollback_dir()
            .join(format!("{key}.{SCROLLBACK_EXT}")))
    }

    pub fn save_workspace(&self, workspace: &Workspace) -> Result<(), PersistError> {
        validate(workspace)?;
        let path = self.workspace_path();
        let json = serde_json::to_vec_pretty(workspace)
            .map_err(|e| io_error(&path)(io::Error::new(io::ErrorKind::InvalidData, e)))?;
        if json.len() > MAX_WORKSPACE_BYTES {
            return Err(PersistError::InvalidWorkspace("file would be too large"));
        }
        write_atomic(&path, &json).map_err(io_error(&path))
    }

    /// The keys of the saved sessions, lower case: an empty list without a saved workspace,
    /// `None` when one exists but cannot be read (what belongs to it is then unknown).
    pub fn workspace_keys(&self) -> Option<std::collections::BTreeSet<String>> {
        if !self.workspace_path().exists() {
            return Some(std::collections::BTreeSet::new());
        }
        let workspace = self.load_workspace()?;
        Some(
            workspace
                .sessions
                .iter()
                .filter_map(|session| normalize_key(&session.key).ok())
                .collect(),
        )
    }

    /// `None` if there is no saved workspace or it is unusable. The
    /// reason goes to the log, the content never does (not even excerpts from parser errors).
    pub fn load_workspace(&self) -> Option<Workspace> {
        let path = self.workspace_path();
        let bytes = match read_limited(&path, MAX_WORKSPACE_BYTES) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return None,
            Err(reason) => {
                eprintln!("hangar: ignoring {}: {reason}", path.display());
                return None;
            }
        };
        let workspace: Workspace = match serde_json::from_slice(&bytes) {
            Ok(workspace) => workspace,
            Err(error) => {
                eprintln!(
                    "hangar: ignoring {}: JSON error ({:?}) on line {}, column {}",
                    path.display(),
                    error.classify(),
                    error.line(),
                    error.column()
                );
                return None;
            }
        };
        if let Err(error) = validate(&workspace) {
            eprintln!("hangar: ignoring {}: {error}", path.display());
            return None;
        }
        Some(workspace)
    }

    /// Saves a session's scrollback with every key the secret guard knows masked in place
    /// ([`crate::secret_scan::mask`]): agents print tokens and `.env` files, and the file outlives
    /// the session and lands in backups. The mask keeps the length, so the restored layout stays.
    pub fn save_scrollback(&self, key: &str, data: &str) -> Result<(), PersistError> {
        let path = self.scrollback_path(key)?;
        if data.len() > MAX_SCROLLBACK_BYTES {
            return Err(PersistError::TooLarge {
                size: data.len(),
                max: MAX_SCROLLBACK_BYTES,
            });
        }
        let masked = crate::secret_scan::mask(data);
        write_atomic(&path, masked.as_bytes()).map_err(io_error(&path))
    }

    /// `Ok(None)` if there is no scrollback or the file is unusable (too large, not
    /// UTF-8); the latter goes to the log without content. An invalid key is an error.
    pub fn load_scrollback(&self, key: &str) -> Result<Option<String>, PersistError> {
        let path = self.scrollback_path(key)?;
        let bytes = match read_limited(&path, MAX_SCROLLBACK_BYTES) {
            Ok(Some(bytes)) => bytes,
            Ok(None) => return Ok(None),
            Err(reason) => {
                eprintln!("hangar: ignoring {}: {reason}", path.display());
                return Ok(None);
            }
        };
        match String::from_utf8(bytes) {
            Ok(text) => Ok(Some(text)),
            Err(_) => {
                eprintln!("hangar: ignoring {}: not UTF-8", path.display());
                Ok(None)
            }
        }
    }

    /// If the file is already gone, that is not an error.
    pub fn delete_scrollback(&self, key: &str) -> Result<(), PersistError> {
        let path = self.scrollback_path(key)?;
        remove_if_exists(&path).map_err(io_error(&path))
    }

    /// Deletes all scrollback files and leftover temporary files, but nothing else in the
    /// folder. An error on one file does not stop the others; the first one is reported.
    pub fn clear_scrollbacks(&self) -> Result<(), PersistError> {
        let dir = self.scrollback_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(io_error(&dir)(error)),
        };
        let mut first_error = None;
        for entry in entries {
            let path = match entry {
                Ok(entry) => entry.path(),
                Err(error) => {
                    first_error.get_or_insert_with(|| io_error(&dir)(error));
                    continue;
                }
            };
            if !is_own_scrollback_file(&path) {
                continue;
            }
            if let Err(error) = remove_if_exists(&path) {
                first_error.get_or_insert_with(|| io_error(&path)(error));
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

/// `<uuid>.txt` or a temporary file from [`write_atomic`].
fn is_own_scrollback_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with('.') && name.ends_with(&format!(".{TEMP_EXT}")) {
        return true;
    }
    name.strip_suffix(&format!(".{SCROLLBACK_EXT}"))
        .is_some_and(|stem| normalize_key(stem).is_ok())
}

pub(crate) fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Opens `path` for reading only if it is a regular file. A link at the last component is not
/// followed: a symbolic link or junction placed in a repository could lead to a device, a FIFO
/// or, on Windows, a network share that receives the user's NTLM credentials. A FIFO does not
/// block the open (`O_NONBLOCK`), and the type is checked on the open handle, so nothing swapped
/// in after an earlier check is read. Others may keep writing, renaming or deleting the file.
pub(crate) fn open_regular(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        options.share_mode(0x1 | 0x2 | 0x4);
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // On a regular file O_NONBLOCK changes nothing.
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        // O_NOFOLLOW fails the open of a link with ELOOP (Linux, macOS): the same refusal as for
        // any other entry that is no regular file, which callers tell apart from a failed read.
        #[cfg(unix)]
        Err(error) if error.raw_os_error() == Some(libc::ELOOP) => return Err(not_a_regular_file()),
        Err(error) => return Err(error),
    };
    if !file.metadata()?.is_file() {
        return Err(not_a_regular_file());
    }
    #[cfg(windows)]
    let file = windows_reparse::through_placeholder(file, path)?;
    Ok(file)
}

fn not_a_regular_file() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "not a regular file")
}

/// A regular file that is still a reparse point on Windows: a cloud placeholder (OneDrive), a
/// deduplicated, a compressed system or a projected file. Opened as the reparse point itself,
/// its filter driver does not supply the content, so it is opened once more the ordinary way:
/// first through the handle already open (`ReOpenFile`, no second lookup of the path), and only
/// if that fails by its path again, then accepted only if it is still the same file. The tag is
/// read on the first handle and must be one of those content tags; name surrogates (symbolic
/// links, junctions, mount points) and every other tag are refused.
#[cfg(windows)]
mod windows_reparse {
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::mem::size_of;
    use std::os::windows::fs::{MetadataExt as _, OpenOptionsExt as _};
    use std::os::windows::io::AsRawHandle as _;
    use std::path::Path;

    use super::not_a_regular_file;

    use std::os::windows::io::FromRawHandle as _;

    use windows_sys::Win32::Foundation::{GENERIC_READ, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        FileAttributeTagInfo, GetFileInformationByHandle, GetFileInformationByHandleEx, ReOpenFile,
        BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_TAG_INFO, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE,
    };

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    /// IO_REPARSE_TAG_CLOUD with its sixteen subtypes (bits 12 to 15), ntifs.h.
    const TAG_CLOUD: u32 = 0x9000_001A;
    const TAG_CLOUD_MASK: u32 = 0xFFFF_0FFF;
    /// IO_REPARSE_TAG_DEDUP, IO_REPARSE_TAG_WOF, IO_REPARSE_TAG_PROJFS.
    const CONTENT_TAGS: [u32; 3] = [0x8000_0013, 0x8000_0017, 0x9000_001C];

    /// `IsReparseTagNameSurrogate` of winnt.h: the tag names another path.
    pub(super) fn names_another_path(tag: u32) -> bool {
        tag & 0x2000_0000 != 0
    }

    /// A tag whose filter supplies the file's own content (a placeholder, deduplicated,
    /// compressed or projected file), never another path.
    pub(super) fn supplies_content(tag: u32) -> bool {
        !names_another_path(tag)
            && (tag & TAG_CLOUD_MASK == TAG_CLOUD || CONTENT_TAGS.contains(&tag))
    }

    pub(super) fn through_placeholder(file: File, path: &Path) -> io::Result<File> {
        if file.metadata()?.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
            return Ok(file);
        }
        let tag = reparse_tag(&file)?;
        if !supplies_content(tag) {
            return Err(not_a_regular_file());
        }
        // [UNVERIFIED] whether every content filter supplies the data for a reopened handle as it
        // does for an open by path; where the reopen fails, the path is opened once more.
        if let Ok(content) = reopen(&file) {
            return Ok(content);
        }
        let mut options = OpenOptions::new();
        options.read(true);
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        options.share_mode(0x1 | 0x2 | 0x4);
        let content = options.open(path)?;
        if identity(&content)? != identity(&file)? {
            return Err(not_a_regular_file());
        }
        Ok(content)
    }

    /// The same file opened anew through `file` (`ReOpenFile`), for reading, without
    /// FILE_FLAG_OPEN_REPARSE_POINT; the path is not looked up again.
    pub(super) fn reopen(file: &File) -> io::Result<File> {
        // SAFETY: the handle is open for the call; ReOpenFile does not take it over and
        // returns a new handle or INVALID_HANDLE_VALUE.
        let handle = unsafe {
            ReOpenFile(
                file.as_raw_handle(),
                GENERIC_READ,
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
                0,
            )
        };
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `handle` is a new, valid handle owned by nobody else; File closes it.
        let reopened = unsafe { File::from_raw_handle(handle) };
        if identity(&reopened)? != identity(file)? || !reopened.metadata()?.is_file() {
            return Err(not_a_regular_file());
        }
        Ok(reopened)
    }

    fn reparse_tag(file: &File) -> io::Result<u32> {
        let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
        let size = u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).unwrap_or(u32::MAX);
        // SAFETY: the handle is open for the call; `info` is a writable FILE_ATTRIBUTE_TAG_INFO
        // of exactly `size` bytes, which is what FileAttributeTagInfo writes.
        let done = unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle(),
                FileAttributeTagInfo,
                (&raw mut info).cast(),
                size,
            )
        };
        if done == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info.ReparseTag)
    }

    /// Volume and file index: the same pair means the same file.
    fn identity(file: &File) -> io::Result<(u32, u32, u32)> {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the handle is open for the call and `info` is a writable
        // BY_HANDLE_FILE_INFORMATION.
        if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((
            info.dwVolumeSerialNumber,
            info.nFileIndexHigh,
            info.nFileIndexLow,
        ))
    }
}

/// At most `max` bytes from `reader`, `None` when it holds more. Never reads more than `max + 1`
/// bytes, whatever a size read earlier said.
fn read_up_to(reader: impl io::Read, max: usize) -> io::Result<Option<Vec<u8>>> {
    use std::io::Read as _;
    let mut bytes = Vec::new();
    let limit = u64::try_from(max).unwrap_or(u64::MAX).saturating_add(1);
    reader.take(limit).read_to_end(&mut bytes)?;
    Ok((bytes.len() <= max).then_some(bytes))
}

/// A regular file of at most `max` bytes ([`open_regular`]). `Ok(None)`: the file holds more.
pub(crate) fn read_regular_capped(path: &Path, max: usize) -> io::Result<Option<Vec<u8>>> {
    read_up_to(open_regular(path)?, max)
}

/// Reads a regular file of at most `max` bytes ([`read_regular_capped`]). `Ok(None)`: file
/// missing. `Err`: reason without content (larger, a link, a folder, a FIFO, a device).
pub(crate) fn read_limited(path: &Path, max: usize) -> Result<Option<Vec<u8>>, String> {
    match read_regular_capped(path, max) {
        Ok(Some(bytes)) => Ok(Some(bytes)),
        Ok(None) => Err(format!("larger than {max} bytes")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Writes to a unique temporary file in the same folder first, then renames it.
/// `rename` replaces the target on all platforms; within the same folder it is atomic (Unix) or
/// a single rename operation (Windows). Concurrent calls do not interfere thanks to unique
/// names; the one renamed last wins.
pub(crate) fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path without folder or file name",
        ));
    };
    create_private_dir(dir)?;
    let temp = dir.join(format!(
        ".{}.{}.{}.{TEMP_EXT}",
        name.to_string_lossy(),
        std::process::id(),
        TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = create_private_file(&temp)?;
        file.write_all(data)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub(crate) fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

pub(crate) fn create_private_file(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    /// Counts what the caller pulled, to prove the limit holds whatever the source holds.
    struct Counting<R> {
        inner: R,
        read: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl<R: io::Read> io::Read for Counting<R> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let n = self.inner.read(buf)?;
            self.read.set(self.read.get() + n);
            Ok(n)
        }
    }

    #[test]
    fn capped_reads_stop_one_byte_past_the_limit() {
        let read = std::rc::Rc::new(std::cell::Cell::new(0));
        let endless = Counting {
            inner: io::repeat(b'x'),
            read: read.clone(),
        };
        assert_eq!(read_up_to(endless, 4096).unwrap(), None);
        assert_eq!(read.get(), 4097);
        assert_eq!(
            read_up_to(&b"exact"[..], 5).unwrap().as_deref(),
            Some(&b"exact"[..])
        );
        assert_eq!(read_up_to(&b"longer"[..], 5).unwrap(), None);
    }

    #[test]
    fn read_limited_refuses_large_files_folders_and_links() {
        let dir = TempDir::new("persist-read-limited");
        let big = dir.path().join(".git");
        fs::write(&big, vec![b'x'; 10 * 1024 * 1024]).unwrap();
        assert!(read_limited(&big, 4096).is_err());
        assert_eq!(read_regular_capped(&big, 4096).unwrap(), None);
        assert_eq!(read_limited(&dir.path().join("missing"), 64), Ok(None));
        assert!(read_limited(dir.path(), 64).is_err());
        let small = dir.path().join("HEAD");
        fs::write(&small, "ref: refs/heads/main\n").unwrap();
        assert_eq!(
            read_limited(&small, 64).unwrap().as_deref(),
            Some(&b"ref: refs/heads/main\n"[..])
        );
        let link = dir.path().join("link");
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(&small, &link);
        // File symbolic links need Developer Mode or an elevated process on Windows.
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_file(&small, &link);
        match made {
            Ok(()) => {
                assert!(read_limited(&link, 64).is_err(), "a link was followed");
                // Refused as no regular file on every platform, not as a failed read.
                let refused = open_regular(&link).map(drop);
                assert_eq!(
                    refused.as_ref().map_err(io::Error::kind),
                    Err(io::ErrorKind::InvalidInput),
                    "{refused:?}"
                );
            }
            Err(error) => eprintln!("symbolic link not possible here, link case skipped: {error}"),
        }
    }

    /// Placeholders and other reparse points that name no other path are read through their
    /// filter; links are not.
    #[cfg(windows)]
    #[test]
    fn only_reparse_points_that_name_no_path_are_read_through() {
        use windows_reparse::names_another_path;
        assert!(names_another_path(0xA000_000C), "symbolic link");
        assert!(names_another_path(0xA000_0003), "junction or mount point");
        assert!(!names_another_path(0x9000_001A), "OneDrive placeholder");
        assert!(!names_another_path(0x9000_401A), "OneDrive placeholder");
        assert!(!names_another_path(0x8000_0013), "deduplicated file");
        assert!(!names_another_path(0x8000_0017), "compressed system file");

        use windows_reparse::supplies_content;
        for tag in [
            0x9000_001A,
            0x9000_401A,
            0x9000_F01A,
            0x8000_0013,
            0x8000_0017,
        ] {
            assert!(supplies_content(tag), "{tag:#x}");
        }
        // Links, an app execution alias, a Unix socket and an unknown tag are refused.
        for tag in [
            0xA000_000C,
            0xA000_0003,
            0x8000_001B,
            0x8000_0023,
            0x0000_0001,
        ] {
            assert!(!supplies_content(tag), "{tag:#x}");
        }
    }

    /// The second open goes through the handle already open: same file, readable content.
    #[cfg(windows)]
    #[test]
    fn reopening_through_the_handle_reads_the_same_file() {
        use std::io::Read as _;
        let dir = TempDir::new("persist-reopen");
        let path = dir.path().join("plain.txt");
        fs::write(&path, "same content").unwrap();
        let first = open_regular(&path).unwrap();
        let mut again = windows_reparse::reopen(&first).unwrap();
        let mut text = String::new();
        again.read_to_string(&mut text).unwrap();
        assert_eq!(text, "same content");
        // Replaced by name after the first open: the reopen still reads the first file.
        let first = open_regular(&path).unwrap();
        fs::rename(&path, dir.path().join("moved.txt")).unwrap();
        fs::write(&path, "swapped in").unwrap();
        let mut again = windows_reparse::reopen(&first).unwrap();
        let mut text = String::new();
        again.read_to_string(&mut text).unwrap();
        assert_eq!(text, "same content");
    }

    /// Runs `read` on a thread; a read that blocks fails the test instead of hanging it.
    #[cfg(unix)]
    fn finishes_quickly(path: PathBuf) -> Result<Option<Vec<u8>>, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(read_limited(&path, 4096));
        });
        rx.recv_timeout(std::time::Duration::from_secs(2))
            .expect("the read blocked")
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_or_a_link_to_a_device_is_refused_at_once() {
        use std::os::unix::ffi::OsStrExt;
        let dir = TempDir::new("persist-read-fifo");
        let fifo = dir.path().join("HEAD");
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: `name` is a valid NUL-terminated path that lives for the call.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(finishes_quickly(fifo).is_err());
        let zero = dir.path().join("commondir");
        std::os::unix::fs::symlink("/dev/zero", &zero).unwrap();
        assert!(finishes_quickly(zero).is_err());
        // Not even the device itself, named directly.
        assert!(finishes_quickly(PathBuf::from("/dev/zero")).is_err());
    }

    const KEY: &str = "0f8fad5b-d9cb-469f-a165-70867728950e";
    const KEY2: &str = "7c9e6679-7425-40de-944b-e07fc1f90ae7";

    /// The folder is removed when the [`TempDir`] drops.
    struct TempStore(Store, #[allow(dead_code)] TempDir);

    impl TempStore {
        fn new(name: &str) -> Self {
            let dir = TempDir::new(&format!("persist-{name}"));
            Self(Store::new(dir.path().to_path_buf()), dir)
        }
    }

    fn session(key: &str, order: u32) -> SavedSession {
        SavedSession {
            key: key.into(),
            profile_id: "claude".into(),
            cwd: PathBuf::from(r"C:\Users\Test User\project"),
            title: "Claude Code".into(),
            order,
            color: None,
            emoji: None,
            worktree: None,
            cli_session_id: None,
            sub_agent: None,
            queued_message: None,
            model: None,
        }
    }

    /// Absolute on every platform, in the shape `worktree::create` produces.
    fn sample_worktree() -> Worktree {
        let (root, base) = if cfg!(windows) {
            (
                PathBuf::from(r"C:\Users\Test User\repo"),
                PathBuf::from(r"C:\Users\Test User\AppData\Local\x\worktrees"),
            )
        } else {
            (
                PathBuf::from("/home/test user/repo"),
                PathBuf::from("/home/test user/.local/share/x/worktrees"),
            )
        };
        Worktree {
            path: base
                .join(crate::worktree::repo_dir_name(&root))
                .join("fix-1a2b3c"),
            branch: "hangar/fix-1a2b3c".into(),
            repo_root: root,
        }
    }

    fn workspace() -> Workspace {
        Workspace {
            version: WORKSPACE_VERSION,
            sessions: vec![session(KEY, 0), session(KEY2, 1)],
            active_key: Some(KEY2.into()),
        }
    }

    #[test]
    fn the_model_round_trips_and_is_checked() {
        use crate::start_model::{ModelSource, StartModel};
        let store = TempStore::new("model");
        let mut workspace = workspace();
        workspace.sessions[0].model = Some(StartModel {
            source: ModelSource::Ollama,
            id: "qwen3:8b".into(),
            effort: None,
        });
        store.0.save_workspace(&workspace).unwrap();
        assert_eq!(store.0.load_workspace(), Some(workspace.clone()));
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(store.0.dir().join(WORKSPACE_FILE)).unwrap())
                .unwrap();
        assert_eq!(
            json["sessions"][0]["model"],
            serde_json::json!({ "source": "ollama", "id": "qwen3:8b" })
        );
        // Without a model the field is left out, so an older Hangar still reads the file.
        assert!(json["sessions"][1].get("model").is_none());
        // A reasoning effort is kept with the model; without one the field is left out.
        workspace.sessions[1].model = Some(StartModel {
            source: ModelSource::Provider,
            id: "opus".into(),
            effort: Some("high".into()),
        });
        store.0.save_workspace(&workspace).unwrap();
        assert_eq!(store.0.load_workspace(), Some(workspace.clone()));
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(store.0.dir().join(WORKSPACE_FILE)).unwrap())
                .unwrap();
        assert_eq!(
            json["sessions"][1]["model"],
            serde_json::json!({ "source": "provider", "id": "opus", "effort": "high" })
        );
        assert!(json["sessions"][0]["model"].get("effort").is_none());
    }

    #[test]
    fn an_invalid_model_costs_only_the_model() {
        let store = TempStore::new("model-invalid");
        let mut workspace = workspace();
        workspace.sessions[0].model = Some(crate::start_model::StartModel {
            source: crate::start_model::ModelSource::Ollama,
            id: "qwen3:8b".into(),
            effort: None,
        });
        store.0.save_workspace(&workspace).unwrap();
        let path = store.0.dir().join(WORKSPACE_FILE);
        let mut json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        // A bad id, a source this Hangar does not know, an extra field, a wrong type.
        for bad in [
            serde_json::json!({ "source": "provider", "id": "-rf" }),
            serde_json::json!({ "source": "cloud", "id": "x" }),
            serde_json::json!({ "source": "ollama", "id": "x", "extra": 1 }),
            serde_json::json!("opus"),
        ] {
            json["sessions"][0]["model"] = bad;
            json["sessions"][1]["model"] = serde_json::Value::Null;
            std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
            let loaded = store
                .0
                .load_workspace()
                .expect("the workspace stays readable");
            assert_eq!(loaded.sessions.len(), workspace.sessions.len());
            assert_eq!(loaded.sessions[0].model, None);
            assert_eq!(loaded.sessions[0].key, workspace.sessions[0].key);
            assert_eq!(loaded.sessions[1].model, None);
        }
    }

    #[test]
    fn sub_agent_names_are_checked() {
        let mut workspace = workspace();
        workspace.sessions[0].sub_agent = Some("backend".into());
        assert!(validate(&workspace).is_ok());
        workspace.sessions[0].sub_agent = Some("a & b".into());
        assert!(matches!(
            validate(&workspace),
            Err(PersistError::InvalidWorkspace(_))
        ));
    }

    #[test]
    fn key_must_be_uuid() {
        assert_eq!(normalize_key(KEY).unwrap(), KEY);
        assert_eq!(normalize_key(&KEY.to_uppercase()).unwrap(), KEY);
        for bad in [
            "",
            "abc",
            "../../etc/passwd",
            "..\\..\\x",
            "C:\\x",
            "0f8fad5b-d9cb-469f-a165-70867728950",
            "0f8fad5b-d9cb-469f-a165-70867728950e0",
            "0f8fad5b/d9cb-469f-a165-70867728950e",
            "0f8fad5b-d9cb-469f-a165-70867728950g",
            "0f8fad5bxd9cbx469fxa165x70867728950e",
            "0f8fad5b-d9cb-469f-a165-7086772895.e",
            " 0f8fad5b-d9cb-469f-a165-70867728950",
        ] {
            assert!(
                matches!(normalize_key(bad), Err(PersistError::InvalidKey)),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn workspace_round_trip_in_contract_format() {
        let store = TempStore::new("ws");
        assert_eq!(store.0.load_workspace(), None);
        store.0.save_workspace(&workspace()).unwrap();
        assert_eq!(store.0.load_workspace(), Some(workspace()));

        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(store.0.dir().join(WORKSPACE_FILE)).unwrap()).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["activeKey"], KEY2);
        assert_eq!(json["sessions"][0]["key"], KEY);
        assert_eq!(json["sessions"][0]["profileId"], "claude");
        assert_eq!(json["sessions"][1]["order"], 1);
        assert!(json["sessions"][0]["cwd"].is_string());
        assert!(json["sessions"][0]["title"].is_string());

        // Overwriting replaces completely and leaves no temporary files behind.
        let mut smaller = workspace();
        smaller.sessions.pop();
        smaller.active_key = None;
        store.0.save_workspace(&smaller).unwrap();
        assert_eq!(store.0.load_workspace(), Some(smaller));
        let names: Vec<_> = fs::read_dir(store.0.dir())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [WORKSPACE_FILE]);
    }

    #[test]
    fn invalid_workspaces_are_not_saved() {
        let store = TempStore::new("ws-invalid");
        let mut cases = Vec::new();
        let mut w = workspace();
        w.version = 2;
        cases.push(w);
        let mut w = workspace();
        w.sessions[1].key = KEY.to_uppercase();
        cases.push(w);
        let mut w = workspace();
        w.sessions[0].key = "../x".into();
        cases.push(w);
        let mut w = workspace();
        w.active_key = Some("7c9e6679-7425-40de-944b-000000000000".into());
        cases.push(w);
        let mut w = workspace();
        w.sessions[0].profile_id = " ".into();
        cases.push(w);
        for id in ["", "x", "../0f8fad5b-d9cb-469f-a165-70867728950e"] {
            let mut w = workspace();
            w.sessions[0].cli_session_id = Some(id.into());
            cases.push(w);
        }
        let mut w = workspace();
        w.sessions[0].cli_session_id = Some(KEY.into());
        w.sessions[1].cli_session_id = Some(KEY.to_uppercase());
        cases.push(w);
        for w in cases {
            assert!(
                matches!(
                    store.0.save_workspace(&w),
                    Err(PersistError::InvalidWorkspace(_))
                ),
                "{w:?}"
            );
        }
        assert!(!store.0.dir().join(WORKSPACE_FILE).exists());
    }

    #[test]
    fn markers_and_worktree_round_trip() {
        let store = TempStore::new("ws-markers");
        let mut w = workspace();
        w.sessions[0].color = Some("#1a2B3c".into());
        w.sessions[0].emoji = Some("\u{1F468}\u{200D}\u{1F4BB}".into());
        w.sessions[0].worktree = Some(sample_worktree());
        w.sessions[0].cli_session_id = Some(KEY2.into());
        store.0.save_workspace(&w).unwrap();
        assert_eq!(store.0.load_workspace(), Some(w));
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(store.0.dir().join(WORKSPACE_FILE)).unwrap()).unwrap();
        assert_eq!(json["sessions"][0]["color"], "#1a2B3c");
        assert_eq!(
            json["sessions"][0]["worktree"]["branch"],
            "hangar/fix-1a2b3c"
        );
        assert!(json["sessions"][0]["worktree"]["repoRoot"].is_string());
        assert!(json["sessions"][1]["color"].is_null());
        assert!(json["sessions"][1]["worktree"].is_null());
        assert_eq!(json["sessions"][0]["cliSessionId"], KEY2);
        assert!(json["sessions"][1]["cliSessionId"].is_null());
    }

    #[test]
    fn old_files_without_markers_still_load() {
        let store = TempStore::new("ws-old");
        fs::create_dir_all(store.0.dir()).unwrap();
        let old = format!(
            r#"{{"version":1,"sessions":[
                {{"key":"{KEY}","profileId":"claude","cwd":"c","title":"t","order":0}},
                {{"key":"{KEY2}","profileId":"claude","cwd":"c","title":"t","order":1,
                  "color":null,"emoji":null,"worktree":null}}]}}"#
        );
        fs::write(store.0.dir().join(WORKSPACE_FILE), old).unwrap();
        let loaded = store.0.load_workspace().unwrap();
        assert_eq!(loaded.sessions.len(), 2);
        assert!(loaded.sessions.iter().all(|s| s.color.is_none()
            && s.emoji.is_none()
            && s.worktree.is_none()
            && s.cli_session_id.is_none()
            && s.queued_message.is_none()));
    }

    #[test]
    fn queued_messages_round_trip_and_stay_out_when_absent() {
        let store = TempStore::new("ws-queued");
        let mut w = workspace();
        w.sessions[0].queued_message = Some("run the tests\nthen commit \u{1F680}".into());
        store.0.save_workspace(&w).unwrap();
        assert_eq!(store.0.load_workspace(), Some(w));
        let json: serde_json::Value =
            serde_json::from_slice(&fs::read(store.0.dir().join(WORKSPACE_FILE)).unwrap()).unwrap();
        assert_eq!(
            json["sessions"][0]["queuedMessage"],
            "run the tests\nthen commit \u{1F680}"
        );
        // Without a message the file looks as before, so an older Hangar still reads it.
        assert!(json["sessions"][1].get("queuedMessage").is_none());
    }

    #[test]
    fn queued_message_limit_holds_on_saving_and_loading() {
        let store = TempStore::new("ws-queued-limit");
        let at_limit = "a".repeat(MAX_QUEUED_MESSAGE_UNITS);
        // An astral character is two UTF-16 units, as the editor counts it.
        let over_in_units = "\u{1F600}".repeat(MAX_QUEUED_MESSAGE_UNITS / 2 + 1);
        let mut w = workspace();
        w.sessions[0].queued_message = Some(at_limit);
        assert!(validate(&w).is_ok());
        for text in [
            String::new(),
            " \n\t".into(),
            "a".repeat(MAX_QUEUED_MESSAGE_UNITS + 1),
            over_in_units,
        ] {
            let mut w = workspace();
            w.sessions[0].queued_message = Some(text);
            assert!(matches!(
                store.0.save_workspace(&w),
                Err(PersistError::InvalidWorkspace(_))
            ));
        }
        assert!(!store.0.dir().join(WORKSPACE_FILE).exists());

        // A file written by hand with a message over the limit is not loaded either.
        fs::create_dir_all(store.0.dir()).unwrap();
        let long = "a".repeat(MAX_QUEUED_MESSAGE_UNITS + 1);
        let file = format!(
            r#"{{"version":1,"sessions":[{{"key":"{KEY}","profileId":"claude","cwd":"c",
                "title":"t","order":0,"queuedMessage":"{long}"}}]}}"#
        );
        fs::write(store.0.dir().join(WORKSPACE_FILE), file).unwrap();
        assert_eq!(store.0.load_workspace(), None);
    }

    #[test]
    fn the_largest_valid_workspace_fits_the_file_limit() {
        // Control characters are escaped to six bytes each, the worst case for the file size.
        let text = "\u{1}".repeat(MAX_QUEUED_MESSAGE_UNITS);
        let sessions = (0..MAX_SESSIONS)
            .map(|i| {
                let mut s = session(&format!("00000000-0000-4000-8000-{i:012x}"), i as u32);
                s.queued_message = Some(format!("x{}", &text[1..]));
                s
            })
            .collect();
        let w = Workspace {
            version: WORKSPACE_VERSION,
            sessions,
            active_key: None,
        };
        assert!(validate(&w).is_ok());
        assert!(serde_json::to_vec_pretty(&w).unwrap().len() <= MAX_WORKSPACE_BYTES);
    }

    #[test]
    fn invalid_markers_are_not_saved() {
        let store = TempStore::new("ws-markers-invalid");
        let mut cases = Vec::new();
        for color in ["", "red", "#12345", "#1234567", "#12345g", "123456#"] {
            let mut w = workspace();
            w.sessions[0].color = Some(color.into());
            cases.push(w);
        }
        for emoji in ["", "\u{1F600}\u{1F600}\u{1F600}\u{1F600}\u{1F600}", "a\nb"] {
            let mut w = workspace();
            w.sessions[0].emoji = Some(emoji.into());
            cases.push(w);
        }
        let good = sample_worktree();
        let variants = [
            Worktree {
                branch: "hangar/other-1a2b3c".into(),
                ..good.clone()
            },
            Worktree {
                branch: " ".into(),
                ..good.clone()
            },
            Worktree {
                path: "".into(),
                ..good.clone()
            },
            Worktree {
                repo_root: "".into(),
                ..good.clone()
            },
            Worktree {
                repo_root: good.repo_root.join("elsewhere"),
                ..good
            },
        ];
        for worktree in variants {
            let mut w = workspace();
            w.sessions[0].worktree = Some(worktree);
            cases.push(w);
        }
        for w in cases {
            assert!(
                matches!(
                    store.0.save_workspace(&w),
                    Err(PersistError::InvalidWorkspace(_))
                ),
                "{w:?}"
            );
        }
        // Exactly 16 bytes is allowed.
        let mut w = workspace();
        w.sessions[0].emoji = Some("\u{1F600}\u{1F600}\u{1F600}\u{1F600}".into());
        store.0.save_workspace(&w).unwrap();
    }

    #[test]
    fn workspace_keys_tell_missing_from_unreadable() {
        let store = TempStore::new("ws-keys");
        assert_eq!(store.0.workspace_keys(), Some(Default::default()));
        fs::create_dir_all(store.0.dir()).unwrap();
        let path = store.0.dir().join(WORKSPACE_FILE);
        fs::write(&path, "{").unwrap();
        assert_eq!(store.0.workspace_keys(), None);
        let key = "0F8FAD5B-D9CB-469F-A165-70867728950E";
        fs::write(
            &path,
            format!(
                r#"{{"version":1,"sessions":[{{"key":"{key}","profileId":"a","cwd":"c","title":"t","order":0}}]}}"#
            ),
        )
        .unwrap();
        assert_eq!(
            store.0.workspace_keys(),
            Some(std::collections::BTreeSet::from([key.to_ascii_lowercase()]))
        );
    }

    #[test]
    fn broken_file_means_no_workspace() {
        let store = TempStore::new("ws-broken");
        fs::create_dir_all(store.0.dir()).unwrap();
        let path = store.0.dir().join(WORKSPACE_FILE);
        for content in [
            "",
            "{",
            "not json",
            r#"{"version":2,"sessions":[]}"#,
            r#"{"version":1,"sessions":[],"unknown":1}"#,
            r#"{"version":1,"sessions":[{"key":"x","profileId":"a","cwd":"c","title":"t","order":0}]}"#,
        ] {
            fs::write(&path, content).unwrap();
            assert_eq!(store.0.load_workspace(), None, "{content:?}");
        }
        fs::write(&path, r#"{"version":1,"sessions":[]}"#).unwrap();
        assert_eq!(
            store.0.load_workspace(),
            Some(Workspace {
                version: 1,
                sessions: Vec::new(),
                active_key: None
            })
        );
    }

    #[test]
    fn scrollback_save_load_delete() {
        let store = TempStore::new("sb");
        assert_eq!(store.0.load_scrollback(KEY).unwrap(), None);
        let text = "PS C:\\> echo äöü 😀\r\n\x1b[31mred\x1b[0m\r\n";
        store.0.save_scrollback(KEY, text).unwrap();
        assert_eq!(store.0.load_scrollback(KEY).unwrap().as_deref(), Some(text));
        // An upper-case key hits the same file.
        assert_eq!(
            store
                .0
                .load_scrollback(&KEY.to_uppercase())
                .unwrap()
                .as_deref(),
            Some(text)
        );
        store.0.save_scrollback(KEY, "new").unwrap();
        assert_eq!(
            store.0.load_scrollback(KEY).unwrap().as_deref(),
            Some("new")
        );
        store.0.delete_scrollback(KEY).unwrap();
        assert_eq!(store.0.load_scrollback(KEY).unwrap(), None);
        // Deleting twice is not an error.
        store.0.delete_scrollback(KEY).unwrap();
    }

    #[test]
    fn scrollback_key_and_size_are_checked() {
        let store = TempStore::new("sb-limits");
        for bad in ["../workspace", "..", "a/b", ""] {
            assert!(matches!(
                store.0.save_scrollback(bad, "x"),
                Err(PersistError::InvalidKey)
            ));
            assert!(matches!(
                store.0.load_scrollback(bad),
                Err(PersistError::InvalidKey)
            ));
            assert!(matches!(
                store.0.delete_scrollback(bad),
                Err(PersistError::InvalidKey)
            ));
        }
        let max = "x".repeat(MAX_SCROLLBACK_BYTES);
        store.0.save_scrollback(KEY, &max).unwrap();
        assert_eq!(
            store.0.load_scrollback(KEY).unwrap().map(|s| s.len()),
            Some(MAX_SCROLLBACK_BYTES)
        );
        let too_big = "x".repeat(MAX_SCROLLBACK_BYTES + 1);
        assert!(matches!(
            store.0.save_scrollback(KEY, &too_big),
            Err(PersistError::TooLarge { .. })
        ));
        // The old state survives the rejection.
        assert_eq!(
            store.0.load_scrollback(KEY).unwrap().map(|s| s.len()),
            Some(MAX_SCROLLBACK_BYTES)
        );
        // Files enlarged from outside or broken files are ignored, not loaded.
        let path = store.0.scrollback_path(KEY).unwrap();
        fs::write(&path, &too_big).unwrap();
        assert_eq!(store.0.load_scrollback(KEY).unwrap(), None);
        fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(store.0.load_scrollback(KEY).unwrap(), None);
    }

    #[test]
    fn clear_scrollbacks_deletes_only_own_files() {
        let store = TempStore::new("sb-clear");
        store.0.clear_scrollbacks().unwrap();
        store.0.save_scrollback(KEY, "a").unwrap();
        store.0.save_scrollback(KEY2, "b").unwrap();
        let dir = store.0.dir().join(SCROLLBACK_DIR);
        fs::write(dir.join(format!(".{KEY}.txt.1.2.tmp")), "rest").unwrap();
        fs::write(dir.join("foreign.txt"), "stays").unwrap();
        store.0.save_workspace(&workspace()).unwrap();
        store.0.clear_scrollbacks().unwrap();
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(names, ["foreign.txt"]);
        // The workspace stays.
        assert!(store.0.load_workspace().is_some());
    }

    /// Keys an agent printed do not reach the disk: every shape the secret guard knows is masked
    /// in place, the terminal sequences around it and the rest of the text stay as they were.
    #[test]
    fn printed_keys_are_masked_before_the_scrollback_is_saved() {
        let store = TempStore::new("sb-masked");
        let (hits, _) = crate::secret_scan::tests::vectors();
        for vector in hits {
            let text = format!(
                "\x1b[32m$ cat .env\x1b[0m\r\n{}\r\nhangar-test-marker",
                vector.text
            );
            store.0.save_scrollback(KEY, &text).unwrap();
            let on_disk = fs::read_to_string(store.0.scrollback_path(KEY).unwrap()).unwrap();
            assert_eq!(on_disk.len(), text.len(), "{}", vector.name);
            assert!(
                crate::secret_scan::scan(&on_disk).is_empty(),
                "{}",
                vector.name
            );
            assert!(on_disk.starts_with("\x1b[32m$ cat .env\x1b[0m\r\n"));
            assert!(on_disk.ends_with("\r\nhangar-test-marker"));
            assert_eq!(
                store.0.load_scrollback(KEY).unwrap().as_deref(),
                Some(on_disk.as_str())
            );
        }
        // Text without a key is stored byte for byte.
        let (_, misses) = crate::secret_scan::tests::vectors();
        for vector in misses {
            store.0.save_scrollback(KEY, &vector.text).unwrap();
            assert_eq!(
                store.0.load_scrollback(KEY).unwrap().as_deref(),
                Some(vector.text.as_str()),
                "{}",
                vector.name
            );
        }
    }

    #[test]
    fn errors_contain_no_content() {
        let secret = "sk-secret-key";
        let too_big = format!("{secret}{}", "x".repeat(MAX_SCROLLBACK_BYTES));
        let store = TempStore::new("sb-secret");
        let error = store.0.save_scrollback(KEY, &too_big).unwrap_err();
        assert!(!error.to_string().contains(secret));
        let json = serde_json::to_string(&error).unwrap();
        assert!(!json.contains(secret));
    }

    #[cfg(unix)]
    #[test]
    fn unix_permissions_only_for_owner() {
        use std::os::unix::fs::PermissionsExt;
        let store = TempStore::new("sb-mode");
        store.0.save_scrollback(KEY, "x").unwrap();
        store.0.save_workspace(&workspace()).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&store.0.scrollback_path(KEY).unwrap()), 0o600);
        assert_eq!(mode(&store.0.dir().join(WORKSPACE_FILE)), 0o600);
        assert_eq!(mode(&store.0.dir().join(SCROLLBACK_DIR)), 0o700);
    }

    #[test]
    fn concurrent_saves_leave_a_whole_file() {
        let store = TempStore::new("sb-race");
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let store = store.0.clone();
                std::thread::spawn(move || {
                    let text = i.to_string().repeat(10_000);
                    for _ in 0..5 {
                        // On Windows rename may fail while another thread is replacing the target
                        // at that moment; the previous complete state then remains.
                        let _ = store.save_scrollback(KEY, &text);
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let text = store.0.load_scrollback(KEY).unwrap().unwrap();
        assert_eq!(text.len(), 10_000);
        let first = text.chars().next().unwrap();
        assert!(text.chars().all(|c| c == first));
    }
}
