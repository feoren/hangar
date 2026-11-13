//! Paths and web addresses a session printed, made clickable in the terminal (the frontend's
//! terminalLinks.ts). The frontend names the path here, unlike the file tree: a session prints
//! paths anywhere on the machine. What keeps this narrow:
//!
//! - only local absolute paths: no UNC or device paths, nothing on a network drive and nothing
//!   whose real path is a network path, since a mere lookup would contact another machine (and
//!   hand it the user's credentials on Windows); no alternate data streams;
//! - looking up only says "file", "folder" or nothing, never reads a file, and never follows a
//!   link in the last segment;
//! - a click opens a file in its default program only when its real name (after Windows dropped
//!   trailing dots and spaces, expanded 8.3 short names and followed links) has an extension from
//!   a short list of harmless types (text, code, images, PDF). Everything else is only shown
//!   selected in the file manager: an allowlist, because no list of runnable types is complete;
//! - macOS bundles (an app is a folder) are revealed, never opened;
//! - web addresses only as plain http(s) without user name or password.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Serialize, Serializer};

/// Paths per lookup; the frontend batches the paths of the lines under the mouse.
pub const MAX_PATHS: usize = 64;
/// Longer text is no path a terminal line printed.
const MAX_PATH_BYTES: usize = 4096;

#[derive(Debug, thiserror::Error)]
pub enum PathLinkError {
    #[error("At most {MAX_PATHS} paths per lookup")]
    TooMany,
    #[error("Not a local absolute path")]
    NotLocal,
    #[error("Not found")]
    NotFound,
    #[error("Only plain http(s) addresses open in the browser")]
    NotWeb,
    #[error("Only a click in the focused window opens something")]
    NotFocused,
    #[error("Could not open: {0}")]
    Open(#[source] io::Error),
    #[error("Background task aborted: {0}")]
    Task(String),
    #[error("The path did not answer in time")]
    TimedOut,
}

impl Serialize for PathLinkError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PathKind {
    File,
    Dir,
}

/// What a click did with a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Opened {
    /// A folder in the file manager, a file in its default program.
    Opened,
    /// Shown selected in the file manager instead (any file that is not plainly harmless).
    Revealed,
}

/// What a click does, decided before anything is started (testable without opening windows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    OpenFolder(PathBuf),
    OpenFile(PathBuf),
    Reveal(PathBuf),
}

/// The text as a local absolute path of this platform, or `None`.
pub fn local_path(text: &str) -> Option<PathBuf> {
    local_path_for(text, cfg!(windows)).map(PathBuf::from)
}

/// Platform independent so both rule sets are tested everywhere.
fn local_path_for(text: &str, windows: bool) -> Option<&str> {
    if text.is_empty() || text.len() > MAX_PATH_BYTES || text.chars().any(char::is_control) {
        return None;
    }
    if windows {
        let bytes = text.as_bytes();
        let drive = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/');
        // `\\server\share`, `\\?\` and `\\.\` never get here: they do not start with a drive.
        // A colon after the drive names a data stream, `<>"|?*` are wildcards or invalid.
        let clean = || !text[2..].contains([':', '<', '>', '"', '|', '?', '*']);
        (drive && clean()).then_some(text)
    } else {
        (text.starts_with('/') && !text.starts_with("//")).then_some(text)
    }
}

/// The real path: links followed, 8.3 names expanded, trailing dots and spaces dropped as Windows
/// does. `None` when it cannot be read or is no local drive path (`\\?\UNC\...`). On Windows the
/// `\\?\` prefix is dropped again, so programs get the usual form.
pub fn real_path(path: &Path) -> Option<PathBuf> {
    let real = fs::canonicalize(path).ok()?;
    local_real(&real.to_string_lossy(), cfg!(windows)).map(PathBuf::from)
}

fn local_real(text: &str, windows: bool) -> Option<String> {
    if !windows {
        return local_path_for(text, false).map(str::to_owned);
    }
    let rest = text.strip_prefix(r"\\?\")?;
    let bytes = rest.as_bytes();
    let drive =
        bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' && bytes[2] == b'\\';
    drive.then(|| rest.to_owned())
}

/// Whether the path lies on a network drive (a mapped `Z:`); looking there touches the network.
#[cfg(windows)]
fn on_network_drive(path: &Path) -> bool {
    use std::path::{Component, Prefix};
    /// `DRIVE_REMOTE` of `GetDriveTypeW`.
    const DRIVE_REMOTE: u32 = 4;
    let Some(Component::Prefix(prefix)) = path.components().next() else {
        return true;
    };
    let letter = match prefix.kind() {
        Prefix::Disk(letter) | Prefix::VerbatimDisk(letter) => letter,
        _ => return true,
    };
    let root: Vec<u16> = format!("{}:\\", char::from(letter))
        .encode_utf16()
        .chain(Some(0))
        .collect();
    // SAFETY: `root` is a NUL-terminated UTF-16 string that outlives the call, which only reads
    // it.
    let kind = unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(root.as_ptr()) };
    kind == DRIVE_REMOTE
}

#[cfg(not(windows))]
fn on_network_drive(_path: &Path) -> bool {
    false
}

/// File or folder for the hover underline, `None` when nothing is there, when the last segment
/// is a link (or junction), or when the path lies on or leads to the network.
pub fn kind(path: &Path) -> Option<PathKind> {
    if on_network_drive(path) {
        return None;
    }
    let meta = fs::symlink_metadata(path).ok()?;
    if meta.file_type().is_symlink() {
        return None;
    }
    real_path(path)?;
    if meta.is_dir() {
        Some(PathKind::Dir)
    } else if meta.is_file() {
        Some(PathKind::File)
    } else {
        None
    }
}

/// Looks up each text; anything that is no local absolute path counts as not there.
pub fn kinds(paths: &[String]) -> Result<Vec<Option<PathKind>>, PathLinkError> {
    if paths.len() > MAX_PATHS {
        return Err(PathLinkError::TooMany);
    }
    Ok(paths
        .iter()
        .map(|text| local_path(text).and_then(|path| kind(&path)))
        .collect())
}

/// Extensions a click opens in their default program. Their usual "open" never runs code: text,
/// code of compiled languages, data, images and PDF. Scripts Windows would run (`.js`, `.py`,
/// `.ps1`, ...) are deliberately missing, as is HTML. Compared without case.
const OPENABLE: &[&str] = &[
    "txt", "md", "markdown", "log", "json", "jsonc", "toml", "yaml", "yml", "csv", "tsv", "xml",
    "ini", "cfg", "conf", "rs", "go", "c", "h", "cc", "cpp", "hpp", "cs", "java", "kt", "swift",
    "ts", "tsx", "css", "scss", "sass", "less", "sql", "diff", "patch", "lock", "pdf", "png",
    "jpg", "jpeg", "gif", "webp", "bmp", "ico", "svg",
];

/// macOS bundle folders: `open` would launch or install them.
const BUNDLES: &[&str] = &[
    "app",
    "pkg",
    "mpkg",
    "prefpane",
    "workflow",
    "bundle",
    "framework",
    "plugin",
    "kext",
];

fn extension_in(path: &Path, list: &[&str]) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| list.iter().any(|known| known.eq_ignore_ascii_case(ext)))
}

/// Whether a file with this real name opens in its default program rather than being revealed.
pub fn opens_directly(real: &Path) -> bool {
    extension_in(real, OPENABLE) && !executable(real)
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    fs::metadata(path).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(_path: &Path) -> bool {
    false
}

/// What a click on `text` does; decided on the real path, never on the printed name.
pub fn action(text: &str) -> Result<Action, PathLinkError> {
    action_for(text, cfg!(target_os = "macos"))
}

fn action_for(text: &str, macos: bool) -> Result<Action, PathLinkError> {
    let path = local_path(text).ok_or(PathLinkError::NotLocal)?;
    if on_network_drive(&path) {
        return Err(PathLinkError::NotLocal);
    }
    let real = real_path(&path).ok_or(PathLinkError::NotFound)?;
    let meta = fs::metadata(&real).map_err(|_| PathLinkError::NotFound)?;
    if meta.is_dir() {
        if macos && extension_in(&real, BUNDLES) {
            return Ok(Action::Reveal(real));
        }
        return Ok(Action::OpenFolder(real));
    }
    if !meta.is_file() {
        return Err(PathLinkError::NotFound);
    }
    if opens_directly(&real) {
        Ok(Action::OpenFile(real))
    } else {
        Ok(Action::Reveal(real))
    }
}

/// Opens a folder in the file manager and a harmless file in its default program; anything else
/// is shown selected.
pub fn open(text: &str) -> Result<Opened, PathLinkError> {
    match action(text)? {
        Action::OpenFolder(path) => {
            crate::desktop::open_folder(&path).map_err(PathLinkError::Open)?;
            Ok(Opened::Opened)
        }
        Action::OpenFile(path) => {
            crate::desktop::open_file(&path).map_err(PathLinkError::Open)?;
            Ok(Opened::Opened)
        }
        Action::Reveal(path) => {
            crate::desktop::reveal(&path).map_err(PathLinkError::Open)?;
            Ok(Opened::Revealed)
        }
    }
}

/// A web address a session printed, if it is a plain http(s) address with a host and without
/// user name or password (`https://bank.example@evil.example` would mislead).
pub fn web_url(text: &str) -> Option<reqwest::Url> {
    let url = reqwest::Url::parse(text).ok()?;
    let plain = matches!(url.scheme(), "http" | "https")
        && url.username().is_empty()
        && url.password().is_none()
        && url.host_str().is_some_and(|host| !host.is_empty());
    plain.then_some(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("tests")
            .join(".tmp")
            .join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::canonicalize(&dir)
            .ok()
            .and_then(|real| local_real(&real.to_string_lossy(), cfg!(windows)))
            .map(PathBuf::from)
            .unwrap()
    }

    fn text(path: &Path) -> String {
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn windows_paths_must_be_local_and_absolute() {
        for ok in [
            r"C:\Users\me\file.txt",
            "C:/Users/me",
            r"e:\",
            r"D:\Program Files (x86)\x",
        ] {
            assert_eq!(local_path_for(ok, true), Some(ok), "{ok}");
        }
        for refused in [
            r"\\server\share\file.txt",
            r"\\?\C:\x",
            r"\\.\pipe\x",
            "//server/share",
            r"C:relative",
            r"relative\path",
            r"\rooted",
            r"C:\x\file.txt:stream",
            r"C:\x\*.txt",
            "C:\\x\u{0}y",
            "",
            "C:",
        ] {
            assert_eq!(local_path_for(refused, true), None, "{refused:?}");
        }
    }

    #[test]
    fn unix_paths_must_be_absolute_and_not_network() {
        assert_eq!(
            local_path_for("/home/me/a b.md", false),
            Some("/home/me/a b.md")
        );
        assert_eq!(local_path_for("/", false), Some("/"));
        for refused in ["//host/share", "relative/x", "./x", "", "/x\ny"] {
            assert_eq!(local_path_for(refused, false), None, "{refused:?}");
        }
    }

    #[test]
    fn real_paths_must_stay_on_a_local_drive() {
        assert_eq!(
            local_real(r"\\?\C:\x\a.md", true).as_deref(),
            Some(r"C:\x\a.md")
        );
        assert_eq!(local_real(r"\\?\UNC\server\share\a.md", true), None);
        assert_eq!(local_real(r"\\server\share\a.md", true), None);
        assert_eq!(local_real(r"C:\x", true), None);
        assert_eq!(
            local_real("/home/a.md", false).as_deref(),
            Some("/home/a.md")
        );
        assert_eq!(local_real("//host/a.md", false), None);
    }

    #[test]
    fn overlong_text_is_no_path() {
        let long = format!("/{}", "a".repeat(MAX_PATH_BYTES));
        assert_eq!(local_path_for(&long, false), None);
    }

    #[test]
    fn only_harmless_types_open_directly() {
        for name in [
            "notes.md",
            "image.PNG",
            "report.pdf",
            "data.json",
            "main.rs",
            "a.svg",
        ] {
            assert!(opens_directly(Path::new(name)), "{name}");
        }
        for name in [
            "setup.exe",
            "run.cmd",
            "x.ps1",
            "a.lnk",
            "b.url",
            "tool.js",
            "s.py",
            "page.html",
            "help.chm",
            "Makefile",
            "archive.zip",
        ] {
            assert!(!opens_directly(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn macos_bundles_are_revealed() {
        let dir = test_dir("path_links_bundle");
        let bundle = dir.join("Marker.app");
        fs::create_dir_all(&bundle).unwrap();
        assert_eq!(
            action_for(&text(&bundle), true).unwrap(),
            Action::Reveal(bundle.clone())
        );
        assert_eq!(
            action_for(&text(&bundle), false).unwrap(),
            Action::OpenFolder(bundle.clone())
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn looks_up_files_and_folders() {
        let dir = test_dir("path_links");
        let file = dir.join("marker file.txt");
        fs::write(&file, "marker").unwrap();
        let found = kinds(&[
            text(&dir),
            text(&file),
            text(&dir.join("missing.txt")),
            "relative.txt".into(),
        ])
        .unwrap();
        assert_eq!(
            found,
            vec![Some(PathKind::Dir), Some(PathKind::File), None, None]
        );
        let too_many = vec![text(&file); MAX_PATHS + 1];
        assert!(matches!(kinds(&too_many), Err(PathLinkError::TooMany)));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn real_names_decide_what_opens() {
        let dir = test_dir("path_links_real");
        let script = dir.join("marker.cmd");
        fs::write(&script, "rem marker").unwrap();
        let note = dir.join("marker.md");
        fs::write(&note, "marker").unwrap();
        assert_eq!(
            action(&text(&script)).unwrap(),
            Action::Reveal(script.clone())
        );
        assert_eq!(
            action(&text(&note)).unwrap(),
            Action::OpenFile(note.clone())
        );
        assert!(matches!(
            action(&text(&dir.join("missing.md"))),
            Err(PathLinkError::NotFound)
        ));
        if cfg!(windows) {
            // The printed name hides the extension; Windows still finds the script.
            for disguised in ["marker.cmd.", "marker.cmd "] {
                let printed = format!("{}\\{disguised}", text(&dir));
                assert_eq!(
                    action(&printed).unwrap(),
                    Action::Reveal(script.clone()),
                    "{disguised:?}"
                );
            }
            #[cfg(windows)]
            if let Some(short) = short_name(&dir.join("marker long name.cmd")) {
                assert_eq!(
                    action(&text(&short)).unwrap(),
                    Action::Reveal(dir.join("marker long name.cmd"))
                );
            }
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Creates the file and returns its 8.3 short path, `None` when the volume makes none.
    #[cfg(windows)]
    fn short_name(file: &Path) -> Option<PathBuf> {
        use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};
        fs::write(file, "rem marker").unwrap();
        let wide: Vec<u16> = file.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut buffer = vec![0u16; 1024];
        // SAFETY: `wide` is NUL-terminated and `buffer` holds as many units as passed; the call
        // writes at most that many.
        let length = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetShortPathNameW(
                wide.as_ptr(),
                buffer.as_mut_ptr(),
                buffer.len() as u32,
            )
        } as usize;
        if length == 0 || length > buffer.len() {
            return None;
        }
        let short = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..length]));
        (short != file).then_some(short)
    }

    #[test]
    fn only_plain_web_addresses_open() {
        for ok in [
            "https://example.com/a?b=c#d",
            "http://localhost:5173/",
            "HTTPS://Example.com",
        ] {
            assert!(web_url(ok).is_some(), "{ok}");
        }
        for refused in [
            "https://user@example.com/",
            "https://user:pw@example.com/",
            "file:///C:/Windows/System32/calc.exe",
            "javascript:alert(1)",
            "ftp://example.com/",
            "mailto:a@example.com",
            "example.com",
            "",
        ] {
            assert!(web_url(refused).is_none(), "{refused}");
        }
    }
}
