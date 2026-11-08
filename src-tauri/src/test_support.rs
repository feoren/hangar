//! Helpers shared by unit tests. Test files live under `<repo>/tests/.tmp/` (git-ignored), never in
//! the system temp folder.

use std::fs;
use std::path::{Path, PathBuf};

/// Folder that is created empty and removed again on drop.
pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(name: &str) -> Self {
        let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
        let root = manifest.parent().unwrap_or(manifest);
        let dir = root
            .join("tests")
            .join(".tmp")
            .join(format!("{name}-{}", std::process::id()));
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
