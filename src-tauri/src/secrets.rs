//! Secret profile variables in the OS keychain (contract:
//! docs/wiki/pages/syntheses/agent-workflow-features.md, "Secrets").
//!
//! A profile names its secret variables in `secret_env`; the values live only in the keychain
//! (Windows Credential Manager, macOS Keychain, Secret Service on Linux) under service `hangar`
//! and account `<profile id>/<VARIABLE>`. They are read when a session starts and go straight
//! into the child's environment. No value is ever logged, put into an error text or sent to the
//! window; errors name the variable at most.
//!
//! The store sits behind [`SecretStore`] so tests run against [`MemoryStore`] and never touch the
//! user's keychain.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::{Serialize, Serializer};

pub const SERVICE: &str = "hangar";
/// Profile ids and variable names are part of the account name; both stay short.
pub const MAX_NAME_CHARS: usize = 128;
/// Windows Credential Manager keeps at most 2560 bytes per credential and keyring stores UTF-16,
/// so 1280 UTF-16 units; checked here so the error is clear before the keychain is asked.
pub const MAX_VALUE_UNITS: usize = 1280;

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("The keychain is not available on this system")]
    Unavailable,
    #[error("The keychain refused access: {0}")]
    Denied(String),
    #[error("The keychain failed: {0}")]
    Failed(String),
    #[error("The value is too long for the keychain")]
    TooLong,
    #[error("The stored value is not text")]
    NotText,
    #[error("The value must not be empty")]
    Empty,
    #[error("Invalid variable name '{0}': letters, digits and _ only, not starting with a digit")]
    InvalidName(String),
    #[error("Invalid profile id '{0}'")]
    InvalidProfile(String),
    #[error("Background task aborted: {0}")]
    Task(String),
}

impl Serialize for SecretError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// Storage of secret values by account name.
pub trait SecretStore: Send + Sync {
    /// `Ok(None)` when nothing is stored under `account`.
    fn get(&self, account: &str) -> Result<Option<String>, SecretError>;
    fn set(&self, account: &str, value: &str) -> Result<(), SecretError>;
    /// Deleting what is not there is not an error.
    fn delete(&self, account: &str) -> Result<(), SecretError>;
}

/// `[A-Za-z_][A-Za-z0-9_]*`: a name every shell and every platform accepts as a variable, and
/// one that cannot change the meaning of the account name.
pub fn is_valid_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= MAX_NAME_CHARS
        && bytes
            .next()
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Profile ids as the account prefix: not empty, no whitespace, no control characters, no `/`.
pub fn is_valid_profile_id(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= MAX_NAME_CHARS
        && !id
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '/')
}

/// `<profile id>/<VARIABLE>`, after checking both parts.
pub fn account(profile_id: &str, name: &str) -> Result<String, SecretError> {
    if !is_valid_profile_id(profile_id) {
        return Err(SecretError::InvalidProfile(profile_id.to_owned()));
    }
    if !is_valid_name(name) {
        return Err(SecretError::InvalidName(name.to_owned()));
    }
    Ok(format!("{profile_id}/{name}"))
}

pub fn set(
    store: &dyn SecretStore,
    profile_id: &str,
    name: &str,
    value: &str,
) -> Result<(), SecretError> {
    let account = account(profile_id, name)?;
    if value.is_empty() {
        return Err(SecretError::Empty);
    }
    if value.encode_utf16().count() > MAX_VALUE_UNITS {
        return Err(SecretError::TooLong);
    }
    store.set(&account, value)
}

pub fn delete(store: &dyn SecretStore, profile_id: &str, name: &str) -> Result<(), SecretError> {
    store.delete(&account(profile_id, name)?)
}

/// Whether a value is stored, per name; the window shows "stored", never the value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecretStatus {
    pub name: String,
    pub stored: bool,
}

pub fn statuses(
    store: &dyn SecretStore,
    profile_id: &str,
    names: &[String],
) -> Result<Vec<SecretStatus>, SecretError> {
    names
        .iter()
        .map(|name| {
            let stored = store.get(&account(profile_id, name)?)?.is_some();
            Ok(SecretStatus {
                name: name.clone(),
                stored,
            })
        })
        .collect()
}

/// Values for a session start: each stored name with its value. A missing secret is skipped,
/// a failing keychain too (the reason goes to the log with the name, never the value); the
/// program then starts without it and reports the missing key itself.
pub fn for_launch(
    store: &dyn SecretStore,
    profile_id: &str,
    names: &[String],
) -> Vec<(String, String)> {
    names
        .iter()
        .filter_map(|name| {
            match account(profile_id, name).and_then(|account| store.get(&account)) {
                Ok(Some(value)) => Some((name.clone(), value)),
                Ok(None) => None,
                Err(error) => {
                    eprintln!("hangar: secret {name} of profile {profile_id} not read: {error}");
                    None
                }
            }
        })
        .collect()
}

/// Deletes every listed secret of a profile, best effort (a deleted profile or a name taken out
/// of `secret_env`). Failures go to the log with the name.
pub fn delete_all(store: &dyn SecretStore, profile_id: &str, names: &[String]) {
    for name in names {
        if let Err(error) = delete(store, profile_id, name) {
            eprintln!("hangar: secret {name} of profile {profile_id} not deleted: {error}");
        }
    }
}

/// The platform keychain through the `keyring` crate, under [`SERVICE`].
#[derive(Debug, Clone, Copy)]
pub struct KeyringStore {
    service: &'static str,
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self { service: SERVICE }
    }
}

impl KeyringStore {
    /// Another service name, so a test never touches Hangar's own entries.
    pub fn with_service(service: &'static str) -> Self {
        Self { service }
    }

    fn entry(&self, account: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(self.service, account).map_err(map_error)
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        match self.entry(account)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(map_error(error)),
        }
    }

    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        self.entry(account)?.set_password(value).map_err(map_error)
    }

    fn delete(&self, account: &str) -> Result<(), SecretError> {
        match self.entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(map_error(error)),
        }
    }
}

/// Own texts per kind: some keyring errors carry the raw stored bytes (`BadEncoding`,
/// `BadDataFormat`) or debug output of entries (`Ambiguous`), which must not reach a log or the
/// window. Platform errors are the operating system's own message.
fn map_error(error: keyring::Error) -> SecretError {
    match error {
        keyring::Error::NoDefaultStore => SecretError::Unavailable,
        keyring::Error::NoStorageAccess(platform) => SecretError::Denied(platform.to_string()),
        keyring::Error::PlatformFailure(platform) => SecretError::Failed(platform.to_string()),
        keyring::Error::TooLong(..) => SecretError::TooLong,
        keyring::Error::BadEncoding(_) | keyring::Error::BadDataFormat(..) => SecretError::NotText,
        keyring::Error::Ambiguous(_) => {
            SecretError::Failed("more than one keychain entry matches".to_owned())
        }
        keyring::Error::NoEntry => SecretError::Failed("no keychain entry".to_owned()),
        keyring::Error::Invalid(attribute, _) => {
            SecretError::Failed(format!("invalid keychain attribute '{attribute}'"))
        }
        keyring::Error::BadStoreFormat(_) | keyring::Error::NotSupportedByStore(_) => {
            SecretError::Failed("the keychain does not support this".to_owned())
        }
        _ => SecretError::Failed("unknown keychain error".to_owned()),
    }
}

/// In-memory store for tests (unit and integration); never used by the app.
#[derive(Debug, Default)]
pub struct MemoryStore {
    values: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemoryStore {
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        Ok(crate::lock(&self.values).get(account).cloned())
    }

    fn set(&self, account: &str, value: &str) -> Result<(), SecretError> {
        crate::lock(&self.values).insert(account.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, account: &str) -> Result<(), SecretError> {
        crate::lock(&self.values).remove(account);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "sk-test-value-123";

    #[test]
    fn names_and_ids() {
        for good in ["ANTHROPIC_API_KEY", "_x", "a1", "OPENAI_API_KEY"] {
            assert!(is_valid_name(good), "{good}");
        }
        let long = "A".repeat(MAX_NAME_CHARS + 1);
        for bad in [
            "",
            "1A",
            "A-B",
            "A B",
            "A=B",
            "A/B",
            "Ä",
            "A\0",
            long.as_str(),
        ] {
            assert!(!is_valid_name(bad), "{bad:?}");
        }
        for good in ["claude", "wsl-ubuntu-24.04", "my_agent"] {
            assert!(is_valid_profile_id(good), "{good}");
        }
        for bad in ["", "a b", "a/b", "a\nb", "a\tb"] {
            assert!(!is_valid_profile_id(bad), "{bad:?}");
        }
        assert_eq!(account("claude", "KEY").unwrap(), "claude/KEY");
        assert!(matches!(
            account("a/b", "KEY"),
            Err(SecretError::InvalidProfile(_))
        ));
        assert!(matches!(
            account("a", "K/../X"),
            Err(SecretError::InvalidName(_))
        ));
    }

    #[test]
    fn set_read_delete_round_trip() {
        let store = MemoryStore::default();
        let names = vec!["API_KEY".to_owned(), "OTHER".to_owned()];
        assert_eq!(
            statuses(&store, "p", &names).unwrap(),
            [
                SecretStatus {
                    name: "API_KEY".into(),
                    stored: false
                },
                SecretStatus {
                    name: "OTHER".into(),
                    stored: false
                }
            ]
        );
        set(&store, "p", "API_KEY", SECRET).unwrap();
        assert!(statuses(&store, "p", &names).unwrap()[0].stored);
        assert_eq!(
            for_launch(&store, "p", &names),
            [("API_KEY".to_owned(), SECRET.to_owned())]
        );
        // Another profile does not see it.
        assert!(for_launch(&store, "q", &names).is_empty());
        delete(&store, "p", "API_KEY").unwrap();
        delete(&store, "p", "API_KEY").unwrap();
        assert!(for_launch(&store, "p", &names).is_empty());

        set(&store, "p", "API_KEY", SECRET).unwrap();
        set(&store, "p", "OTHER", SECRET).unwrap();
        delete_all(&store, "p", &names);
        assert!(for_launch(&store, "p", &names).is_empty());
    }

    /// The real keychain, under a service of its own; by hand:
    /// `cargo test --lib -- --ignored keychain_round_trip`.
    #[test]
    #[ignore = "touches the OS keychain"]
    fn keychain_round_trip() {
        let store = KeyringStore::with_service("hangar-tests");
        let account = format!("test-{}/HANGAR_TEST_SECRET", std::process::id());
        store.delete(&account).unwrap();
        assert_eq!(store.get(&account).unwrap(), None);
        store.set(&account, "äöü value 123").unwrap();
        assert_eq!(
            store.get(&account).unwrap().as_deref(),
            Some("äöü value 123")
        );
        store.set(&account, "second").unwrap();
        assert_eq!(store.get(&account).unwrap().as_deref(), Some("second"));
        store.delete(&account).unwrap();
        store.delete(&account).unwrap();
        assert_eq!(store.get(&account).unwrap(), None);
        // The longest value Hangar accepts fits.
        let long = "x".repeat(MAX_VALUE_UNITS);
        store.set(&account, &long).unwrap();
        assert_eq!(store.get(&account).unwrap().as_deref(), Some(long.as_str()));
        store.delete(&account).unwrap();
    }

    #[test]
    fn values_are_checked_and_never_in_errors() {
        let store = MemoryStore::default();
        assert!(matches!(set(&store, "p", "K", ""), Err(SecretError::Empty)));
        let long = format!("{SECRET}{}", "x".repeat(MAX_VALUE_UNITS));
        let error = set(&store, "p", "K", &long).unwrap_err();
        assert!(matches!(error, SecretError::TooLong));
        let error = set(&store, "p", "BAD NAME", SECRET).unwrap_err();
        for text in [error.to_string(), serde_json::to_string(&error).unwrap()] {
            assert!(!text.contains(SECRET), "{text}");
        }
        // Raw bytes inside keyring errors do not come through.
        let mapped = map_error(keyring::Error::BadEncoding(SECRET.as_bytes().to_vec()));
        assert!(!mapped.to_string().contains(SECRET));
        let mapped = map_error(keyring::Error::BadDataFormat(
            SECRET.as_bytes().to_vec(),
            Box::new(std::io::Error::other("x")),
        ));
        assert!(!format!("{mapped} {mapped:?}").contains(SECRET));
    }
}
