//! Which note files this computer knew while their key was still the team's newest (ADR 0006,
//! "Old keys after a removal").
//!
//! After a removal the team gets a new notes key, and Hangar re-encrypts the notes under it. A
//! removed member still holds the old keys, though, and may still be able to push to the
//! repository: re-encrypting whatever lies in the folder would turn a note planted with an old
//! key into one that looks like any other. So only files whose exact bytes this computer saw
//! earlier are re-encrypted: files it wrote itself, files of a solo vault (only its owner holds
//! that key), and files under the newest key that a scan met while the team's verified log knew
//! of no removal past that key. Everything else under an old key stays as it is and is marked.
//!
//! The record lives in the app data folder beside the vault's local trash
//! (`notes/shared/<vault id>/seen`): one SHA-256 of a whole note file per line, oldest first, at
//! most [`MAX_ENTRIES`]. The file holds no plaintext, no key and no title. Losing it costs only
//! re-encryptions: the notes it would have covered stay readable and marked.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

use hangar_team_crypto::notes::VaultId;
use sha2::{Digest as _, Sha256};

use crate::persist::{create_private_dir, read_regular_capped, write_atomic};

pub(super) type Digest = [u8; 32];

const FILE_NAME: &str = "seen";
const HEADER: &str = "hangar-seen 1";
/// Enough for every note of a full vault several times over (old and new versions).
pub(super) const MAX_ENTRIES: usize = 4 * super::MAX_NOTES;
/// 64 hex digits and a line feed per entry, plus the header.
const MAX_FILE_BYTES: usize = MAX_ENTRIES * 65 + 64;

pub(super) fn digest(bytes: &[u8]) -> Digest {
    Sha256::digest(bytes).into()
}

fn to_hex(digest: &Digest) -> String {
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn from_hex(text: &str) -> Option<Digest> {
    if text.len() != 64 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; 32];
    for (index, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(text.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

#[derive(Default)]
struct Entries {
    order: VecDeque<Digest>,
    set: HashSet<Digest>,
}

impl Entries {
    fn parse(text: &str) -> Self {
        let mut entries = Self::default();
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return entries;
        }
        for digest in lines.filter_map(from_hex) {
            entries.push(digest);
        }
        entries
    }

    /// `true` when the digest is new.
    fn push(&mut self, digest: Digest) -> bool {
        if !self.set.insert(digest) {
            return false;
        }
        self.order.push_back(digest);
        while self.order.len() > MAX_ENTRIES {
            if let Some(oldest) = self.order.pop_front() {
                self.set.remove(&oldest);
            }
        }
        true
    }

    fn text(&self) -> String {
        let mut out = String::with_capacity(HEADER.len() + 1 + self.order.len() * 65);
        out.push_str(HEADER);
        out.push('\n');
        for digest in &self.order {
            out.push_str(&to_hex(digest));
            out.push('\n');
        }
        out
    }
}

/// The record of every vault this run touched, loaded on first use.
pub(super) struct Seen {
    root: PathBuf,
    vaults: HashMap<VaultId, Entries>,
}

impl Seen {
    /// `root` is the folder of the vaults' local trash (`notes/shared` in the app data).
    pub(super) fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            vaults: HashMap::new(),
        }
    }

    fn path(&self, vault: &VaultId) -> PathBuf {
        self.root.join(vault.to_hex()).join(FILE_NAME)
    }

    fn entries(&mut self, vault: &VaultId) -> &mut Entries {
        let path = self.path(vault);
        self.vaults.entry(*vault).or_insert_with(|| {
            match read_regular_capped(&path, MAX_FILE_BYTES) {
                Ok(Some(bytes)) => Entries::parse(&String::from_utf8_lossy(&bytes)),
                Ok(None) => {
                    eprintln!(
                        "hangar: the record of seen shared notes is too large; starting anew"
                    );
                    Entries::default()
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Entries::default(),
                Err(error) => {
                    eprintln!("hangar: the record of seen shared notes was not read: {error}");
                    Entries::default()
                }
            }
        })
    }

    pub(super) fn contains(&mut self, vault: &VaultId, digest: &Digest) -> bool {
        self.entries(vault).set.contains(digest)
    }

    /// Adds `digests` and writes the record when one of them was new. A failed write is logged:
    /// the digests stay known for this run, and the next run re-encrypts less, never more.
    pub(super) fn add(&mut self, vault: &VaultId, digests: impl IntoIterator<Item = Digest>) {
        let entries = self.entries(vault);
        let mut changed = false;
        for digest in digests {
            changed |= entries.push(digest);
        }
        if !changed {
            return;
        }
        let text = entries.text();
        let path = self.path(vault);
        let written = path
            .parent()
            .map_or(Ok(()), create_private_dir)
            .and_then(|()| write_atomic(&path, text.as_bytes()));
        if let Err(error) = written {
            eprintln!("hangar: the record of seen shared notes was not written: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    #[test]
    fn the_record_round_trips_and_keeps_only_the_newest_entries() {
        let dir = TempDir::new("shared-notes-seen");
        let vault = VaultId::from_hex("00112233445566778899aabbccddeeff").unwrap();
        let mut seen = Seen::new(dir.path());
        let first = digest(b"one");
        seen.add(&vault, [first, digest(b"two")]);
        let mut again = Seen::new(dir.path());
        assert!(again.contains(&vault, &first));
        assert!(!again.contains(&vault, &digest(b"three")));

        let mut entries = Entries::default();
        for index in 0..=MAX_ENTRIES {
            entries.push(digest(&index.to_be_bytes()));
        }
        assert_eq!(entries.order.len(), MAX_ENTRIES);
        assert!(!entries.set.contains(&digest(&0usize.to_be_bytes())));
        let parsed = Entries::parse(&entries.text());
        assert_eq!(parsed.order, entries.order);
        // A damaged or foreign file counts as empty.
        assert!(Entries::parse("hangar-seen 2\n").order.is_empty());
        assert_eq!(Entries::parse(&format!("{HEADER}\nxyz\n")).order.len(), 0);
    }
}
