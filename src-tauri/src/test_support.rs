//! Helpers shared by unit tests. Test files live under `<repo>/tests/.tmp/` (git-ignored), never in
//! the system temp folder.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// `<repo>/tests/.tmp`, the one place test files go.
pub fn tmp_root() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .unwrap_or(manifest)
        .join("tests")
        .join(".tmp")
}

/// Creates a fresh, empty folder under [`tmp_root`] and returns its path; the caller removes it.
/// Process id and counter keep parallel tests and parallel runs (several worktrees, a second
/// `cargo test`) apart even when two tests pick the same name. Names stay short, because worktrees
/// already sit deep in the tree and Windows tools still trip over paths beyond 260 characters.
pub fn new_dir(name: &str) -> PathBuf {
    let dir = tmp_root().join(format!(
        "{name}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Folder that is created empty and removed again on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        Self(new_dir(name))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Source folders the guard reads, relative to `src-tauri`.
    const SCANNED: [&str; 4] = ["src", "tests", "examples", "../crates"];

    /// Calls that put files into the system temp folder. Assembled with `concat!` so that this
    /// file does not match its own patterns.
    const FORBIDDEN: [&str; 4] = [
        concat!("env::", "temp_dir"),
        concat!("tempfile::", "tempdir()"),
        concat!("tempfile::", "tempfile()"),
        concat!("NamedTempFile::", "new()"),
    ];

    /// A comment line may name the call (to warn against it); only code counts.
    fn reaches_system_temp(line: &str) -> bool {
        !line.trim_start().starts_with("//")
            && FORBIDDEN.iter().any(|pattern| line.contains(pattern))
    }

    fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            if path.is_dir() {
                // Build output and test files are not source.
                if name != "target" && name != ".tmp" {
                    rust_files(&path, found);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path);
            }
        }
    }

    /// Guard: no code in the backend reaches for the system temp folder. Runtime code uses none
    /// today; tests take [`TempDir`] or [`new_dir`] (integration tests: `tests/common/mod.rs`).
    /// Should runtime code ever need the system temp folder on purpose, add it here by file.
    #[test]
    fn nothing_writes_to_the_system_temp_folder() {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut files = Vec::new();
        for dir in SCANNED {
            rust_files(&manifest.join(dir), &mut files);
        }
        assert!(
            files.len() > 100,
            "the scan found too few files: {}",
            files.len()
        );
        let mut hits = Vec::new();
        for file in &files {
            // Lossy: a few sources carry raw control bytes in test strings.
            let bytes = fs::read(file).unwrap();
            let text = String::from_utf8_lossy(&bytes);
            for (number, line) in text.lines().enumerate() {
                if reaches_system_temp(line) {
                    hits.push(format!(
                        "{}:{}: {}",
                        file.display(),
                        number + 1,
                        line.trim()
                    ));
                }
            }
        }
        assert!(
            hits.is_empty(),
            "test files belong under tests/.tmp (test_support::TempDir), not in the system temp \
             folder:\n{}",
            hits.join("\n")
        );
    }

    #[test]
    fn the_guard_flags_code_but_not_comments() {
        let call = concat!("std::env::", "temp_dir()");
        assert!(reaches_system_temp(&format!("    let dir = {call};")));
        assert!(reaches_system_temp(&format!("let dir = {call}; // why")));
        assert!(!reaches_system_temp(&format!("    // never {call}")));
        assert!(!reaches_system_temp(&format!("/// never {call}")));
        assert!(!reaches_system_temp("let dir = env.temp_dir();"));
    }

    #[test]
    fn folders_with_the_same_name_do_not_collide() {
        let a = TempDir::new("same");
        let b = TempDir::new("same");
        assert_ne!(a.path(), b.path());
        assert!(a.path().is_dir() && b.path().is_dir());
        assert!(a.path().starts_with(tmp_root()));
        let path = a.path().to_path_buf();
        drop(a);
        assert!(!path.exists());
    }
}
