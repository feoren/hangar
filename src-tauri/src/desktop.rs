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
    let mut command = Command::new(program);
    command.args(args);
    spawn_detached(command)
}

/// Program and arguments that show `file` selected in the file manager. On Windows the single
/// argument is `/select,"<path>"` and must be passed raw ([`reveal`]): explorer.exe parses its
/// command line itself and does not understand the C runtime quoting of a whole argument. A
/// Windows path cannot contain `"`, so the quotes cannot be broken out of. Linux has no common
/// way to select a file; the folder opens instead.
pub fn reveal_command(file: &Path) -> (PathBuf, Vec<std::ffi::OsString>) {
    #[cfg(windows)]
    {
        let (program, _) = command_for(file, Target::Folder);
        let mut arg = std::ffi::OsString::from("/select,\"");
        arg.push(file.as_os_str());
        arg.push("\"");
        (program, vec![arg])
    }
    #[cfg(target_os = "macos")]
    {
        (
            PathBuf::from("/usr/bin/open"),
            vec!["-R".into(), file.as_os_str().to_owned()],
        )
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let folder = file.parent().unwrap_or(file);
        command_for(folder, Target::Folder)
    }
}

/// Shows `file` selected in Explorer or Finder; callers pass only paths from Hangar's own storage.
pub fn reveal(file: &Path) -> io::Result<()> {
    let (program, args) = reveal_command(file);
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        for arg in &args {
            command.raw_arg(arg);
        }
    }
    #[cfg(not(windows))]
    command.args(args);
    spawn_detached(command)
}

fn spawn_detached(mut command: Command) -> io::Result<()> {
    let mut child = command
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
    fn reveal_selects_the_file() {
        let file = if cfg!(windows) {
            Path::new(r"C:\Users\Test User\notes\a & b.md")
        } else {
            Path::new("/home/test user/notes/a & b.md")
        };
        let (program, args) = reveal_command(file);
        let name = program.file_name().and_then(|n| n.to_str()).unwrap_or("");
        #[cfg(windows)]
        {
            assert_eq!(name, "explorer.exe");
            assert_eq!(
                args,
                [std::ffi::OsString::from(
                    r#"/select,"C:\Users\Test User\notes\a & b.md""#
                )]
            );
        }
        #[cfg(target_os = "macos")]
        {
            assert_eq!(name, "open");
            assert_eq!(args.len(), 2);
            assert_eq!(args[1], file.as_os_str());
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            assert_eq!(name, "xdg-open");
            assert_eq!(args, [file.parent().unwrap().as_os_str().to_owned()]);
        }
    }

    #[test]
    fn folder_without_editor_flag() {
        let (_, args) = command_for(Path::new("x"), Target::Folder);
        assert_eq!(args.len(), 1);
    }
}
