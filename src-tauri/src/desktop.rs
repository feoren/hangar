//! Opens folders and files with the program the operating system assigns to them.
//!
//! Callers only pass paths the backend determined itself (notes folder, profiles.toml).
//! The frontend never names a path, so this never becomes a command for opening arbitrary files.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// What gets opened. On macOS this decides on `open -t`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Show in the file manager.
    Folder,
    /// Text file in the default editor. On macOS with `-t`, because `open` otherwise rejects a `.toml`
    /// without an associated program; Windows (Explorer) and Linux (`xdg-open`) pick the
    /// associated program or ask.
    TextFile,
}

/// Program and arguments, without a shell. Separate from [`open`] so it can be tested.
pub fn command_for(path: &Path, target: Target) -> (PathBuf, Vec<std::ffi::OsString>) {
    #[cfg(windows)]
    let program = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("explorer.exe");
    #[cfg(target_os = "macos")]
    let program = PathBuf::from("/usr/bin/open");
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = PathBuf::from("xdg-open");

    let mut args = Vec::new();
    if cfg!(target_os = "macos") && target == Target::TextFile {
        args.push("-t".into());
    }
    args.push(path.as_os_str().to_owned());
    (program, args)
}

pub fn open(path: &Path, target: Target) -> io::Result<()> {
    let (program, args) = command_for(path, target);
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    // Reap it so no zombie remains on Unix; explorer.exe exits right away anyway.
    std::thread::Builder::new()
        .name("open-wait".into())
        .spawn(move || {
            let _ = child.wait();
        })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_is_a_single_argument_without_shell() {
        let path = Path::new("Folder with spaces & symbols/profiles.toml");
        let (program, args) = command_for(path, Target::TextFile);
        assert_eq!(args.last().map(|a| a.as_os_str()), Some(path.as_os_str()));
        let name = program.file_name().and_then(|n| n.to_str()).unwrap_or("");
        #[cfg(windows)]
        assert_eq!(name, "explorer.exe");
        #[cfg(target_os = "macos")]
        {
            assert_eq!(name, "open");
            assert_eq!(args.first().and_then(|a| a.to_str()), Some("-t"));
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        assert_eq!(name, "xdg-open");
        if !cfg!(target_os = "macos") {
            assert_eq!(args.len(), 1);
        }
    }

    #[test]
    fn folder_without_editor_flag() {
        let (_, args) = command_for(Path::new("x"), Target::Folder);
        assert_eq!(args.len(), 1);
    }
}
