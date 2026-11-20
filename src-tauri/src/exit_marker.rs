//! Whether the run before this one ended cleanly (wiki: session-restore, section "After a
//! crash").
//!
//! At the start the backend writes a small marker file into the local app data folder and removes
//! it on every clean quit. A marker found at the start means the previous run never got that far:
//! it crashed, was killed or the machine lost power. The window then tells the person that it
//! restored the sessions after a crash.
//!
//! Every quit path removes the marker: `RunEvent::Exit` (closing the window with quit, the tray's
//! Quit, the restart of the process plugin) and Tauri's `cleanup_before_exit`, which also runs
//! where no `Exit` event comes: the Windows updater before it starts the installer and a restart
//! requested on the main thread (verified in tauri 2.12.5 `app.rs` and tauri-plugin-updater
//! 2.14.1). That second hook is a resource in the app's resource table, which
//! `cleanup_before_exit` drops.
//!
//! Release builds allow one instance, debug builds run next to it (`single_instance.rs`), so each
//! kind has its own marker: a dev build would otherwise report a crash whenever the installed
//! Hangar is running and remove the installed one's marker when it quits.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::persist::{read_limited, remove_if_exists, write_atomic};

pub const FILE_NAME: &str = if cfg!(debug_assertions) {
    "running-debug.marker"
} else {
    "running.marker"
};
/// What the marker holds. Its presence is what counts; the text only tells a person who finds
/// the file what it is.
const CONTENT: &[u8] = b"Hangar is running. Removed when Hangar quits cleanly.\n";
/// Reading stops here; anything larger is no marker of ours, but still something left behind.
const MAX_BYTES: usize = 4 * 1024;

/// How the previous run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LastExit {
    /// No marker: a clean quit, or the first start with a version that writes one.
    Clean,
    /// A marker was left behind: crash, kill or power loss.
    Unclean,
}

/// The previous run's end from what was found at the marker's path. Only a missing file counts
/// as clean: a damaged marker, a folder or an unreadable file in its place was left behind as
/// well, and a wrong notice costs less than a missing one.
pub fn judge(found: &Result<Option<Vec<u8>>, String>) -> LastExit {
    match found {
        Ok(None) => LastExit::Clean,
        Ok(Some(_)) | Err(_) => LastExit::Unclean,
    }
}

/// The marker of this run.
#[derive(Debug)]
pub struct ExitMarker {
    /// `None`: no data folder known; nothing is written and every start counts as clean.
    path: Option<PathBuf>,
    last: LastExit,
}

impl ExitMarker {
    /// Reads what the previous run left in `dir`, then writes the marker of this run. A failed
    /// write is logged; the app runs on, the next start then cannot tell a crash.
    pub fn arm(dir: Option<&Path>) -> Self {
        let path = dir.map(|dir| dir.join(FILE_NAME));
        let last = path.as_deref().map_or(LastExit::Clean, |path| {
            judge(&read_limited(path, MAX_BYTES))
        });
        if let Some(path) = &path {
            if let Err(error) = write_atomic(path, CONTENT) {
                eprintln!(
                    "hangar: exit marker {} not written: {error}",
                    path.display()
                );
            }
        }
        Self { path, last }
    }

    pub fn last_exit(&self) -> LastExit {
        self.last
    }

    /// Removes the marker: this run is quitting cleanly. Safe to call more than once.
    pub fn disarm(&self) {
        let Some(path) = &self.path else {
            return;
        };
        if let Err(error) = remove_if_exists(path) {
            eprintln!(
                "hangar: exit marker {} not removed: {error}",
                path.display()
            );
        }
    }
}

/// Removes the marker when Tauri clears the app's resource table in `cleanup_before_exit`.
pub struct DisarmOnCleanup(pub Arc<ExitMarker>);

impl tauri::Resource for DisarmOnCleanup {}

impl Drop for DisarmOnCleanup {
    fn drop(&mut self) {
        self.0.disarm();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn only_a_missing_marker_means_a_clean_end() {
        assert_eq!(judge(&Ok(None)), LastExit::Clean);
        assert_eq!(judge(&Ok(Some(CONTENT.to_vec()))), LastExit::Unclean);
        assert_eq!(
            judge(&Ok(Some(b"\xff\x00garbage".to_vec()))),
            LastExit::Unclean
        );
        assert_eq!(judge(&Ok(Some(Vec::new()))), LastExit::Unclean);
        assert_eq!(judge(&Err("not a regular file".into())), LastExit::Unclean);
    }

    #[test]
    fn a_start_without_marker_was_clean_and_leaves_one() {
        let dir = TempDir::new("exit-marker-first");
        let marker = ExitMarker::arm(Some(dir.path()));
        assert_eq!(marker.last_exit(), LastExit::Clean);
        assert_eq!(
            std::fs::read(dir.path().join(FILE_NAME)).unwrap(),
            CONTENT.to_vec()
        );
    }

    #[test]
    fn a_clean_quit_removes_the_marker() {
        let dir = TempDir::new("exit-marker-clean");
        let marker = ExitMarker::arm(Some(dir.path()));
        marker.disarm();
        marker.disarm();
        assert!(!dir.path().join(FILE_NAME).exists());
        assert_eq!(
            ExitMarker::arm(Some(dir.path())).last_exit(),
            LastExit::Clean
        );
    }

    #[test]
    fn a_marker_left_behind_means_a_crash() {
        let dir = TempDir::new("exit-marker-crash");
        // The previous run ends without disarming, as a crash or a kill would.
        drop(ExitMarker::arm(Some(dir.path())));
        let next = ExitMarker::arm(Some(dir.path()));
        assert_eq!(next.last_exit(), LastExit::Unclean);
        // This run writes its own marker again; after a clean quit the start after is clean.
        assert!(dir.path().join(FILE_NAME).exists());
        next.disarm();
        assert_eq!(
            ExitMarker::arm(Some(dir.path())).last_exit(),
            LastExit::Clean
        );
    }

    #[test]
    fn a_damaged_or_oversized_marker_counts_as_a_crash() {
        let dir = TempDir::new("exit-marker-damaged");
        let path = dir.path().join(FILE_NAME);
        std::fs::write(&path, b"\xff\xfe half").unwrap();
        assert_eq!(
            ExitMarker::arm(Some(dir.path())).last_exit(),
            LastExit::Unclean
        );
        std::fs::write(&path, vec![b'x'; MAX_BYTES + 1]).unwrap();
        assert_eq!(
            ExitMarker::arm(Some(dir.path())).last_exit(),
            LastExit::Unclean
        );
    }

    #[test]
    fn a_folder_in_place_of_the_marker_counts_as_a_crash() {
        let dir = TempDir::new("exit-marker-folder");
        std::fs::create_dir(dir.path().join(FILE_NAME)).unwrap();
        let marker = ExitMarker::arm(Some(dir.path()));
        assert_eq!(marker.last_exit(), LastExit::Unclean);
        // Removing fails on a folder; that is logged, not a panic.
        marker.disarm();
    }

    #[test]
    fn without_a_data_folder_every_start_is_clean() {
        let marker = ExitMarker::arm(None);
        assert_eq!(marker.last_exit(), LastExit::Clean);
        marker.disarm();
    }

    #[test]
    fn the_cleanup_resource_removes_the_marker_when_dropped() {
        let dir = TempDir::new("exit-marker-cleanup");
        let marker = Arc::new(ExitMarker::arm(Some(dir.path())));
        drop(DisarmOnCleanup(marker.clone()));
        assert!(!dir.path().join(FILE_NAME).exists());
    }
}
