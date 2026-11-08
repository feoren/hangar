//! Finds the executable program for a profile command.
//!
//! A search of our own instead of relying on CreateProcess: this gives a clear error message
//! ("not found") instead of a raw OS error, and on Windows `codex.cmd` wins over the
//! extensionless Unix shell script `codex` that npm places next to it.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

pub fn resolve_program(
    command: &str,
    path: Option<&OsStr>,
    pathext: Option<&OsStr>,
) -> Option<PathBuf> {
    let candidates = extension_candidates(command, pathext);
    let direct = Path::new(command);
    if direct.is_absolute() || command.contains('/') || command.contains('\\') {
        return find_with_extensions(direct, &candidates);
    }
    std::env::split_paths(path?)
        .filter(|dir| !dir.as_os_str().is_empty())
        .find_map(|dir| find_with_extensions(&dir.join(command), &candidates))
}

fn find_with_extensions(base: &Path, extensions: &[OsString]) -> Option<PathBuf> {
    extensions.iter().find_map(|ext| {
        let mut candidate = base.as_os_str().to_os_string();
        candidate.push(ext);
        let candidate = PathBuf::from(candidate);
        candidate.is_file().then_some(candidate)
    })
}

/// On Windows only extensions from PATHEXT, unless the command already carries one of them.
fn extension_candidates(command: &str, pathext: Option<&OsStr>) -> Vec<OsString> {
    if !cfg!(windows) {
        return vec![OsString::new()];
    }
    let pathext = pathext
        .and_then(OsStr::to_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(DEFAULT_PATHEXT);
    let extensions: Vec<String> = pathext
        .split(';')
        .map(str::trim)
        .filter(|e| e.starts_with('.'))
        .map(str::to_ascii_lowercase)
        .collect();
    let lower = command.to_ascii_lowercase();
    if extensions.iter().any(|ext| lower.ends_with(ext.as_str())) {
        return vec![OsString::new()];
    }
    extensions.into_iter().map(OsString::from).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("hangar-resolve-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn touch(&self, file: &str) -> PathBuf {
            let path = self.0.join(file);
            std::fs::write(&path, b"").unwrap();
            path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn path_of(dirs: &[&Path]) -> OsString {
        std::env::join_paths(dirs).unwrap()
    }

    #[test]
    fn not_found() {
        let dir = TempDir::new("missing");
        let path = path_of(&[&dir.0]);
        assert_eq!(resolve_program("does-not-exist", Some(&path), None), None);
        assert_eq!(resolve_program("x", None, None), None);
    }

    #[test]
    fn first_hit_in_path_wins() {
        let a = TempDir::new("first-a");
        let b = TempDir::new("first-b");
        let file = if cfg!(windows) { "tool.exe" } else { "tool" };
        a.touch(file);
        b.touch(file);
        let path = path_of(&[&a.0, &b.0]);
        assert_eq!(
            resolve_program("tool", Some(&path), None),
            Some(a.0.join(file))
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_prefers_pathext_over_file_without_extension() {
        let dir = TempDir::new("pathext");
        dir.touch("codex");
        let cmd = dir.touch("codex.cmd");
        let path = path_of(&[&dir.0]);
        let pathext = OsString::from(".COM;.EXE;.BAT;.CMD");
        assert_eq!(
            resolve_program("codex", Some(&path), Some(&pathext)),
            Some(cmd)
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_extension_in_command_is_respected() {
        let dir = TempDir::new("ext");
        let exe = dir.touch("claude.exe");
        dir.touch("claude.cmd");
        let path = path_of(&[&dir.0]);
        assert_eq!(resolve_program("claude.exe", Some(&path), None), Some(exe));
        // Windows file systems are case-insensitive; the extension in the command may be upper case.
        assert!(resolve_program("CLAUDE.EXE", Some(&path), None).is_some());
    }

    #[test]
    fn absolute_path_with_spaces() {
        let dir = TempDir::new("with spaces");
        let file = if cfg!(windows) {
            "my tool.exe"
        } else {
            "my tool"
        };
        let full = dir.touch(file);
        let command = dir.0.join("my tool");
        assert_eq!(
            resolve_program(command.to_str().unwrap(), None, None),
            Some(full)
        );
    }
}
