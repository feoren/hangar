//! Helpers shared by integration tests. Test files live under `<repo>/tests/.tmp/` (git-ignored),
//! never in the system temp folder; the unit-test twin is `src/test_support.rs`.

// Each test crate compiles this module on its own and uses a different part of it, some parts
// only on one platform.
#![allow(dead_code)]

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

/// A shared folder for tests that need an existing home or working folder but write nothing into
/// it. Never removed, so parallel tests can rely on it; it stays empty.
pub fn home() -> PathBuf {
    let dir = tmp_root().join("home");
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Folder that is created empty and removed again on drop. Process id and counter keep parallel
/// tests and parallel runs apart; names stay short for Windows path limits in deep worktrees.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        let dir = tmp_root().join(format!(
            "{name}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
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
