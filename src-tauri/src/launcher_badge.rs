//! The number of waiting sessions on the launcher icon on Linux (ADR 0013, evidence:
//! docs/wiki/pages/sources/launcher-entry-and-macos-usage-reading.md).
//!
//! Docks that show counts (KDE Plasma's task manager, Dash to Dock, Plank) listen for the
//! `Update` signal of `com.canonical.Unity.LauncherEntry` on the session bus and match its
//! `application://<desktop id>` against the desktop file the icon came from. Hangar sends the
//! signal itself through zbus: Tauri's badge count goes through libunity and only while the
//! Unity shell runs. The desktop id depends on how Hangar was installed and started
//! ([`desktop_id`]).

/// The desktop file the Tauri bundler writes for the .deb (`<productName>.desktop`), which
/// Tauri's own badge count names too.
pub const DEFAULT_DESKTOP_ID: &str = "Hangar.desktop";
/// Prefix of the desktop files AppImageLauncher writes for an integrated AppImage.
const APPIMAGELAUNCHER_PREFIX: &str = "appimagekit_";

/// What the environment says about how Hangar was started.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Launch<'a> {
    /// `GIO_LAUNCHED_DESKTOP_FILE`: the desktop file a GLib-based launcher started from.
    pub gio_desktop_file: Option<&'a str>,
    /// `GIO_LAUNCHED_DESKTOP_FILE_PID`: the process the launcher started.
    pub gio_pid: Option<&'a str>,
    /// `APPIMAGE`: the AppImage file Hangar runs from.
    pub appimage: Option<&'a str>,
}

/// The desktop id the launcher icon belongs to, most reliable source first:
///
/// 1. The desktop file a GLib launcher names, when it started this process or its parent (an
///    AppImage's runtime starts Hangar as a child). A variable inherited from a terminal that was
///    itself started from a desktop file names the terminal and is not taken.
/// 2. For an AppImage, the desktop file AppImageLauncher wrote for exactly this file
///    (`entries`: name and content of the user's desktop files).
/// 3. [`DEFAULT_DESKTOP_ID`].
pub fn desktop_id(
    launch: Launch<'_>,
    own: u32,
    parent: u32,
    entries: &[(String, String)],
) -> String {
    let launched_us = launch
        .gio_pid
        .and_then(|pid| pid.trim().parse::<u32>().ok())
        .is_some_and(|pid| pid == own || pid == parent);
    if launched_us {
        let name = launch
            .gio_desktop_file
            .and_then(|path| path.rsplit('/').next())
            .filter(|name| name.len() > ".desktop".len() && name.ends_with(".desktop"));
        if let Some(name) = name {
            return name.to_owned();
        }
    }
    if let Some(appimage) = launch.appimage.filter(|path| path.starts_with('/')) {
        let integrated = entries.iter().find(|(name, content)| {
            name.starts_with(APPIMAGELAUNCHER_PREFIX)
                && name.ends_with(".desktop")
                && content.lines().any(|line| {
                    line.strip_prefix("Exec=")
                        .is_some_and(|exec| exec.split_whitespace().next() == Some(appimage))
                })
        });
        if let Some((name, _)) = integrated {
            return name.clone();
        }
    }
    DEFAULT_DESKTOP_ID.to_owned()
}

/// The `app_uri` of the signal.
pub fn app_uri(desktop_id: &str) -> String {
    format!("application://{desktop_id}")
}

/// The signal's properties: the count, and whether it shows (0 hides it).
pub fn properties(count: u32) -> (i64, bool) {
    (i64::from(count), count > 0)
}

/// A desktop file is a few hundred bytes; anything larger than this is not one.
pub const MAX_ENTRY_BYTES: u64 = 64 * 1024;

/// The text of a small regular file, at most `limit` bytes; `None` for anything else. The checks
/// run on the opened handle, so a symbolic link counts by its target and a FIFO, a device or a
/// file that grows meanwhile cannot make the read block or run long. On Unix the file is opened
/// without blocking, so opening a FIFO returns at once.
pub fn read_small_file(path: &std::path::Path, limit: u64) -> Option<String> {
    use std::io::Read;

    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > limit {
        return None;
    }
    let mut text = String::new();
    file.take(limit).read_to_string(&mut text).ok()?;
    Some(text)
}

/// Name and content of AppImageLauncher's desktop files in `folder`.
pub fn integrated_entries(folder: &std::path::Path) -> Vec<(String, String)> {
    let Ok(files) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    files
        .flatten()
        .filter_map(|file| {
            let name = file.file_name().into_string().ok()?;
            if !name.starts_with(APPIMAGELAUNCHER_PREFIX) || !name.ends_with(".desktop") {
                return None;
            }
            Some((name, read_small_file(&file.path(), MAX_ENTRY_BYTES)?))
        })
        .collect()
}

#[cfg(target_os = "linux")]
mod imp {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;

    use zbus::blocking::Connection;
    use zbus::zvariant::Value;

    use super::{app_uri, desktop_id, integrated_entries, properties, Launch};

    const PATH: &str = "/com/feoren/hangar/LauncherEntry";
    const INTERFACE: &str = "com.canonical.Unity.LauncherEntry";

    /// What the first count found out: the `app_uri`, and the session bus, kept open and dropped
    /// after a failure until the next count asks again.
    struct State {
        app_uri: String,
        connection: Option<Connection>,
    }

    /// Nothing is read at start: the desktop id and the bus come with the first count, on the
    /// blocking thread of `set_launcher_count`.
    #[derive(Default)]
    pub struct LauncherBadge {
        state: Mutex<Option<State>>,
    }

    /// The user's applications folder (`$XDG_DATA_HOME/applications`, by default
    /// `~/.local/share/applications`).
    fn applications_folder() -> Option<PathBuf> {
        let data = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|home| home.is_absolute())
                    .map(|home| home.join(".local/share"))
            })?;
        Some(data.join("applications"))
    }

    fn find_app_uri() -> String {
        let gio_desktop_file = std::env::var("GIO_LAUNCHED_DESKTOP_FILE").ok();
        let gio_pid = std::env::var("GIO_LAUNCHED_DESKTOP_FILE_PID").ok();
        let appimage = std::env::var("APPIMAGE").ok();
        let entries = match (&appimage, applications_folder()) {
            (Some(_), Some(folder)) => integrated_entries(Path::new(&folder)),
            _ => Vec::new(),
        };
        let launch = Launch {
            gio_desktop_file: gio_desktop_file.as_deref(),
            gio_pid: gio_pid.as_deref(),
            appimage: appimage.as_deref(),
        };
        app_uri(&desktop_id(
            launch,
            std::process::id(),
            std::os::unix::process::parent_id(),
            &entries,
        ))
    }

    impl LauncherBadge {
        /// Sends the count; `false` when the session bus cannot be reached.
        pub fn set(&self, count: u32) -> bool {
            let mut state = crate::lock(&self.state);
            let state = state.get_or_insert_with(|| State {
                app_uri: find_app_uri(),
                connection: None,
            });
            if state.connection.is_none() {
                state.connection = Connection::session().ok();
            }
            let Some(bus) = state.connection.as_ref() else {
                return false;
            };
            let (count, visible) = properties(count);
            let mut changes: HashMap<&str, Value<'_>> = HashMap::new();
            changes.insert("count", Value::from(count));
            changes.insert("count-visible", Value::from(visible));
            let sent = bus.emit_signal(
                None::<&str>,
                PATH,
                INTERFACE,
                "Update",
                &(state.app_uri.as_str(), changes),
            );
            if let Err(error) = sent {
                eprintln!("hangar: the launcher count was not sent: {error}");
                state.connection = None;
                return false;
            }
            true
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    /// Windows draws an overlay icon and macOS sets the Dock badge from the window; nothing to
    /// send here. A private field so callers build it with `default()` on every platform.
    #[derive(Default)]
    pub struct LauncherBadge {
        _private: (),
    }

    impl LauncherBadge {
        pub fn set(&self, _count: u32) -> bool {
            false
        }
    }
}

pub use imp::LauncherBadge;

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, exec: &str) -> (String, String) {
        (
            name.to_owned(),
            format!("[Desktop Entry]\nName=Hangar\nExec={exec}\nType=Application\n"),
        )
    }

    #[test]
    fn a_glib_launcher_names_the_desktop_file() {
        let launch = Launch {
            gio_desktop_file: Some("/usr/share/applications/Hangar.desktop"),
            gio_pid: Some("4242"),
            appimage: None,
        };
        assert_eq!(desktop_id(launch, 4242, 1, &[]), "Hangar.desktop");
        // An AppImage's runtime is the process the launcher started; Hangar is its child.
        let integrated = Launch {
            gio_desktop_file: Some(
                "/home/u/.local/share/applications/appimagekit_ab-Hangar.desktop",
            ),
            gio_pid: Some("4000"),
            appimage: Some("/home/u/Applications/Hangar.AppImage"),
        };
        assert_eq!(
            desktop_id(integrated, 4242, 4000, &[]),
            "appimagekit_ab-Hangar.desktop"
        );
    }

    #[test]
    fn a_desktop_file_inherited_from_a_terminal_is_not_ours() {
        let launch = Launch {
            gio_desktop_file: Some("/usr/share/applications/org.gnome.Terminal.desktop"),
            gio_pid: Some("1200"),
            appimage: None,
        };
        assert_eq!(desktop_id(launch, 4242, 3000, &[]), DEFAULT_DESKTOP_ID);
        let without_pid = Launch {
            gio_pid: None,
            ..launch
        };
        assert_eq!(desktop_id(without_pid, 4242, 3000, &[]), DEFAULT_DESKTOP_ID);
    }

    #[test]
    fn an_odd_desktop_file_name_is_ignored() {
        for path in ["/usr/share/applications/", "/x/.desktop", "/x/Hangar"] {
            let launch = Launch {
                gio_desktop_file: Some(path),
                gio_pid: Some("7"),
                appimage: None,
            };
            assert_eq!(desktop_id(launch, 7, 1, &[]), DEFAULT_DESKTOP_ID, "{path}");
        }
    }

    #[test]
    fn an_integrated_appimage_finds_its_own_desktop_file() {
        let appimage = "/home/u/Applications/Hangar-0.13.0.AppImage";
        let entries = [
            entry(
                "appimagekit_11-Other.desktop",
                "/home/u/Applications/Other.AppImage %U",
            ),
            entry("appimagekit_22-Hangar.desktop", &format!("{appimage} %U")),
            entry("Hangar.desktop", appimage),
        ];
        let launch = Launch {
            appimage: Some(appimage),
            ..Launch::default()
        };
        assert_eq!(
            desktop_id(launch, 1, 1, &entries),
            "appimagekit_22-Hangar.desktop"
        );
        // Another copy of the AppImage, or a path that only starts the same, is not this one.
        let other = Launch {
            appimage: Some("/home/u/Applications/Hangar-0.13"),
            ..Launch::default()
        };
        assert_eq!(desktop_id(other, 1, 1, &entries), DEFAULT_DESKTOP_ID);
        // Without integration the AppImage has no desktop file of its own.
        assert_eq!(desktop_id(launch, 1, 1, &[]), DEFAULT_DESKTOP_ID);
    }

    #[test]
    fn the_signal_names_the_desktop_file_and_hides_zero() {
        assert_eq!(app_uri("Hangar.desktop"), "application://Hangar.desktop");
        assert_eq!(properties(0), (0, false));
        assert_eq!(properties(3), (3, true));
    }

    #[test]
    fn only_small_regular_desktop_files_are_read() {
        let dir = crate::test_support::TempDir::new("launcher-entries");
        let small = dir.path().join("appimagekit_1-Hangar.desktop");
        std::fs::write(&small, "[Desktop Entry]\nExec=/a/Hangar.AppImage\n").unwrap();
        let big = dir.path().join("appimagekit_2-Big.desktop");
        let size = usize::try_from(MAX_ENTRY_BYTES).unwrap() + 1;
        std::fs::write(&big, "x".repeat(size)).unwrap();
        std::fs::create_dir(dir.path().join("appimagekit_3-Folder.desktop")).unwrap();
        std::fs::write(dir.path().join("Hangar.desktop"), "[Desktop Entry]\n").unwrap();
        std::fs::write(dir.path().join("appimagekit_4-Notes.txt"), "x").unwrap();
        assert_eq!(read_small_file(&big, MAX_ENTRY_BYTES), None);
        assert_eq!(read_small_file(dir.path(), MAX_ENTRY_BYTES), None);
        assert_eq!(
            read_small_file(&dir.path().join("missing"), MAX_ENTRY_BYTES),
            None
        );
        let entries = integrated_entries(dir.path());
        assert_eq!(entries.len(), 1, "{entries:?}");
        assert_eq!(entries[0].0, "appimagekit_1-Hangar.desktop");
        assert!(integrated_entries(&dir.path().join("missing")).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn links_count_by_their_target_and_a_fifo_never_blocks() {
        let dir = crate::test_support::TempDir::new("launcher-links");
        let big = dir.path().join("big");
        let size = usize::try_from(MAX_ENTRY_BYTES).unwrap() + 1;
        std::fs::write(&big, "x".repeat(size)).unwrap();
        let link = dir.path().join("appimagekit_1-Link.desktop");
        std::os::unix::fs::symlink(&big, &link).unwrap();
        assert_eq!(read_small_file(&link, MAX_ENTRY_BYTES), None);
        let fifo = dir.path().join("appimagekit_2-Fifo.desktop");
        let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: `name` is a NUL-terminated path inside the test folder.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let started = std::time::Instant::now();
        assert_eq!(read_small_file(&fifo, MAX_ENTRY_BYTES), None);
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert!(integrated_entries(dir.path()).is_empty());
    }

    #[test]
    fn other_platforms_send_nothing() {
        if !cfg!(target_os = "linux") {
            assert!(!LauncherBadge::default().set(1));
        }
    }
}
