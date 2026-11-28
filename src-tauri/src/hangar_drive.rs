//! Hangar Drive, the separate sync and backup app (ADR 0024). Hangar only finds out whether it is
//! installed, hands it a project folder through its `hangar-drive://add-folder` link and reads the
//! public list of the folders it syncs, to mark the project cards. Hangar Drive asks the person to
//! confirm every folder itself; nothing here adds a folder or writes a file of Hangar Drive.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

/// The link scheme Hangar Drive registers for itself.
pub const SCHEME: &str = "hangar-drive";
/// Paths one status call may ask about; the overview asks for one per card.
pub const MAX_PATHS: usize = 256;
/// The only version of the folder list Hangar understands; any other means no marks.
const FOLDERS_CONTRACT: &str = "drive-folders 1";
/// The list is a few kilobytes; a larger file is not read at all.
const MAX_FOLDERS_BYTES: u64 = 1024 * 1024;
/// Entries read from the list at most.
const MAX_FOLDERS: usize = 4096;
/// Longer paths are refused before the file system is asked; Hangar Drive takes at most this many
/// bytes of decoded path.
const MAX_PATH_LEN: usize = 4096;
/// Hangar Drive refuses a longer add-folder link. A path within [`MAX_PATH_LEN`] always fits
/// (three characters per byte plus the prefix), so this is a last guard, not a second limit.
const MAX_LINK_LEN: usize = 12_320;
/// Hangar Drive keeps one proposal open and ignores new ones for 30 s after an outcome without an
/// add, so the same folder is not handed over again within this time.
const PROPOSAL_COOLDOWN: Duration = Duration::from_secs(30);
/// Names Windows keeps for devices, also with an extension (`nul.txt`).
const RESERVED_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DriveError {
    #[error("Only the main window may do this")]
    WrongWindow,
    #[error("The window is not focused")]
    NotFocused,
    #[error("Hangar Drive is not installed")]
    NotInstalled,
    #[error("Only an existing folder on a local drive can be backed up")]
    NotLocalFolder,
    #[error("Too many folders at once")]
    TooMany,
    #[error("The link for this folder would be too long")]
    LinkTooLong,
    #[error("Background task aborted: {0}")]
    Task(String),
    #[error("Hangar Drive did not open: {0}")]
    Open(String),
}

impl Serialize for DriveError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// What Hangar Drive does with a folder. A state this version does not know is [`Self::Other`]:
/// the folder is still in Hangar Drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FolderState {
    Active,
    Paused,
    Stopped,
    Other,
}

impl FolderState {
    fn parse(text: Option<&str>) -> Self {
        match text {
            Some("active") => Self::Active,
            Some("paused") => Self::Paused,
            Some("stopped") => Self::Stopped,
            _ => Self::Other,
        }
    }
}

/// One folder of Hangar Drive's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveFolder {
    pub path: String,
    pub state: FolderState,
}

/// What the window needs: whether to offer Hangar Drive at all, and per asked path the state of
/// the folder in Hangar Drive that holds it (`None`: not in Hangar Drive).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveStatus {
    pub installed: bool,
    pub marks: Vec<Option<FolderState>>,
}

/// The `hangar-drive://add-folder` link for one checked folder. Only [`add_folder_link`] makes
/// one, so the opener in desktop.rs can never be handed another address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriveLink(String);

impl DriveLink {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The link for `path`: exactly one query parameter, no fragment. Every byte outside the RFC 3986
/// unreserved set is percent-encoded (a space as `%20`, never `+`), so `#`, `&`, `%` and `?` in a
/// folder name cannot end or split the parameter.
pub fn add_folder_link(path: &str) -> Result<DriveLink, DriveError> {
    let link = format!("{SCHEME}://add-folder?path={}", percent_encode(path));
    if link.len() > MAX_LINK_LEN {
        return Err(DriveError::LinkTooLong);
    }
    Ok(DriveLink(link))
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The program in a shell open command such as `"C:\x\hangar-drive.exe" "%1"`: the quoted first
/// part, or the first word when it has no quotes. Only an absolute path to an `.exe` counts.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn command_program(command: &str) -> Option<PathBuf> {
    let command = command.trim();
    let program = if let Some(rest) = command.strip_prefix('"') {
        &rest[..rest.find('"')?]
    } else {
        command.split_whitespace().next()?
    };
    if program.chars().any(char::is_control) {
        return None;
    }
    let path = PathBuf::from(program);
    let exe = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"));
    (exe && absolute(program, true)).then_some(path)
}

/// The parts of a local absolute path, for comparing; `None` for anything else (relative, UNC
/// or device paths, `..`). On Windows `/` counts as `\`, a leading `\\?\` before a drive letter is
/// dropped and the parts are compared without case, as NTFS does.
fn parts(path: &str, windows: bool) -> Option<Vec<String>> {
    if path.is_empty() || path.len() > MAX_PATH_LEN || path.chars().any(char::is_control) {
        return None;
    }
    let parts: Vec<String> = if windows {
        let path = path
            .strip_prefix(r"\\?\")
            .unwrap_or(path)
            .replace('/', "\\");
        let bytes = path.as_bytes();
        // `C:` alone or `C:\…`; `C:x` is relative to the drive's own current folder.
        let drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
        if !drive || (bytes.len() > 2 && bytes[2] != b'\\') {
            return None;
        }
        path.split('\\')
            .filter(|part| !part.is_empty() && *part != ".")
            .map(str::to_lowercase)
            .collect()
    } else {
        if !path.starts_with('/') {
            return None;
        }
        path.split('/')
            .filter(|part| !part.is_empty() && *part != ".")
            .map(str::to_owned)
            .collect()
    };
    (!parts.iter().any(|part| part == "..")).then_some(parts)
}

fn absolute(path: &str, windows: bool) -> bool {
    !(windows && (path.starts_with(r"\\") || path.starts_with("//")))
        && parts(path, windows).is_some()
}

/// Whether `project` equals `folder` or lies inside it, part by part: `C:\a\proj2` is not inside
/// `C:\a\proj`.
pub fn covers(folder: &str, project: &str, windows: bool) -> bool {
    match (parts(folder, windows), parts(project, windows)) {
        (Some(folder), Some(project)) => {
            !folder.is_empty() && project.len() >= folder.len() && project[..folder.len()] == folder
        }
        _ => false,
    }
}

/// The state of the innermost listed folder that holds `project`.
pub fn mark(folders: &[DriveFolder], project: &str, windows: bool) -> Option<FolderState> {
    folders
        .iter()
        .filter(|folder| covers(&folder.path, project, windows))
        .max_by_key(|folder| parts(&folder.path, windows).map_or(0, |parts| parts.len()))
        .map(|folder| folder.state)
}

/// The folders of Hangar Drive's list. Broken JSON, another contract or a missing list give none;
/// an entry without a usable path is left out, an unknown state is [`FolderState::Other`].
pub fn parse_folders(bytes: &[u8]) -> Vec<DriveFolder> {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return Vec::new();
    };
    if value.get("contract").and_then(Value::as_str) != Some(FOLDERS_CONTRACT) {
        return Vec::new();
    }
    let Some(list) = value.get("folders").and_then(Value::as_array) else {
        return Vec::new();
    };
    list.iter()
        .take(MAX_FOLDERS)
        .filter_map(|entry| {
            let path = entry.get("path")?.as_str()?;
            parts(path, cfg!(windows))?;
            Some(DriveFolder {
                path: path.to_owned(),
                state: FolderState::parse(entry.get("state").and_then(Value::as_str)),
            })
        })
        .collect()
}

/// `<config_local_dir>/Hangar Drive/public/folders.json`, the file the release build of Hangar
/// Drive writes; its debug builds write elsewhere and are never read.
pub fn folders_file() -> Option<PathBuf> {
    dirs::config_local_dir().map(|dir| dir.join("Hangar Drive").join("public").join("folders.json"))
}

/// The list in `file`; none when it is missing, too large or unreadable.
pub fn read_folders(file: &Path) -> Vec<DriveFolder> {
    let Ok(handle) = fs::File::open(file) else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    // One byte more than allowed tells a file that is too large from one that just fits.
    if handle
        .take(MAX_FOLDERS_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
        || bytes.len() as u64 > MAX_FOLDERS_BYTES
    {
        return Vec::new();
    }
    parse_folders(&bytes)
}

/// The local form of a canonical path: on Windows without the `\\?\` that `canonicalize` adds,
/// and only when a drive letter follows (a network share comes back as `\\?\UNC\…`).
fn local_form(canonical: &str, windows: bool) -> Option<String> {
    let text = if windows {
        canonical.strip_prefix(r"\\?\").unwrap_or(canonical)
    } else {
        canonical
    };
    absolute(text, windows).then(|| text.to_owned())
}

/// A part of a Windows path that Hangar Drive refuses: a reserved device name (also with an
/// extension, `CON.txt`) or a name that ends in a dot or a space, which Windows would drop.
fn awkward_windows_part(part: &str) -> bool {
    if part.ends_with('.') || part.ends_with(' ') {
        return true;
    }
    let stem = part.split('.').next().unwrap_or(part).trim_end();
    RESERVED_NAMES
        .iter()
        .any(|name| stem.eq_ignore_ascii_case(name))
}

/// Whether a path from the window may be offered to Hangar Drive before the file system is asked:
/// local and absolute, no control characters, no UNC or device path, no `..`; on Windows also no
/// reserved device name and no part ending in a dot or a space.
pub fn plain_folder(path: &str, windows: bool) -> bool {
    absolute(path, windows)
        && !(windows && path.split(['\\', '/']).skip(1).any(awkward_windows_part))
}

/// The folder as Hangar Drive gets it: an existing directory on a local fixed drive, resolved.
pub fn checked_folder(path: &str) -> Result<String, DriveError> {
    let windows = cfg!(windows);
    if !plain_folder(path, windows) {
        return Err(DriveError::NotLocalFolder);
    }
    let canonical = fs::canonicalize(path).map_err(|_| DriveError::NotLocalFolder)?;
    if !canonical.is_dir() || !crate::gate::local_fixed_drive(&canonical) {
        return Err(DriveError::NotLocalFolder);
    }
    let text = canonical.to_str().ok_or(DriveError::NotLocalFolder)?;
    local_form(text, windows)
        .filter(|text| plain_folder(text, windows))
        .ok_or(DriveError::NotLocalFolder)
}

/// When each folder was last handed to Hangar Drive, so a second click within
/// [`PROPOSAL_COOLDOWN`] does not send a proposal Hangar Drive would only ignore.
#[derive(Debug, Default)]
pub struct Proposals {
    sent: HashMap<String, Instant>,
}

impl Proposals {
    /// Whether `folder` may be handed over at `now`; records it when it may.
    pub fn allow(&mut self, folder: &str, now: Instant) -> bool {
        self.sent
            .retain(|_, at| now.saturating_duration_since(*at) < PROPOSAL_COOLDOWN);
        if self.sent.contains_key(folder) {
            return false;
        }
        self.sent.insert(folder.to_owned(), now);
        true
    }

    /// Forgets `folder` after its link did not open, so the next click tries again at once.
    pub fn forget(&mut self, folder: &str) {
        self.sent.remove(folder);
    }
}

fn with_proposals<T>(action: impl FnOnce(&mut Proposals) -> T) -> T {
    action(
        PROPOSALS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(Proposals::default),
    )
}

static PROPOSALS: Mutex<Option<Proposals>> = Mutex::new(None);

/// Whether the release build of Hangar Drive is installed for this user.
///
/// Windows: the open command of its link scheme in `HKCU\Software\Classes` names an existing
/// program. Linux: `xdg-mime` names a handler for the scheme. macOS: not shipped yet, so never.
pub fn installed() -> bool {
    #[cfg(windows)]
    {
        use crate::install_source::registry::{open, raw_string_value};
        use windows_sys::Win32::System::Registry::HKEY_CURRENT_USER;

        let key = format!(r"Software\Classes\{SCHEME}\shell\open\command");
        open(HKEY_CURRENT_USER, &key, 0)
            .and_then(|key| raw_string_value(&key, ""))
            .and_then(|command| command_program(&command))
            .is_some_and(|program| program.is_file())
    }
    #[cfg(target_os = "macos")]
    {
        false
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let scheme = format!("x-scheme-handler/{SCHEME}");
        crate::bounded_output::output_within(
            "xdg-mime",
            &["query", "default", &scheme],
            crate::bounded_output::TOOL_LIMIT,
        )
        .is_some_and(|output| !String::from_utf8_lossy(&output).trim().is_empty())
    }
}

/// Whether Hangar Drive is installed and, per path, the state of the folder that holds it. Reads
/// the list only when it is installed: without it, the window shows nothing of Hangar Drive.
pub fn status(paths: &[String]) -> Result<DriveStatus, DriveError> {
    if paths.len() > MAX_PATHS {
        return Err(DriveError::TooMany);
    }
    if !installed() {
        return Ok(DriveStatus {
            installed: false,
            marks: vec![None; paths.len()],
        });
    }
    let folders = folders_file().map_or_else(Vec::new, |file| read_folders(&file));
    Ok(DriveStatus {
        installed: true,
        marks: paths
            .iter()
            .map(|path| mark(&folders, path, cfg!(windows)))
            .collect(),
    })
}

/// Hands `path` to Hangar Drive, which asks the person whether to back it up. A folder handed
/// over less than [`PROPOSAL_COOLDOWN`] ago is not sent again: Hangar Drive's question for it is
/// still open, or Hangar Drive would ignore it.
pub fn add_folder(path: &str) -> Result<(), DriveError> {
    if !installed() {
        return Err(DriveError::NotInstalled);
    }
    let folder = checked_folder(path)?;
    let link = add_folder_link(&folder)?;
    if !with_proposals(|proposals| proposals.allow(&folder, Instant::now())) {
        return Ok(());
    }
    crate::desktop::open_drive_link(&link).map_err(|error| {
        with_proposals(|proposals| proposals.forget(&folder));
        DriveError::Open(error.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    fn decode(text: &str) -> String {
        let bytes = text.as_bytes();
        let mut out = Vec::new();
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'%' {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                index += 3;
            } else {
                out.push(bytes[index]);
                index += 1;
            }
        }
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn the_link_encodes_every_special_character() {
        let path = r"C:\Users\Jörg Müller\a#b&c%d?e+f=g";
        let link = add_folder_link(path).unwrap();
        let text = link.as_str();
        let value = text
            .strip_prefix("hangar-drive://add-folder?path=")
            .unwrap();
        assert_eq!(
            value,
            "C%3A%5CUsers%5CJ%C3%B6rg%20M%C3%BCller%5Ca%23b%26c%25d%3Fe%2Bf%3Dg"
        );
        assert!(!value.contains(['#', '&', ' ', '?', '+', '=']));
        assert_eq!(decode(value), path);
    }

    #[test]
    fn the_link_keeps_unreserved_characters_and_unix_paths() {
        let link = add_folder_link("/home/a/my-proj_1.0~x").unwrap();
        assert_eq!(
            link.as_str(),
            "hangar-drive://add-folder?path=%2Fhome%2Fa%2Fmy-proj_1.0~x"
        );
    }

    #[test]
    fn the_longest_path_still_gives_a_link_within_the_limit() {
        let longest = "\u{e4}".repeat(MAX_PATH_LEN / 2);
        assert_eq!(longest.len(), MAX_PATH_LEN);
        let link = add_folder_link(&longest).unwrap();
        assert!(link.as_str().len() <= MAX_LINK_LEN);
        assert_eq!(
            add_folder_link(&"a%".repeat(MAX_LINK_LEN / 2)),
            Err(DriveError::LinkTooLong)
        );
    }

    #[test]
    fn a_plus_is_encoded_and_the_link_has_one_parameter_and_no_fragment() {
        let link = add_folder_link(r"C:\a+b#c&path=d").unwrap();
        let text = link.as_str();
        assert!(text.ends_with("C%3A%5Ca%2Bb%23c%26path%3Dd"), "{text}");
        assert_eq!(text.matches('?').count(), 1);
        assert_eq!(text.matches('=').count(), 1);
        assert!(!text.contains(['#', '&', '+']));
    }

    #[test]
    fn reserved_names_and_trailing_dots_or_spaces_are_refused_on_windows() {
        for path in [
            r"C:\CON",
            r"C:\a\con",
            r"C:\a\Nul.txt",
            r"C:\a\aux.tar.gz",
            r"C:\a\COM1\b",
            r"C:\a\lpt9",
            r"C:\a\prn .txt",
            r"C:\a\proj.",
            r"C:\a\proj \b",
            "C:/a/proj./b",
        ] {
            assert!(!plain_folder(path, true), "{path:?}");
        }
        for path in [
            r"C:\a\console",
            r"C:\a\com10",
            r"C:\a\lpt",
            r"C:\a\.git",
            r"C:\a\my proj",
            r"C:\a\v1.2",
            r"C:\",
        ] {
            assert!(plain_folder(path, true), "{path:?}");
        }
        assert!(plain_folder("/home/a/con", false));
        assert!(plain_folder("/home/a/proj.", false));
    }

    #[test]
    fn the_same_folder_is_handed_over_once_per_cooldown() {
        let start = Instant::now();
        let mut proposals = Proposals::default();
        assert!(proposals.allow(r"C:\a", start));
        assert!(!proposals.allow(r"C:\a", start + Duration::from_secs(29)));
        assert!(proposals.allow(r"C:\b", start + Duration::from_secs(1)));
        assert!(proposals.allow(r"C:\a", start + PROPOSAL_COOLDOWN));
        assert!(!proposals.allow(r"C:\a", start + PROPOSAL_COOLDOWN));
        proposals.forget(r"C:\a");
        assert!(proposals.allow(r"C:\a", start + PROPOSAL_COOLDOWN));
    }

    #[test]
    fn plain_folders_are_local_and_absolute() {
        assert!(plain_folder(r"C:\a\proj", true));
        assert!(plain_folder("C:/a/proj", true));
        assert!(plain_folder(r"C:\", true));
        assert!(plain_folder("/home/a/proj", false));

        for path in [
            "",
            r"a\proj",
            r"\a\proj",
            "C:proj",
            r"\\server\share\proj",
            "//server/share/proj",
            r"\\?\C:\a",
            r"\\.\C:\a",
            r"\\?\UNC\server\share",
            r"C:\a\..\b",
            "C:\\a\nb",
            "C:\\a\u{7}",
        ] {
            assert!(!plain_folder(path, true), "{path:?}");
        }
        for path in ["", "home/a", "./a", "/a/../b", "/a\u{0}b", "/a\tb"] {
            assert!(!plain_folder(path, false), "{path:?}");
        }
        assert!(!plain_folder(
            &format!("C:\\{}", "a".repeat(MAX_PATH_LEN)),
            true
        ));
    }

    #[test]
    fn canonical_paths_lose_the_verbatim_prefix_but_not_a_share() {
        assert_eq!(
            local_form(r"\\?\C:\a\proj", true).as_deref(),
            Some(r"C:\a\proj")
        );
        assert_eq!(local_form(r"\\?\UNC\server\share\proj", true), None);
        assert_eq!(
            local_form("/home/a/proj", false).as_deref(),
            Some("/home/a/proj")
        );
    }

    #[test]
    fn checked_folders_exist_and_are_folders() {
        let dir = TempDir::new("drive-check");
        let folder = dir.path().join("my proj");
        fs::create_dir_all(&folder).unwrap();
        let file = dir.path().join("note.txt");
        fs::write(&file, "x").unwrap();

        let checked = checked_folder(folder.to_str().unwrap()).unwrap();
        assert!(Path::new(&checked).is_dir());
        assert!(!checked.starts_with(r"\\"));
        assert_eq!(
            checked_folder(file.to_str().unwrap()),
            Err(DriveError::NotLocalFolder)
        );
        let missing = dir.path().join("missing");
        assert_eq!(
            checked_folder(missing.to_str().unwrap()),
            Err(DriveError::NotLocalFolder)
        );
        assert_eq!(checked_folder("relative"), Err(DriveError::NotLocalFolder));
    }

    #[test]
    fn a_folder_covers_itself_and_what_lies_inside() {
        assert!(covers(r"C:\a\proj", r"C:\a\proj", true));
        assert!(covers(r"C:\a\proj", r"C:\a\proj\src", true));
        assert!(covers(r"C:\a\proj\", r"C:\a\proj", true));
        assert!(covers(r"c:\A\Proj", r"C:\a\PROJ\src", true));
        assert!(covers("C:/a/proj", r"C:\a\proj\src", true));
        assert!(covers(r"\\?\C:\a\proj", r"C:\a\proj", true));
        assert!(covers(r"C:\", r"C:\a", true));

        assert!(!covers(r"C:\a\proj", r"C:\a\proj2", true));
        assert!(!covers(r"C:\a\proj", r"C:\a", true));
        assert!(!covers(r"C:\a\proj", r"D:\a\proj", true));
        assert!(!covers(r"\\server\share", r"\\server\share\x", true));
        assert!(!covers("a", r"C:\a", true));
    }

    #[test]
    fn unix_paths_compare_with_case() {
        assert!(covers("/home/a/proj", "/home/a/proj/src", false));
        assert!(!covers("/home/a/proj", "/home/a/Proj", false));
        assert!(!covers("/home/a/proj", "/home/a/proj2", false));
        assert!(!covers("home/a", "/home/a", false));
        assert!(!covers("/", "/home", false));
    }

    #[test]
    fn the_innermost_folder_gives_the_mark() {
        let folders = [
            DriveFolder {
                path: r"C:\a".into(),
                state: FolderState::Active,
            },
            DriveFolder {
                path: r"C:\a\proj".into(),
                state: FolderState::Paused,
            },
        ];
        assert_eq!(
            mark(&folders, r"C:\a\proj\src", true),
            Some(FolderState::Paused)
        );
        assert_eq!(
            mark(&folders, r"C:\a\other", true),
            Some(FolderState::Active)
        );
        assert_eq!(mark(&folders, r"D:\a", true), None);
    }

    fn sample(path: &str) -> String {
        serde_json::json!({
            "contract": "drive-folders 1",
            "appVersion": "0.1.0",
            "updated": 1_764_300_000_000_u64,
            "setUp": true,
            "folders": [
                { "path": path, "name": "proj", "state": "active" },
                { "path": path, "name": "proj", "state": "paused" },
                { "path": path, "name": "proj", "state": "stopped" },
                { "path": path, "name": "proj", "state": "syncing-later" },
                { "path": path, "name": "proj" },
                { "path": "relative", "state": "active" },
                { "path": 42, "state": "active" },
                { "name": "no path" },
                "not an object"
            ]
        })
        .to_string()
    }

    fn local_sample_path() -> &'static str {
        if cfg!(windows) {
            r"C:\a\proj"
        } else {
            "/a/proj"
        }
    }

    #[test]
    fn the_folder_list_keeps_usable_entries_and_tolerates_unknown_states() {
        let folders = parse_folders(sample(local_sample_path()).as_bytes());
        let states: Vec<FolderState> = folders.iter().map(|folder| folder.state).collect();
        assert_eq!(
            states,
            [
                FolderState::Active,
                FolderState::Paused,
                FolderState::Stopped,
                FolderState::Other,
                FolderState::Other,
            ]
        );
        assert!(folders
            .iter()
            .all(|folder| folder.path == local_sample_path()));
    }

    #[test]
    fn a_broken_or_foreign_list_gives_no_folders() {
        assert!(parse_folders(b"").is_empty());
        assert!(parse_folders(b"{ not json").is_empty());
        assert!(parse_folders(b"[]").is_empty());
        assert!(
            parse_folders(br#"{"contract":"drive-folders 2","folders":[{"path":"/a"}]}"#)
                .is_empty()
        );
        assert!(parse_folders(br#"{"folders":[{"path":"/a"}]}"#).is_empty());
        assert!(parse_folders(br#"{"contract":"drive-folders 1"}"#).is_empty());
        assert!(parse_folders(br#"{"contract":"drive-folders 1","folders":{}}"#).is_empty());
    }

    #[test]
    fn the_folder_file_is_read_within_its_limit() {
        let dir = TempDir::new("drive-folders");
        let file = dir.path().join("folders.json");
        assert!(read_folders(&file).is_empty());

        fs::write(&file, sample(local_sample_path())).unwrap();
        assert_eq!(read_folders(&file).len(), 5);

        let mut large = sample(local_sample_path()).into_bytes();
        large.resize(usize::try_from(MAX_FOLDERS_BYTES).unwrap() + 1, b' ');
        fs::write(&file, &large).unwrap();
        assert!(read_folders(&file).is_empty());
    }

    #[test]
    fn the_folder_file_sits_in_the_public_folder() {
        let file = folders_file().unwrap();
        assert!(file.ends_with(
            Path::new("Hangar Drive")
                .join("public")
                .join("folders.json")
        ));
    }

    #[test]
    fn the_open_command_names_the_program() {
        assert_eq!(
            command_program(
                r#""C:\Users\Jörg Müller\AppData\Local\Hangar Drive\hangar-drive.exe" "%1""#
            ),
            Some(PathBuf::from(
                r"C:\Users\Jörg Müller\AppData\Local\Hangar Drive\hangar-drive.exe"
            ))
        );
        assert_eq!(
            command_program(r"C:\Tools\hangar-drive.EXE %1"),
            Some(PathBuf::from(r"C:\Tools\hangar-drive.EXE"))
        );
        assert_eq!(
            command_program(r#"  "C:\x\hangar-drive.exe"  "#),
            Some(PathBuf::from(r"C:\x\hangar-drive.exe"))
        );

        for command in [
            "",
            "   ",
            r#""C:\x\hangar-drive.exe"#,
            r#""hangar-drive.exe" "%1""#,
            r#""C:\x\hangar-drive.cmd" "%1""#,
            r#""\\server\share\hangar-drive.exe" "%1""#,
            r#"rundll32 "C:\x\hangar-drive.exe""#,
            "\"C:\\x\\hangar\u{1}drive.exe\" \"%1\"",
        ] {
            assert_eq!(command_program(command), None, "{command:?}");
        }
    }

    #[test]
    fn too_many_paths_are_refused() {
        let paths = vec![String::from("/a"); MAX_PATHS + 1];
        assert_eq!(status(&paths), Err(DriveError::TooMany));
    }
}
