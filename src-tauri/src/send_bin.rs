//! The folder sessions with partner projects get at the end of their `PATH` (ADR 0012): it holds
//! a copy of `hangar-send` and nothing else.
//!
//! The client is built and bundled next to Hangar's own executable (a `[[bin]]` of this crate,
//! which Tauri's bundler installs beside the main one). That folder itself must not go on `PATH`:
//! it holds Hangar and the uninstaller on Windows, and on Linux it is `/usr/bin`, whose order
//! against `/usr/local/bin` the user set. So Hangar copies the client into `<local data>/bin` at
//! startup, when it is missing or differs, and hands sessions that folder.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// File name of the client on this platform.
pub const FILE_NAME: &str = if cfg!(windows) {
    "hangar-send.exe"
} else {
    "hangar-send"
};
/// Folder below the local data folder.
pub const DIR: &str = "bin";
/// A larger file is no client of ours; nothing is copied.
const MAX_BYTES: u64 = 32 * 1024 * 1024;

/// Copies `hangar-send` from `exe_dir` into `<data_dir>/bin` when the copy there is missing or
/// differs, and answers that folder. `None` when there is no client next to Hangar (a dev build
/// that did not build it): then no session is offered sending. A copy that cannot be replaced
/// because a session runs it right now (Windows) is kept as it is.
pub fn install(exe_dir: &Path, data_dir: &Path) -> io::Result<Option<PathBuf>> {
    let source = exe_dir.join(FILE_NAME);
    let meta = match fs::metadata(&source) {
        Ok(meta) if meta.is_file() && meta.len() <= MAX_BYTES => meta,
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let dir = data_dir.join(DIR);
    let target = dir.join(FILE_NAME);
    if same_content(&source, &target, meta.len())? {
        return Ok(Some(dir));
    }
    fs::create_dir_all(&dir)?;
    let partial = dir.join(format!(".{FILE_NAME}.{}.part", std::process::id()));
    fs::copy(&source, &partial)?;
    match fs::rename(&partial, &target) {
        Ok(()) => Ok(Some(dir)),
        Err(error) => {
            let _ = fs::remove_file(&partial);
            if target.is_file() {
                eprintln!(
                    "hangar: {} not updated, the old copy stays: {error}",
                    target.display()
                );
                Ok(Some(dir))
            } else {
                Err(error)
            }
        }
    }
}

fn same_content(source: &Path, target: &Path, length: u64) -> io::Result<bool> {
    match fs::metadata(target) {
        Ok(meta) if meta.is_file() && meta.len() == length => {}
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    }
    Ok(fs::read(source)? == fs::read(target)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn the_client_is_copied_once_and_replaced_when_it_changes() {
        let tmp = TempDir::new("send-bin");
        let exe = tmp.path().join("app");
        let data = tmp.path().join("data");
        fs::create_dir_all(&exe).unwrap();
        // No client next to Hangar: nothing to offer.
        assert_eq!(install(&exe, &data).unwrap(), None);
        assert!(!data.join(DIR).exists());

        fs::write(exe.join(FILE_NAME), b"client v1").unwrap();
        let dir = install(&exe, &data).unwrap().unwrap();
        assert_eq!(dir, data.join(DIR));
        assert_eq!(fs::read(dir.join(FILE_NAME)).unwrap(), b"client v1");
        // Only the client lives there.
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);

        fs::write(exe.join(FILE_NAME), b"client v2").unwrap();
        install(&exe, &data).unwrap();
        assert_eq!(fs::read(dir.join(FILE_NAME)).unwrap(), b"client v2");
        // Same length, other bytes: still replaced.
        fs::write(exe.join(FILE_NAME), b"client v3").unwrap();
        install(&exe, &data).unwrap();
        assert_eq!(fs::read(dir.join(FILE_NAME)).unwrap(), b"client v3");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
    }

    #[test]
    fn a_folder_in_place_of_the_client_is_no_client() {
        let tmp = TempDir::new("send-bin-folder");
        let exe = tmp.path().join("app");
        fs::create_dir_all(exe.join(FILE_NAME)).unwrap();
        assert_eq!(install(&exe, &tmp.path().join("data")).unwrap(), None);
    }
}
