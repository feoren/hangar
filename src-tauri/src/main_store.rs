//! State only the main window may read and write: the security switches (local claims, the agent
//! gate, the command guard) and the queued messages per session.
//!
//! The panel and file viewer windows share the main window's origin and with it its
//! `localStorage`. A compromised viewer (a crafted Markdown file) could otherwise turn the gate
//! off for the next start, or queue a message that Hangar later pastes into an agent session.
//! Here the files sit in the local app data folder, written atomically
//! ([`crate::persist::write_atomic`]) and validated on reading and on writing; the commands
//! answer the main window only ([`check_main`]).
//!
//! `None` from a load means nothing was stored yet: the main window then migrates the values it
//! kept in `localStorage` before (once) and removes them there.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, Serializer};

use crate::persist::{normalize_key, read_limited, write_atomic};

pub const SWITCHES_FILE: &str = "security-switches.json";
pub const QUEUE_FILE: &str = "queued-messages.json";
/// Queued messages kept at most, one per session ([`crate::persist::MAX_SESSIONS`]).
pub const MAX_QUEUED: usize = crate::persist::MAX_SESSIONS;
/// Length of one queued message in UTF-16 units, as the window counts (`MAX_QUEUED_CHARS`).
pub const MAX_QUEUED_UNITS: usize = 20_000;
const MAX_SWITCHES_BYTES: usize = 4 * 1024;
/// Every message at its longest in UTF-8, with room for the keys and the JSON around them.
const MAX_QUEUE_BYTES: usize = MAX_QUEUED * (MAX_QUEUED_UNITS * 3 + 128);

#[derive(Debug, thiserror::Error)]
pub enum MainStoreError {
    #[error("Only the main window may do this")]
    WrongWindow,
    #[error("Invalid queued message: {0}")]
    Invalid(&'static str),
    #[error("File {path} cannot be read or written: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("Background task aborted: {0}")]
    Task(String),
}

/// No error text carries a queued message.
impl Serialize for MainStoreError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// Only the window labelled `main` may call the commands of this store. The capabilities grant
/// them to it alone already; this check holds even if a capability file grew by mistake.
pub fn check_main(label: &str) -> Result<(), MainStoreError> {
    if label == crate::panel_window::MAIN_LABEL {
        Ok(())
    } else {
        Err(MainStoreError::WrongWindow)
    }
}

/// The switches that keep agents in check. All on by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SecuritySwitches {
    /// Local claims; the gate needs them.
    pub claims_enabled: bool,
    pub gate_enabled: bool,
    pub command_guard_enabled: bool,
}

impl Default for SecuritySwitches {
    fn default() -> Self {
        Self {
            claims_enabled: true,
            gate_enabled: true,
            command_guard_enabled: true,
        }
    }
}

/// Queued message text per session key (a UUID, as the workspace stores it).
pub type QueuedMessages = BTreeMap<String, String>;

/// Keys as UUIDs in lower case, texts not empty and not too long, not too many.
pub fn validate_queue(entries: QueuedMessages) -> Result<QueuedMessages, MainStoreError> {
    if entries.len() > MAX_QUEUED {
        return Err(MainStoreError::Invalid("too many messages"));
    }
    let mut valid = BTreeMap::new();
    for (key, text) in entries {
        let key = normalize_key(&key).map_err(|_| MainStoreError::Invalid("session key"))?;
        if text.trim().is_empty() {
            return Err(MainStoreError::Invalid("empty text"));
        }
        if text.encode_utf16().count() > MAX_QUEUED_UNITS {
            return Err(MainStoreError::Invalid("text too long"));
        }
        valid.insert(key, text);
    }
    Ok(valid)
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> MainStoreError + '_ {
    move |source| MainStoreError::Io {
        path: path.to_owned(),
        source,
    }
}

/// `None` when the file does not exist. A file that is too large, no regular file, cannot be
/// parsed or fails the checks counts as stored, with the safe defaults: switches on, no
/// messages (`Some(None)`).
fn load<T: serde::de::DeserializeOwned>(path: &Path, max: usize) -> Option<Option<T>> {
    match read_limited(path, max) {
        Ok(Some(bytes)) => Some(serde_json::from_slice(&bytes).ok()),
        Ok(None) => None,
        Err(_) => Some(None),
    }
}

fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), MainStoreError> {
    let json = serde_json::to_vec(value)
        .map_err(|error| io_error(path)(io::Error::new(io::ErrorKind::InvalidData, error)))?;
    write_atomic(path, &json).map_err(io_error(path))
}

/// The files in the local app data folder. Cheap to clone for blocking threads.
#[derive(Debug, Clone)]
pub struct MainStore {
    dir: PathBuf,
}

impl MainStore {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// `None`: never stored. A damaged file gives the defaults (everything on).
    pub fn load_switches(&self) -> Option<SecuritySwitches> {
        let path = self.dir.join(SWITCHES_FILE);
        load(&path, MAX_SWITCHES_BYTES).map(Option::unwrap_or_default)
    }

    pub fn save_switches(&self, switches: SecuritySwitches) -> Result<(), MainStoreError> {
        save(&self.dir.join(SWITCHES_FILE), &switches)
    }

    /// `None`: never stored. A damaged file, or one that fails the checks, gives no messages.
    pub fn load_queue(&self) -> Option<QueuedMessages> {
        let path = self.dir.join(QUEUE_FILE);
        load::<QueuedMessages>(&path, MAX_QUEUE_BYTES).map(|entries| {
            entries
                .and_then(|entries| validate_queue(entries).ok())
                .unwrap_or_default()
        })
    }

    pub fn save_queue(&self, entries: QueuedMessages) -> Result<(), MainStoreError> {
        let entries = validate_queue(entries)?;
        save(&self.dir.join(QUEUE_FILE), &entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempDir;

    const KEY: &str = "0F8FAD5B-D9CB-469F-A165-70867728950E";

    #[test]
    fn only_the_main_window_may_call() {
        assert!(check_main("main").is_ok());
        for other in ["panel", "file-viewer-1", "Main", ""] {
            assert!(
                matches!(check_main(other), Err(MainStoreError::WrongWindow)),
                "{other}"
            );
        }
    }

    #[test]
    fn switches_round_trip_and_start_unset() {
        let tmp = TempDir::new("main-store-switches");
        let store = MainStore::new(tmp.path().to_owned());
        assert_eq!(store.load_switches(), None);
        let off = SecuritySwitches {
            claims_enabled: true,
            gate_enabled: false,
            command_guard_enabled: false,
        };
        store.save_switches(off).unwrap();
        assert_eq!(store.load_switches(), Some(off));
        // Damaged or foreign content: stored, but everything on.
        std::fs::write(tmp.path().join(SWITCHES_FILE), r#"{"gateEnabled":"no"}"#).unwrap();
        assert_eq!(store.load_switches(), Some(SecuritySwitches::default()));
        let json = serde_json::to_value(off).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "claimsEnabled": true, "gateEnabled": false,
                "commandGuardEnabled": false })
        );
    }

    #[test]
    fn queued_messages_are_checked_both_ways() {
        let tmp = TempDir::new("main-store-queue");
        let store = MainStore::new(tmp.path().to_owned());
        assert_eq!(store.load_queue(), None);
        let entries = QueuedMessages::from([(KEY.to_owned(), "run the tests".to_owned())]);
        store.save_queue(entries).unwrap();
        assert_eq!(
            store.load_queue(),
            Some(QueuedMessages::from([(
                KEY.to_ascii_lowercase(),
                "run the tests".to_owned()
            )]))
        );
        for (key, text) in [
            ("../escape", "x"),
            (KEY, "   "),
            (KEY, &"y".repeat(MAX_QUEUED_UNITS + 1)),
        ] {
            let bad = QueuedMessages::from([(key.to_owned(), text.to_owned())]);
            assert!(store.save_queue(bad).is_err(), "{key}");
        }
        // Exactly at the limit, counted in UTF-16 units like the window (one emoji is two).
        let emoji = "\u{1F600}".repeat(MAX_QUEUED_UNITS / 2);
        assert!(validate_queue(QueuedMessages::from([(KEY.to_owned(), emoji)])).is_ok());
        let many: QueuedMessages = (0..=MAX_QUEUED)
            .map(|i| (format!("00000000-0000-0000-0000-{i:012}"), "x".to_owned()))
            .collect();
        assert!(validate_queue(many).is_err());
        // A file someone else wrote with a bad key gives no messages at all.
        std::fs::write(tmp.path().join(QUEUE_FILE), r#"{"../x":"HANGAR-MARKER"}"#).unwrap();
        assert_eq!(store.load_queue(), Some(QueuedMessages::new()));
    }
}
