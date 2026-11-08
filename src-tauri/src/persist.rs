//! Workspace and scrollback across a restart of Hangar.
//!
//! Running processes do not survive closing the app (no background service). Only what the
//! frontend needs for restoring is stored:
//! - `workspace.json`: which sessions were open and which one was active ([`Workspace`])
//! - `scrollback/<key>.txt`: the scrollback per session, serialized by the frontend
//!
//! Writing is atomic (temporary file, `sync_all`, then `rename`), so a crash in the middle of
//! saving never leaves half a file behind. Contents are never logged: scrollback may contain
//! secrets (printed tokens, pasted keys).
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
pub const MAX_WORKSPACE_BYTES: usize = 1024 * 1024;
pub const MAX_SESSIONS: usize = 512;
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
}

#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    #[error("Invalid session key: expected a UUID")]
    InvalidKey,
    #[error("Scrollback too large: {size} bytes, at most {max} allowed")]
    TooLarge { size: usize, max: usize },
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
        if session
            .cli_session_id
            .as_deref()
            .is_some_and(|id| normalize_key(id).is_err())
        {
            return invalid("cliSessionId is not a UUID");
        }
        if let Some(worktree) = &session.worktree {
            if crate::worktree::check_shape(worktree).is_err() {
                return invalid("worktree is not one Hangar created");
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
fn is_hex_color(color: &str) -> bool {
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

    pub fn save_scrollback(&self, key: &str, data: &str) -> Result<(), PersistError> {
        let path = self.scrollback_path(key)?;
        if data.len() > MAX_SCROLLBACK_BYTES {
            return Err(PersistError::TooLarge {
                size: data.len(),
                max: MAX_SCROLLBACK_BYTES,
            });
        }
        write_atomic(&path, data.as_bytes()).map_err(io_error(&path))
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

/// Reads at most `max` bytes. `Ok(None)`: file missing. `Err`: reason without content.
pub(crate) fn read_limited(path: &Path, max: usize) -> Result<Option<Vec<u8>>, String> {
    let meta = match fs::metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if usize::try_from(meta.len()).map_or(true, |len| len > max) {
        return Err(format!("larger than {max} bytes"));
    }
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
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

fn create_private_file(path: &Path) -> io::Result<File> {
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
            && s.cli_session_id.is_none()));
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
