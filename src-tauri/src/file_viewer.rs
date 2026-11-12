//! Read-only file viewer windows (contract: docs/wiki/pages/syntheses/file-viewer.md).
//!
//! Only the backend creates them, with a label `file-viewer-<n>`, a fixed app URL and a fixed
//! size, like the notes window. Each window is bound to the one file it was opened for: the
//! frontend of a viewer names no path at all, the backend looks up the calling window's label in
//! [`Viewers`] and reads that file. A viewer therefore cannot read anything else, even with a
//! compromised page.
//!
//! The file is found with the file tree's rules ([`crate::file_tree::segments`]): a `/` separated
//! path below the session's folder, no `..`, no `.git`, no link anywhere on the way, and the
//! resolved path must still lie below the resolved root. Every read resolves it again, so a file
//! swapped for a link after the window opened is refused.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Serialize, Serializer};

use crate::file_tree::{self, FileTreeError};
use crate::lock;
use crate::session::SessionId;

/// Label prefix of every viewer window; the capability `file-viewer` matches `file-viewer-*`.
pub const LABEL_PREFIX: &str = "file-viewer-";
/// Relative to the app URL; the frontend renders the viewer for `?window=file`.
pub const URL: &str = "index.html?window=file";
pub const WIDTH: f64 = 960.0;
pub const HEIGHT: f64 = 720.0;
pub const MIN_WIDTH: f64 = 480.0;
pub const MIN_HEIGHT: f64 = 320.0;
/// Bytes of a file handed to the viewer; the rest is cut and `truncated` set.
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
/// A NUL byte in this many leading bytes marks the file as binary (as git and grep decide).
pub const BINARY_PROBE_BYTES: usize = 8 * 1024;
/// A relative path longer than this is refused before touching the file system.
const MAX_RELATIVE_BYTES: usize = 4096;
const UTF8_BOM: &[u8] = b"\xEF\xBB\xBF";

#[derive(Debug, thiserror::Error)]
pub enum FileViewerError {
    #[error("Unknown session {0}")]
    UnknownSession(SessionId),
    #[error("Invalid file path: {0}")]
    InvalidPath(&'static str),
    #[error("The file lies outside the session's folder")]
    OutsideRoot,
    #[error("Links are not followed in the file viewer")]
    Link,
    #[error("The file no longer exists")]
    NotFound,
    #[error("Not a file")]
    NotAFile,
    #[error("Only the main window may open a file viewer")]
    NotMainWindow,
    #[error("Only a file viewer window may read its file")]
    WrongWindow,
    #[error("Unknown file viewer window")]
    UnknownWindow,
    #[error("The file cannot be read: {0}")]
    Io(#[source] io::Error),
    #[error("Background task aborted: {0}")]
    Task(String),
    #[error("The file did not answer in time")]
    TimedOut,
    #[error("File viewer: {0}")]
    Tauri(#[from] tauri::Error),
}

impl Serialize for FileViewerError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// The file a viewer window shows. Kept by the backend only; the window never names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub session_id: SessionId,
    /// The session's folder, resolved.
    pub root: PathBuf,
    /// The file, resolved, below `root`.
    pub path: PathBuf,
    /// As the file tree names it: `/` separated, below `root`.
    pub relative: String,
}

impl Target {
    /// File name for the window title; the relative path if it has none.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.relative.clone())
    }
}

/// What the command `file_viewer_content` hands to the viewer window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileContent {
    pub name: String,
    /// Absolute, for display only, without a `\\?\` prefix.
    pub path: String,
    pub relative_path: String,
    /// Size on disk in bytes, also when only part of it was read.
    pub size: u64,
    /// `None` for a binary file.
    pub text: Option<String>,
    /// Larger than [`MAX_BYTES`]; `text` holds the first part, cut on a character boundary.
    pub truncated: bool,
    pub binary: bool,
}

/// Only the main window opens viewers.
pub fn check_opener(label: &str) -> Result<(), FileViewerError> {
    if label == crate::notes_window::MAIN_LABEL {
        Ok(())
    } else {
        Err(FileViewerError::NotMainWindow)
    }
}

/// Only windows labelled `file-viewer-<n>` read content.
pub fn check_sender(label: &str) -> Result<(), FileViewerError> {
    match label.strip_prefix(LABEL_PREFIX) {
        Some(n) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => Ok(()),
        _ => Err(FileViewerError::WrongWindow),
    }
}

/// Resolves `relative` below `root` to a regular file with the file tree's rules; returns the
/// resolved root and file.
pub fn resolve_file(root: &Path, relative: &str) -> Result<(PathBuf, PathBuf), FileViewerError> {
    if relative.len() > MAX_RELATIVE_BYTES {
        return Err(FileViewerError::InvalidPath("path too long"));
    }
    let parts = file_tree::segments(relative).map_err(tree_error)?;
    let Some((last, folders)) = parts.split_last() else {
        return Err(FileViewerError::NotAFile);
    };
    let root = fs::canonicalize(root).map_err(FileViewerError::Io)?;
    if !root.is_dir() {
        return Err(FileViewerError::Io(io::Error::new(
            io::ErrorKind::NotADirectory,
            "the session's folder is not a folder",
        )));
    }
    let mut path = root.clone();
    for part in folders {
        path.push(part);
        let meta = symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            return Err(FileViewerError::Link);
        }
        if !meta.is_dir() {
            return Err(FileViewerError::NotFound);
        }
    }
    path.push(last);
    let meta = symlink_metadata(&path)?;
    if meta.file_type().is_symlink() {
        return Err(FileViewerError::Link);
    }
    if !meta.is_file() {
        return Err(FileViewerError::NotAFile);
    }
    // No link was passed, so this only normalizes (case, short names) and guards against
    // anything the walk did not foresee.
    let path = fs::canonicalize(&path).map_err(not_found_or_io)?;
    let inside = path
        .strip_prefix(&root)
        .map_err(|_| FileViewerError::OutsideRoot)?;
    if file_tree::reaches_git_folder(inside) {
        return Err(FileViewerError::InvalidPath("git folder"));
    }
    Ok((root, path))
}

fn symlink_metadata(path: &Path) -> Result<fs::Metadata, FileViewerError> {
    fs::symlink_metadata(path).map_err(not_found_or_io)
}

fn not_found_or_io(error: io::Error) -> FileViewerError {
    match error.kind() {
        io::ErrorKind::NotFound => FileViewerError::NotFound,
        _ => FileViewerError::Io(error),
    }
}

fn tree_error(error: FileTreeError) -> FileViewerError {
    match error {
        FileTreeError::InvalidPath(reason) => FileViewerError::InvalidPath(reason),
        FileTreeError::OutsideRoot => FileViewerError::OutsideRoot,
        FileTreeError::Link => FileViewerError::Link,
        FileTreeError::Io(error) => FileViewerError::Io(error),
        other => FileViewerError::Task(other.to_string()),
    }
}

/// Resolves the target again and reads its content.
pub fn read_target(target: &Target) -> Result<FileContent, FileViewerError> {
    let (_, path) = resolve_file(&target.root, &target.relative)?;
    // Same text, different file: the folder was renamed or swapped under the session.
    if path != target.path {
        return Err(FileViewerError::NotFound);
    }
    let mut file = fs::File::open(&path).map_err(not_found_or_io)?;
    let meta = file.metadata().map_err(FileViewerError::Io)?;
    if !meta.is_file() {
        return Err(FileViewerError::NotAFile);
    }
    // One byte more than the limit tells a file that grew past it since `metadata`.
    let mut bytes = Vec::with_capacity(
        usize::try_from(meta.len())
            .unwrap_or(MAX_BYTES)
            .min(MAX_BYTES + 1),
    );
    file.by_ref()
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(FileViewerError::Io)?;
    let decoded = decode(&bytes);
    Ok(FileContent {
        name: target.name(),
        path: file_tree::display_path(&path),
        relative_path: target.relative.clone(),
        size: meta.len().max(bytes.len() as u64),
        text: decoded.text,
        truncated: decoded.truncated,
        binary: decoded.binary,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub struct Decoded {
    pub text: Option<String>,
    pub truncated: bool,
    pub binary: bool,
}

/// Applies the content rules to the bytes read (at most [`MAX_BYTES`] + 1): binary if a NUL
/// shows up in the first [`BINARY_PROBE_BYTES`], otherwise lossy UTF-8 without a BOM, line
/// endings untouched, cut to [`MAX_BYTES`] on a character boundary.
pub fn decode(bytes: &[u8]) -> Decoded {
    decode_limited(bytes, MAX_BYTES)
}

fn decode_limited(bytes: &[u8], limit: usize) -> Decoded {
    let truncated = bytes.len() > limit;
    let probe = &bytes[..bytes.len().min(BINARY_PROBE_BYTES)];
    if probe.contains(&0) {
        return Decoded {
            text: None,
            truncated,
            binary: true,
        };
    }
    let mut body = if truncated {
        cut_incomplete_char(&bytes[..limit])
    } else {
        bytes
    };
    body = body.strip_prefix(UTF8_BOM).unwrap_or(body);
    Decoded {
        text: Some(String::from_utf8_lossy(body).into_owned()),
        truncated,
        binary: false,
    }
}

/// Drops a UTF-8 sequence the cut left incomplete at the end, so the cut does not turn a valid
/// character into U+FFFD. Invalid bytes elsewhere are left for lossy decoding.
fn cut_incomplete_char(bytes: &[u8]) -> &[u8] {
    // A sequence is at most four bytes, so its lead byte is among the last four.
    for back in 1..=bytes.len().min(4) {
        let index = bytes.len() - back;
        let byte = bytes[index];
        if byte & 0b1100_0000 == 0b1000_0000 {
            continue;
        }
        let needed = match byte {
            0x00..=0x7F => 1,
            0xC0..=0xDF => 2,
            0xE0..=0xEF => 3,
            0xF0..=0xF7 => 4,
            _ => return bytes,
        };
        return if back < needed {
            &bytes[..index]
        } else {
            bytes
        };
    }
    bytes
}

/// Result of [`Viewers::claim`].
#[derive(Debug, PartialEq, Eq)]
pub enum Claim {
    /// A viewer for this file exists (or is being created) under this label.
    Existing(String),
    /// A new label, already registered; the caller creates the window or calls
    /// [`Viewers::remove`] if it cannot.
    New(String),
}

/// Open viewer windows: label to target.
#[derive(Clone, Default)]
pub struct Viewers(Arc<Mutex<ViewerState>>);

#[derive(Default)]
struct ViewerState {
    next: u64,
    targets: HashMap<String, Target>,
}

impl Viewers {
    /// The label of the viewer already showing `target`'s file in its session, or a new label
    /// registered for it. Check and registration happen under one lock, so two quick
    /// double-clicks open one window.
    pub fn claim(&self, target: Target) -> Claim {
        let mut state = lock(&self.0);
        let existing = state
            .targets
            .iter()
            .find(|(_, open)| open.session_id == target.session_id && open.path == target.path);
        if let Some((label, _)) = existing {
            return Claim::Existing(label.clone());
        }
        state.next += 1;
        let label = format!("{LABEL_PREFIX}{}", state.next);
        state.targets.insert(label.clone(), target);
        Claim::New(label)
    }

    pub fn target(&self, label: &str) -> Option<Target> {
        lock(&self.0).targets.get(label).cloned()
    }

    pub fn remove(&self, label: &str) {
        lock(&self.0).targets.remove(label);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn target_for(root: &Path, relative: &str) -> Target {
        let (root, path) = resolve_file(root, relative).unwrap();
        Target {
            session_id: 1,
            root,
            path,
            relative: relative.to_owned(),
        }
    }

    #[test]
    fn resolves_files_below_the_root() {
        let dir = TempDir::new("file-viewer-resolve");
        fs::create_dir_all(dir.path().join("src/a b")).unwrap();
        fs::write(dir.path().join("src/a b/main.rs"), "fn main() {}\n").unwrap();
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();

        let (root, path) = resolve_file(dir.path(), "src/a b/main.rs").unwrap();
        assert_eq!(root, fs::canonicalize(dir.path()).unwrap());
        assert!(path.starts_with(&root));
        assert!(path.ends_with("main.rs"));
        assert!(resolve_file(dir.path(), "README.md").is_ok());
    }

    #[test]
    fn escapes_and_odd_paths_are_refused() {
        let dir = TempDir::new("file-viewer-escape");
        fs::create_dir_all(dir.path().join("inner")).unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join("inner/x.txt"), "x").unwrap();
        fs::write(dir.path().join(".git/config"), "x").unwrap();
        let outside = dir.path().join("inner");

        for bad in [
            "../x.txt",
            "a/../x.txt",
            "./x.txt",
            "/etc/hosts",
            "C:/Windows/win.ini",
            "\\\\server\\share\\x",
            "inner\\x.txt",
            "x.txt:stream",
            ".git/config",
            "inner/",
            "a\0b",
        ] {
            assert!(
                matches!(
                    resolve_file(&outside, bad),
                    Err(FileViewerError::InvalidPath(_))
                ),
                "{bad:?} was accepted"
            );
        }
        let long = "a/".repeat(MAX_RELATIVE_BYTES) + "x";
        assert!(matches!(
            resolve_file(dir.path(), &long),
            Err(FileViewerError::InvalidPath(_))
        ));
    }

    #[test]
    fn folders_and_missing_files_are_refused() {
        let dir = TempDir::new("file-viewer-kind");
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("file.txt"), "x").unwrap();
        assert!(matches!(
            resolve_file(dir.path(), ""),
            Err(FileViewerError::NotAFile)
        ));
        assert!(matches!(
            resolve_file(dir.path(), "src"),
            Err(FileViewerError::NotAFile)
        ));
        assert!(matches!(
            resolve_file(dir.path(), "nope.txt"),
            Err(FileViewerError::NotFound)
        ));
        // A file used as a folder on the way.
        assert!(matches!(
            resolve_file(dir.path(), "file.txt/x"),
            Err(FileViewerError::NotFound)
        ));
    }

    #[cfg(unix)]
    fn link_file(target: &Path, link: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).is_ok()
    }

    /// File symlinks on Windows need the privilege or developer mode; a hard link would not be a
    /// link to the check, but it cannot cross volumes and needs write access to the target.
    #[cfg(windows)]
    fn link_file(target: &Path, link: &Path) -> bool {
        std::os::windows::fs::symlink_file(target, link).is_ok()
    }

    #[cfg(unix)]
    fn link_dir(target: &Path, link: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).is_ok()
    }

    #[cfg(windows)]
    fn link_dir(target: &Path, link: &Path) -> bool {
        std::os::windows::fs::symlink_dir(target, link).is_ok()
            || std::process::Command::new("cmd")
                .args(["/D", "/C", "mklink", "/J"])
                .arg(link)
                .arg(target)
                .stdout(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
    }

    #[test]
    fn links_do_not_lead_out() {
        let outside = TempDir::new("file-viewer-outside");
        fs::write(outside.path().join("other.txt"), "x").unwrap();
        let dir = TempDir::new("file-viewer-links");
        if !link_dir(outside.path(), &dir.path().join("out")) {
            eprintln!("folder links cannot be created here, skipping");
            return;
        }
        assert!(matches!(
            resolve_file(dir.path(), "out/other.txt"),
            Err(FileViewerError::Link)
        ));
        if link_file(&outside.path().join("other.txt"), &dir.path().join("f.txt")) {
            assert!(matches!(
                resolve_file(dir.path(), "f.txt"),
                Err(FileViewerError::Link)
            ));
        } else {
            eprintln!("file symlinks cannot be created here, skipping that part");
        }
    }

    #[test]
    fn reads_text_and_reports_size() {
        let dir = TempDir::new("file-viewer-read");
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            b"\xEF\xBB\xBFline 1\r\nline 2\n",
        )
        .unwrap();
        let target = target_for(dir.path(), "src/lib.rs");
        let content = read_target(&target).unwrap();
        assert_eq!(content.name, "lib.rs");
        assert_eq!(content.relative_path, "src/lib.rs");
        assert_eq!(content.size, 18);
        assert_eq!(content.text.as_deref(), Some("line 1\r\nline 2\n"));
        assert!(!content.truncated);
        assert!(!content.binary);
        assert!(!content.path.starts_with(r"\\?\"));

        // Re-read on every call: changes show up.
        fs::write(dir.path().join("src/lib.rs"), "changed").unwrap();
        assert_eq!(
            read_target(&target).unwrap().text.as_deref(),
            Some("changed")
        );

        fs::remove_file(dir.path().join("src/lib.rs")).unwrap();
        assert!(matches!(
            read_target(&target),
            Err(FileViewerError::NotFound)
        ));
        // Replaced by a folder.
        fs::create_dir(dir.path().join("src/lib.rs")).unwrap();
        assert!(matches!(
            read_target(&target),
            Err(FileViewerError::NotAFile)
        ));
    }

    #[test]
    fn large_files_are_cut_at_the_limit() {
        let dir = TempDir::new("file-viewer-large");
        let mut data = vec![b'a'; MAX_BYTES - 1];
        data.extend_from_slice("\u{e9}tail".as_bytes());
        fs::write(dir.path().join("big.txt"), &data).unwrap();
        let content = read_target(&target_for(dir.path(), "big.txt")).unwrap();
        assert!(content.truncated);
        assert_eq!(content.size, data.len() as u64);
        let text = content.text.unwrap();
        // The two byte character straddles the limit and is dropped whole.
        assert_eq!(text.len(), MAX_BYTES - 1);
        assert!(!text.contains('\u{FFFD}'));
    }

    #[test]
    fn binary_files_have_no_text() {
        let nul = decode(b"PK\x03\x04\x00\x00rest");
        assert_eq!(
            nul,
            Decoded {
                text: None,
                truncated: false,
                binary: true
            }
        );
        // A NUL past the probe does not make a file binary.
        let mut late = vec![b'x'; BINARY_PROBE_BYTES];
        late.push(0);
        let decoded = decode(&late);
        assert!(!decoded.binary);
        assert_eq!(decoded.text.map(|t| t.len()), Some(BINARY_PROBE_BYTES + 1));
        let big_binary = decode_limited(b"\x00abcdef", 3);
        assert!(big_binary.binary && big_binary.truncated && big_binary.text.is_none());
    }

    #[test]
    fn decoding_rules() {
        assert_eq!(decode(b"").text.as_deref(), Some(""));
        assert_eq!(decode(b"\xEF\xBB\xBFhi").text.as_deref(), Some("hi"));
        // Only one BOM is stripped; CRLF stays as it is.
        assert_eq!(
            decode(b"\xEF\xBB\xBF\xEF\xBB\xBFa\r\n").text.as_deref(),
            Some("\u{FEFF}a\r\n")
        );
        // Invalid bytes become U+FFFD instead of failing.
        assert_eq!(
            decode(b"ok \xFF\xFE end").text.as_deref(),
            Some("ok \u{FFFD}\u{FFFD} end")
        );
        // A cut through a character drops it whole; a complete one at the end stays.
        let snowman = "\u{2603}".as_bytes();
        for keep in 1..snowman.len() {
            let mut bytes = b"ab".to_vec();
            bytes.extend_from_slice(snowman);
            let decoded = decode_limited(&bytes, 2 + keep);
            assert!(decoded.truncated);
            assert_eq!(decoded.text.as_deref(), Some("ab"), "cut after {keep}");
        }
        let mut bytes = "ab\u{2603}".as_bytes().to_vec();
        bytes.push(b'z');
        let decoded = decode_limited(&bytes, 5);
        assert_eq!(decoded.text.as_deref(), Some("ab\u{2603}"));
        let four_bytes = "\u{1F600}".as_bytes();
        let decoded = decode_limited(&[four_bytes, b"x"].concat(), 3);
        assert_eq!(decoded.text.as_deref(), Some(""));
        // Invalid bytes at the cut are not mistaken for a character and stay for lossy decoding.
        assert_eq!(
            decode_limited(b"a\xFF\xFFb", 3).text.as_deref(),
            Some("a\u{FFFD}\u{FFFD}")
        );
    }

    #[test]
    fn only_viewer_windows_read_and_only_main_opens() {
        assert!(check_sender("file-viewer-1").is_ok());
        assert!(check_sender("file-viewer-42").is_ok());
        for bad in [
            "main",
            "notes",
            "file-viewer-",
            "file-viewer-x",
            "file-viewer-1a",
            "xfile-viewer-1",
        ] {
            assert!(
                matches!(check_sender(bad), Err(FileViewerError::WrongWindow)),
                "{bad}"
            );
        }
        assert!(check_opener("main").is_ok());
        assert!(matches!(
            check_opener("file-viewer-1"),
            Err(FileViewerError::NotMainWindow)
        ));
    }

    #[test]
    fn one_window_per_session_and_file() {
        let viewers = Viewers::default();
        let target = |session_id, path: &str| Target {
            session_id,
            root: PathBuf::from("r"),
            path: PathBuf::from(path),
            relative: path.to_owned(),
        };
        assert_eq!(
            viewers.claim(target(1, "a")),
            Claim::New("file-viewer-1".into())
        );
        assert_eq!(
            viewers.claim(target(1, "a")),
            Claim::Existing("file-viewer-1".into())
        );
        assert_eq!(
            viewers.claim(target(2, "a")),
            Claim::New("file-viewer-2".into())
        );
        assert_eq!(
            viewers.claim(target(1, "b")),
            Claim::New("file-viewer-3".into())
        );
        assert_eq!(viewers.target("file-viewer-3"), Some(target(1, "b")));
        viewers.remove("file-viewer-1");
        assert_eq!(viewers.target("file-viewer-1"), None);
        // Labels are never reused, so a late call of a closed window finds nothing.
        assert_eq!(
            viewers.claim(target(1, "a")),
            Claim::New("file-viewer-4".into())
        );
    }

    #[test]
    fn content_serializes_camel_case_and_errors_as_text() {
        let content = FileContent {
            name: "a.rs".into(),
            path: "C:\\p\\a.rs".into(),
            relative_path: "a.rs".into(),
            size: 3,
            text: None,
            truncated: false,
            binary: true,
        };
        let json = serde_json::to_value(content).unwrap();
        assert_eq!(json["relativePath"], "a.rs");
        assert!(json["text"].is_null());
        assert_eq!(json["binary"], true);
        let error = serde_json::to_value(FileViewerError::UnknownWindow).unwrap();
        assert_eq!(error, "Unknown file viewer window");
    }
}
