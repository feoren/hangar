//! Read-only file viewer windows (contract: docs/wiki/pages/syntheses/file-viewer.md).
//!
//! Only the backend creates them, with a label `file-viewer-<n>`, a fixed app URL and a fixed
//! size, like the side panel window. Each window is bound to the one file it was opened for: the
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
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, UNIX_EPOCH};

use serde::{Serialize, Serializer};

use crate::file_tree::{self, FileTreeError};
use crate::git::{self, Git, GitError};
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
/// Bytes of an image handed to the viewer as Base64; a larger image is not shown, only named.
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// An SVG must name its `<svg` element within this many leading bytes.
const SVG_PROBE_BYTES: usize = 4096;
/// A link longer than this is not opened.
const MAX_LINK_BYTES: usize = 8 * 1024;
/// A viewer opens at most one link in this time.
pub const LINK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

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
    #[error("Only the main window or the side panel may open a file viewer")]
    NotOpener,
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
    #[error("{0}")]
    Git(#[from] GitError),
    #[error("Only web addresses (http, https) open from the file viewer")]
    InvalidLink,
    #[error("Links open only from the focused viewer, one per second")]
    LinkRefused,
    #[error("The browser could not be opened: {0}")]
    Open(#[source] io::Error),
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
    /// Last change on disk, milliseconds since the Unix epoch; `None` where the system has none.
    pub modified: Option<u64>,
    /// The picture of a PNG, JPEG, GIF, WebP or SVG file whose content matches its name.
    pub image: Option<ImageData>,
    /// An image larger than [`MAX_IMAGE_BYTES`]: not handed over.
    pub image_too_large: bool,
}

/// An image as the viewer shows it: `<img src="data:{mime};base64,{data}">`. An SVG is only
/// ever drawn as an image, where it runs no script and loads nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageData {
    pub mime: &'static str,
    pub data: String,
}

/// What the live refresh compares: a changed time or size means the file changed on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStamp {
    pub modified: Option<u64>,
    pub size: u64,
}

/// How the file stands against the last commit (`HEAD`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DiffState {
    /// The file lies outside a git work tree.
    NotRepo,
    /// The repository has no commit yet.
    NoHead,
    /// The last commit does not hold the file (new, untracked or ignored).
    NotInHead,
    /// No change against the last commit.
    Same,
    Changed,
}

/// The answer of `file_viewer_diff`: the unified diff of the file against `HEAD`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeadDiff {
    pub state: DiffState,
    /// Empty unless `state` is `changed`; cut at [`git::MAX_DIFF_BYTES`] on a whole line.
    pub diff: String,
    pub truncated: bool,
}

impl HeadDiff {
    fn without_diff(state: DiffState) -> Self {
        Self {
            state,
            diff: String::new(),
            truncated: false,
        }
    }
}

/// Only the main window and the detached side panel open viewers: both show the Files tab.
pub fn check_opener(label: &str) -> Result<(), FileViewerError> {
    if label == crate::panel_window::MAIN_LABEL || label == crate::panel_window::LABEL {
        Ok(())
    } else {
        Err(FileViewerError::NotOpener)
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

/// Resolves the target again; the same text naming another file (a folder renamed or swapped
/// under the session) counts as gone.
fn resolve_target(target: &Target) -> Result<PathBuf, FileViewerError> {
    let (_, path) = resolve_file(&target.root, &target.relative)?;
    if path != target.path {
        return Err(FileViewerError::NotFound);
    }
    Ok(path)
}

/// Resolves the target again and reads its content.
pub fn read_target(target: &Target) -> Result<FileContent, FileViewerError> {
    let path = resolve_target(target)?;
    let mut file = fs::File::open(&path).map_err(not_found_or_io)?;
    let meta = file.metadata().map_err(FileViewerError::Io)?;
    if !meta.is_file() {
        return Err(FileViewerError::NotAFile);
    }
    let name = target.name();
    let mime = image_mime(&name);
    let limit = if mime.is_some() {
        MAX_IMAGE_BYTES.max(MAX_BYTES)
    } else {
        MAX_BYTES
    };
    // One byte more than the limit tells a file that grew past it since `metadata`.
    let mut bytes = Vec::with_capacity(usize::try_from(meta.len()).unwrap_or(limit).min(limit + 1));
    file.by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(FileViewerError::Io)?;
    let (image, image_too_large) = match mime {
        Some(mime) => image_limited(mime, &bytes, MAX_IMAGE_BYTES),
        None => (None, false),
    };
    let decoded = decode(&bytes[..bytes.len().min(MAX_BYTES + 1)]);
    Ok(FileContent {
        name,
        path: file_tree::display_path(&path),
        relative_path: target.relative.clone(),
        size: meta.len().max(bytes.len() as u64),
        text: decoded.text,
        truncated: decoded.truncated,
        binary: decoded.binary,
        modified: modified_millis(&meta),
        image,
        image_too_large,
    })
}

/// Size and time of the last change, for the live refresh; resolves the target like a read.
pub fn stamp_target(target: &Target) -> Result<FileStamp, FileViewerError> {
    let path = resolve_target(target)?;
    let meta = fs::metadata(&path).map_err(not_found_or_io)?;
    if !meta.is_file() {
        return Err(FileViewerError::NotAFile);
    }
    Ok(FileStamp {
        modified: modified_millis(&meta),
        size: meta.len(),
    })
}

fn modified_millis(meta: &fs::Metadata) -> Option<u64> {
    let since = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    u64::try_from(since.as_millis()).ok()
}

/// The image type a file name stands for; the content still has to match ([`image_limited`]).
pub fn image_mime(name: &str) -> Option<&'static str> {
    let (_, extension) = name.rsplit_once('.')?;
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "svg" => Some("image/svg+xml"),
        _ => None,
    }
}

/// The image of `bytes` (all of the file, at most `limit` + 1 bytes read) and whether it was too
/// large. Content that does not match the type is no image: the window then shows it as text or
/// binary, as any other file.
fn image_limited(mime: &'static str, bytes: &[u8], limit: usize) -> (Option<ImageData>, bool) {
    if bytes.len() > limit {
        return (None, true);
    }
    if !content_matches(mime, bytes) {
        return (None, false);
    }
    let data = ImageData {
        mime,
        data: crate::icon::base64(bytes),
    };
    (Some(data), false)
}

/// The leading bytes match the image type (magic numbers; an SVG names `<svg` early on).
fn content_matches(mime: &str, bytes: &[u8]) -> bool {
    match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => bytes.starts_with(b"\xFF\xD8\xFF"),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        "image/webp" => bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP",
        "image/svg+xml" => {
            let probe = &bytes[..bytes.len().min(SVG_PROBE_BYTES)];
            !probe.contains(&0)
                && String::from_utf8_lossy(probe)
                    .to_ascii_lowercase()
                    .contains("<svg")
        }
        _ => false,
    }
}

/// The file against the last commit of the work tree it lies in: `git diff HEAD -- <file>` with
/// the hardened runner and checks of the changes panel (filters off, no link to a share, the
/// work tree where git found it). Runs from the file's own folder, so a file in a submodule or a
/// nested repository compares with that repository.
pub fn head_diff(git: &Git, target: &Target) -> Result<HeadDiff, FileViewerError> {
    let path = resolve_target(target)?;
    let folder = path.parent().ok_or(FileViewerError::NotAFile)?;
    git::check_folder(folder)?;
    let deadline = Instant::now() + git::TIMEOUT;
    let Some(top) = git::toplevel(git, folder, deadline)? else {
        return Ok(HeadDiff::without_diff(DiffState::NotRepo));
    };
    let top = fs::canonicalize(&top).map_err(FileViewerError::Io)?;
    let Ok(inside) = path.strip_prefix(&top) else {
        return Ok(HeadDiff::without_diff(DiffState::NotRepo));
    };
    let relative = git_relative(inside).ok_or(FileViewerError::InvalidPath("not UTF-8"))?;
    let neutral = git::filter_overrides(git, &top, deadline)?;
    let has_head = git
        .run(
            &top,
            ["rev-parse", "--verify", "--quiet", "HEAD^{commit}"],
            deadline,
            4096,
        )?
        .success;
    if !has_head {
        return Ok(HeadDiff::without_diff(DiffState::NoHead));
    }
    // `HEAD:<path>` names the path from the top level, literally.
    let in_head = git
        .run(
            &top,
            ["cat-file", "-e", &format!("HEAD:{relative}")],
            deadline,
            4096,
        )?
        .success;
    if !in_head {
        return Ok(HeadDiff::without_diff(DiffState::NotInHead));
    }
    let patch = git.run(
        &top,
        neutral
            .iter()
            .map(String::as_str)
            .chain(["--literal-pathspecs"])
            .chain(git::DIFF_ARGS.iter().copied())
            .chain(["HEAD", "--", relative.as_str()]),
        deadline,
        git::MAX_DIFF_BYTES,
    )?;
    if !patch.usable() {
        return Err(patch.failure().into());
    }
    if patch.stdout.is_empty() {
        return Ok(HeadDiff::without_diff(DiffState::Same));
    }
    let truncated = patch.truncated;
    Ok(HeadDiff {
        state: DiffState::Changed,
        diff: git::cut_text(patch.stdout, truncated),
        truncated,
    })
}

/// A path below the top level as git names it: `/` separated; `None` if not UTF-8.
fn git_relative(inside: &Path) -> Option<String> {
    let mut parts = Vec::new();
    for part in inside.components() {
        match part {
            Component::Normal(name) => parts.push(name.to_str()?),
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// A link from the Markdown preview, checked before the browser gets it: a plain http(s)
/// address with a host, of reasonable length. Everything else (`file:`, `javascript:`, app
/// addresses) is refused.
pub fn checked_link(text: &str) -> Result<reqwest::Url, FileViewerError> {
    if text.len() > MAX_LINK_BYTES {
        return Err(FileViewerError::InvalidLink);
    }
    let url = reqwest::Url::parse(text).map_err(|_| FileViewerError::InvalidLink)?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(FileViewerError::InvalidLink);
    }
    // `https://user:pass@host` hands credentials over, or hides the real host behind a name that
    // looks like one (`https://example.com@other.host`).
    if !url.username().is_empty() || url.password().is_some() {
        return Err(FileViewerError::InvalidLink);
    }
    Ok(url)
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
    /// When each viewer last opened a link ([`Viewers::allow_link`]).
    last_link: HashMap<String, Instant>,
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
        let mut state = lock(&self.0);
        state.targets.remove(label);
        state.last_link.remove(label);
    }

    /// At most one link per [`LINK_INTERVAL`] and viewer, so a page cannot open browser tabs in a
    /// loop. Records `now` when it allows one.
    pub fn allow_link(&self, label: &str, now: Instant) -> bool {
        let mut state = lock(&self.0);
        if !state.targets.contains_key(label) {
            return false;
        }
        if let Some(last) = state.last_link.get(label) {
            if now.saturating_duration_since(*last) < LINK_INTERVAL {
                return false;
            }
        }
        state.last_link.insert(label.to_owned(), now);
        true
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

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR";

    #[test]
    fn image_types_by_name_and_content() {
        assert_eq!(image_mime("a.PNG"), Some("image/png"));
        assert_eq!(image_mime("photo.jpeg"), Some("image/jpeg"));
        assert_eq!(image_mime("x.Jpg"), Some("image/jpeg"));
        assert_eq!(image_mime("a.gif"), Some("image/gif"));
        assert_eq!(image_mime("a.webp"), Some("image/webp"));
        assert_eq!(image_mime("logo.svg"), Some("image/svg+xml"));
        for none in ["png", "a.txt", "a.png.txt", "a.ico", "a.svgz"] {
            assert_eq!(image_mime(none), None, "{none}");
        }
        assert!(content_matches("image/png", PNG));
        assert!(content_matches("image/jpeg", b"\xFF\xD8\xFF\xE0rest"));
        assert!(content_matches("image/gif", b"GIF89a..."));
        assert!(content_matches(
            "image/webp",
            b"RIFF\x10\x00\x00\x00WEBPVP8 "
        ));
        assert!(content_matches(
            "image/svg+xml",
            b"<?xml version=\"1.0\"?>\n<SVG xmlns=\"x\"/>"
        ));
        // A name does not make an image: the content must match.
        assert!(!content_matches("image/png", b"not a png"));
        assert!(!content_matches("image/webp", b"RIFF\x10\x00\x00\x00WAVE"));
        assert!(!content_matches("image/svg+xml", b"<html></html>"));
        assert!(!content_matches("image/svg+xml", b"<svg\x00"));
    }

    #[test]
    fn images_are_capped() {
        let (image, too_large) = image_limited("image/png", PNG, PNG.len());
        assert!(!too_large);
        let image = image.unwrap();
        assert_eq!(image.mime, "image/png");
        assert_eq!(image.data, crate::icon::base64(PNG));
        // One byte over the limit: neither data nor an error, only the flag.
        assert_eq!(image_limited("image/png", PNG, PNG.len() - 1), (None, true));
        assert_eq!(image_limited("image/png", b"text", 100), (None, false));

        let dir = TempDir::new("file-viewer-image");
        fs::write(dir.path().join("a.png"), PNG).unwrap();
        fs::write(dir.path().join("fake.png"), "plain text").unwrap();
        fs::write(dir.path().join("logo.svg"), "<svg xmlns=\"x\"></svg>").unwrap();
        let png = read_target(&target_for(dir.path(), "a.png")).unwrap();
        assert!(png.binary && png.image.is_some() && !png.image_too_large);
        let fake = read_target(&target_for(dir.path(), "fake.png")).unwrap();
        assert!(fake.image.is_none());
        assert_eq!(fake.text.as_deref(), Some("plain text"));
        // An SVG is text and an image at once.
        let svg = read_target(&target_for(dir.path(), "logo.svg")).unwrap();
        assert_eq!(svg.image.map(|i| i.mime), Some("image/svg+xml"));
        assert!(svg.text.is_some());

        let mut big = PNG.to_vec();
        big.resize(MAX_IMAGE_BYTES + 1, 0);
        fs::write(dir.path().join("big.png"), &big).unwrap();
        let big = read_target(&target_for(dir.path(), "big.png")).unwrap();
        assert!(big.image.is_none() && big.image_too_large);
        assert_eq!(big.size, MAX_IMAGE_BYTES as u64 + 1);
    }

    #[test]
    fn stamps_follow_changes_on_disk() {
        let dir = TempDir::new("file-viewer-stamp");
        fs::write(dir.path().join("a.txt"), "one").unwrap();
        let target = target_for(dir.path(), "a.txt");
        let first = stamp_target(&target).unwrap();
        assert_eq!(first.size, 3);
        assert!(first.modified.is_some());
        assert_eq!(
            read_target(&target).unwrap().modified,
            first.modified,
            "the content carries the stamp it was read with"
        );
        fs::write(dir.path().join("a.txt"), "longer").unwrap();
        assert_ne!(stamp_target(&target).unwrap(), first);
        fs::remove_file(dir.path().join("a.txt")).unwrap();
        assert!(matches!(
            stamp_target(&target),
            Err(FileViewerError::NotFound)
        ));
    }

    #[test]
    fn links_must_be_web_addresses() {
        assert_eq!(
            checked_link("https://example.com/a?b=c#d")
                .unwrap()
                .as_str(),
            "https://example.com/a?b=c#d"
        );
        assert!(checked_link("http://example.com").is_ok());
        for bad in [
            "file:///C:/Windows/win.ini",
            "javascript:marker",
            "tauri://localhost/x",
            "mailto:a@example.com",
            "relative/path.md",
            "",
            "http://",
        ] {
            assert!(
                matches!(checked_link(bad), Err(FileViewerError::InvalidLink)),
                "{bad:?} was accepted"
            );
        }
        let long = format!("https://example.com/{}", "a".repeat(MAX_LINK_BYTES));
        assert!(checked_link(&long).is_err());
        // Credentials in the address, or a host hidden behind a user name, are refused.
        for bad in [
            "https://user:secret@example.com/",
            "https://user@example.com/",
            "https://example.com@other.example/",
            "https://:secret@example.com/",
        ] {
            assert!(
                matches!(checked_link(bad), Err(FileViewerError::InvalidLink)),
                "{bad:?} was accepted"
            );
        }
    }

    #[test]
    fn links_open_at_most_once_a_second_per_viewer() {
        let viewers = Viewers::default();
        let target = Target {
            session_id: 1,
            root: PathBuf::from("r"),
            path: PathBuf::from("a"),
            relative: "a".to_owned(),
        };
        let Claim::New(label) = viewers.claim(target) else {
            panic!("new viewer expected");
        };
        let start = Instant::now();
        assert!(viewers.allow_link(&label, start));
        assert!(!viewers.allow_link(&label, start + LINK_INTERVAL / 2));
        assert!(viewers.allow_link(&label, start + LINK_INTERVAL));
        // Unknown and closed windows open nothing.
        assert!(!viewers.allow_link("file-viewer-99", start));
        viewers.remove(&label);
        assert!(!viewers.allow_link(&label, start + LINK_INTERVAL * 5));
    }

    #[test]
    fn git_paths_use_slashes() {
        assert_eq!(
            git_relative(&Path::new("a").join("b c").join("d.rs")).as_deref(),
            Some("a/b c/d.rs")
        );
        assert_eq!(git_relative(Path::new("")), None);
        assert_eq!(git_relative(Path::new("../x")), None);
    }

    #[test]
    fn diff_against_the_last_commit() {
        use crate::git::tests::{git, git_ok, init_repo};
        let Some(git) = git() else { return };
        let dir = TempDir::new("file-viewer-diff");
        let repo = dir.path().join("repo");
        init_repo(&git, &repo);
        fs::create_dir_all(repo.join("src")).unwrap();
        fs::write(repo.join("src/a.txt"), "one\ntwo\n").unwrap();
        let target = target_for(&repo, "src/a.txt");
        assert_eq!(head_diff(&git, &target).unwrap().state, DiffState::NoHead);

        git_ok(&git, &repo, &["add", "-A"]);
        git_ok(&git, &repo, &["commit", "--quiet", "-m", "first"]);
        let same = head_diff(&git, &target).unwrap();
        assert_eq!(same, HeadDiff::without_diff(DiffState::Same));

        fs::write(repo.join("src/a.txt"), "one\nzwei\n").unwrap();
        let changed = head_diff(&git, &target).unwrap();
        assert_eq!(changed.state, DiffState::Changed);
        assert!(changed.diff.contains("-two\n+zwei"));
        assert!(!changed.truncated);
        // Staged changes count too: the comparison is with the commit, not the index.
        git_ok(&git, &repo, &["add", "-A"]);
        assert_eq!(head_diff(&git, &target).unwrap().state, DiffState::Changed);

        // A new file, and one whose name looks like pathspec magic, only compare themselves.
        fs::write(repo.join("new.txt"), "x\n").unwrap();
        let new = target_for(&repo, "new.txt");
        assert_eq!(head_diff(&git, &new).unwrap().state, DiffState::NotInHead);
        fs::write(repo.join("*.txt"), "x\n").ok();
        if repo.join("*.txt").exists() {
            git_ok(&git, &repo, &["add", "-A"]);
            git_ok(&git, &repo, &["commit", "--quiet", "-m", "second"]);
            // As a glob, `*.txt` would match this change.
            fs::write(repo.join("new.txt"), "changed\n").unwrap();
            let star = target_for(&repo, "*.txt");
            assert_eq!(head_diff(&git, &star).unwrap().state, DiffState::Same);
        }

        fs::remove_file(repo.join("src/a.txt")).unwrap();
        assert!(matches!(
            head_diff(&git, &target),
            Err(FileViewerError::NotFound)
        ));
    }

    #[test]
    fn diff_answers_serialize_camel_case() {
        let json = serde_json::to_value(HeadDiff::without_diff(DiffState::NotInHead)).unwrap();
        assert_eq!(json["state"], "notInHead");
        assert_eq!(json["diff"], "");
    }

    #[test]
    fn only_viewer_windows_read_and_only_main_opens() {
        assert!(check_sender("file-viewer-1").is_ok());
        assert!(check_sender("file-viewer-42").is_ok());
        for bad in [
            "main",
            "panel",
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
        assert!(check_opener("panel").is_ok());
        for bad in ["file-viewer-1", "notes", "Panel", ""] {
            assert!(
                matches!(check_opener(bad), Err(FileViewerError::NotOpener)),
                "{bad}"
            );
        }
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
            modified: Some(5),
            image: Some(ImageData {
                mime: "image/png",
                data: "AA==".into(),
            }),
            image_too_large: false,
        };
        let json = serde_json::to_value(content).unwrap();
        assert_eq!(json["image"]["mime"], "image/png");
        assert_eq!(json["imageTooLarge"], false);
        assert_eq!(json["modified"], 5);
        assert_eq!(json["relativePath"], "a.rs");
        assert!(json["text"].is_null());
        assert_eq!(json["binary"], true);
        let error = serde_json::to_value(FileViewerError::UnknownWindow).unwrap();
        assert_eq!(error, "Unknown file viewer window");
    }
}
