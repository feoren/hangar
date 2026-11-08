//! Cache for values derived from a program file (version, icon).
//!
//! The key is the path plus modification time and size: a program update changes
//! at least one of them, and the value is determined again without restarting Hangar.

use std::collections::HashMap;
use std::hash::Hash;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use crate::lock;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileStamp {
    pub path: PathBuf,
    pub modified: Option<SystemTime>,
    pub len: u64,
}

impl FileStamp {
    /// `None` if the file is not (or no longer) readable; nothing is cached then.
    pub fn of(path: &Path) -> Option<Self> {
        let meta = std::fs::metadata(path).ok()?;
        Some(Self {
            path: path.to_path_buf(),
            modified: meta.modified().ok(),
            len: meta.len(),
        })
    }
}

/// "No result" is cached too, so a hanging program does not run for five seconds again
/// every time the menu opens.
pub struct FileCache<K, V> {
    entries: Mutex<HashMap<K, V>>,
}

impl<K, V> Default for FileCache<K, V> {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }
}

impl<K: Eq + Hash, V: Clone> FileCache<K, V> {
    pub fn get(&self, key: &K) -> Option<V> {
        lock(&self.entries).get(key).cloned()
    }

    pub fn insert(&self, key: K, value: V) {
        lock(&self.entries).insert(key, value);
    }

    /// Forgets everything. For cases the file stamp cannot see: an npm update swaps the
    /// package but not the launcher script `gemini.cmd`, whose path, time and size stay the same.
    pub fn clear(&self) {
        lock(&self.entries).clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamp_changes_with_content() {
        let path = std::env::temp_dir().join(format!("hangar-stamp-{}", std::process::id()));
        std::fs::write(&path, b"a").unwrap();
        let first = FileStamp::of(&path).unwrap();
        std::fs::write(&path, b"ab").unwrap();
        let second = FileStamp::of(&path).unwrap();
        assert_ne!(first, second);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(FileStamp::of(&path), None);
    }

    #[test]
    fn cache_also_stores_no_result() {
        let cache: FileCache<u8, Option<String>> = FileCache::default();
        assert_eq!(cache.get(&1), None);
        cache.insert(1, None);
        assert_eq!(cache.get(&1), Some(None));
        cache.insert(2, Some("1.2".into()));
        assert_eq!(cache.get(&2), Some(Some("1.2".into())));
        cache.clear();
        assert_eq!(cache.get(&1), None);
        assert_eq!(cache.get(&2), None);
    }
}
