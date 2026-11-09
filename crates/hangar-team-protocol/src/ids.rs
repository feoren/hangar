//! Identifiers, numbers and opaque encrypted fields of the protocol. Every string type checks
//! its content when it is built or parsed, so a value that exists is valid.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::limits::{MAX_CIPHERTEXT_LEN, MAX_ID_LEN, MAX_OPAQUE_ID_LEN, MAX_TICKET_LEN};

/// Why a string is not a valid protocol value.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum InvalidValue {
    #[error("{kind} is empty")]
    Empty { kind: &'static str },
    #[error("{kind} is longer than {max} characters")]
    TooLong { kind: &'static str, max: usize },
    #[error("{kind} contains a character outside its alphabet")]
    Alphabet { kind: &'static str },
}

/// `[A-Za-z0-9_-]`, the alphabet of identifiers the client makes (UUIDs) and of base64url.
fn id_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

/// Identifiers from elsewhere (the website, the relay) are opaque: any character but control,
/// format and line or paragraph separator characters, which could forge or reorder lines in a
/// log or make two ids look alike.
fn opaque_char(c: char) -> bool {
    !crate::text::is_hidden(c)
}

fn check(
    kind: &'static str,
    value: &str,
    max: usize,
    allowed: fn(char) -> bool,
) -> Result<(), InvalidValue> {
    if value.is_empty() {
        return Err(InvalidValue::Empty { kind });
    }
    if value.len() > max {
        return Err(InvalidValue::TooLong { kind, max });
    }
    if !value.chars().all(allowed) {
        return Err(InvalidValue::Alphabet { kind });
    }
    Ok(())
}

macro_rules! string_id {
    ($(#[$doc:meta])* $name:ident, $kind:literal, $max:expr, $allowed:expr) => {
        string_id!(@type $(#[$doc])* $name, $kind, $max, $allowed);

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}({:?})", stringify!($name), self.0)
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
    (secret $(#[$doc:meta])* $name:ident, $kind:literal, $max:expr, $allowed:expr) => {
        string_id!(@type $(#[$doc])* $name, $kind, $max, $allowed);

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}(..)", stringify!($name))
            }
        }
    };
    (@type $(#[$doc:meta])* $name:ident, $kind:literal, $max:expr, $allowed:expr) => {
        $(#[$doc])*
        #[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            /// Checks `value` against the identifier's alphabet and length.
            pub fn new(value: impl Into<String>) -> Result<Self, InvalidValue> {
                let value = value.into();
                check($kind, &value, $max, $allowed)?;
                Ok(Self(value))
            }

            /// The identifier as text.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<String> for $name {
            type Error = InvalidValue;
            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> String {
                value.0
            }
        }
    };
}

string_id!(
    /// Chosen by the client per intent (a UUID); a retry carries the same one, so the server
    /// applies it once.
    OpId, "op id", MAX_ID_LEN, id_char
);
string_id!(
    /// A team: the Hangar website's project id (projects are teams), opaque.
    TeamId, "team id", MAX_OPAQUE_ID_LEN, opaque_char
);
string_id!(
    /// A person: the Hangar website's user id (the ticket's `sub`), opaque.
    MemberId, "member id", MAX_OPAQUE_ID_LEN, opaque_char
);
string_id!(
    /// One installation of Hangar of a member, assigned when the device joins; opaque.
    DeviceId, "device id", MAX_OPAQUE_ID_LEN, opaque_char
);
string_id!(
    /// One Hangar session of a device, chosen by the client at random; never the local session
    /// number, which would tell the server how many sessions were started.
    SessionKey, "session key", MAX_ID_LEN, id_char
);
string_id!(
    /// A stored encrypted blob, assigned by the server; opaque.
    BlobId, "blob id", MAX_OPAQUE_ID_LEN, opaque_char
);
string_id!(
    /// One incarnation of a team's event log, assigned by the server; opaque. A server restored
    /// from a backup starts a new one, so a client never trusts sequence numbers of another
    /// stream.
    StreamId, "stream id", MAX_OPAQUE_ID_LEN, opaque_char
);
string_id!(
    secret
    /// The sign-in proof in `hello` and `ticket.renew`: the short-lived Ed25519 ticket the
    /// Hangar website issues (`base64url(payload).base64url(signature)`), carried unchanged.
    /// The relay verifies it, including its audience: `aud` is required and must name the
    /// relay (`"hangar-relay"` for the official service, the relay's `PUBLIC_URL` when self
    /// hosted), so a ticket for one relay is useless at another. This crate only checks it is
    /// printable ASCII without spaces. Never logged.
    Ticket, "ticket", MAX_TICKET_LEN, |c: char| c.is_ascii_graphic()
);

/// Position in a team's event log. The first event has sequence number 1.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct Seq(pub u64);

impl Seq {
    /// The following sequence number.
    pub fn next(self) -> Seq {
        Seq(self.0.saturating_add(1))
    }
}

/// Unix time in milliseconds, as the server's clock gives it. State machines take it as an
/// argument; nothing in this crate reads a clock.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct UnixMillis(pub u64);

impl UnixMillis {
    /// `millis` later, saturating.
    pub fn plus(self, millis: u64) -> UnixMillis {
        UnixMillis(self.0.saturating_add(millis))
    }

    /// `millis` earlier, saturating at 0.
    pub fn minus(self, millis: u64) -> UnixMillis {
        UnixMillis(self.0.saturating_sub(millis))
    }
}

/// Generation of the team key (step E). Every leave or device removal starts a new one; keyed
/// hashes and ciphertexts of different epochs never match or decrypt.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[serde(transparent)]
pub struct Epoch(pub u32);

/// Encrypted bytes as base64url without padding. The protocol never looks inside; step E
/// defines the format (nonce, sealed data, signature).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Ciphertext(String);

impl Ciphertext {
    /// Checks that `value` is base64url text within [`MAX_CIPHERTEXT_LEN`].
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidValue> {
        let value = value.into();
        check("ciphertext", &value, MAX_CIPHERTEXT_LEN, id_char)?;
        Ok(Self(value))
    }

    /// The base64url text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Ciphertext {
    type Error = InvalidValue;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<Ciphertext> for String {
    fn from(value: Ciphertext) -> String {
        value.0
    }
}

/// Content only the team can read: the ciphertext and the epoch of the key it was sealed with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sealed {
    pub epoch: Epoch,
    pub data: Ciphertext,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_check_alphabet_and_length() {
        assert!(OpId::new("0b7f6c1e-1f2a-4c55-9d1e-3a5b8c2d4e6f").is_ok());
        assert_eq!(OpId::new(""), Err(InvalidValue::Empty { kind: "op id" }));
        assert_eq!(
            TeamId::new("a".repeat(MAX_OPAQUE_ID_LEN + 1)),
            Err(InvalidValue::TooLong {
                kind: "team id",
                max: MAX_OPAQUE_ID_LEN
            })
        );
        for bad in ["a b", "a/b", "a\u{0}", "ä", "a.b"] {
            assert!(OpId::new(bad).is_err(), "{bad:?}");
            assert!(SessionKey::new(bad).is_err(), "{bad:?}");
        }
        // Ids from the website are opaque: taken as they are, only control characters refused.
        for good in ["prj_01J9", "user@example.test", "a b/c.d", "ä"] {
            assert!(TeamId::new(good).is_ok(), "{good:?}");
            assert!(MemberId::new(good).is_ok(), "{good:?}");
        }
        for bad in [
            "a\nb",
            "a\u{1b}[31m",
            "a\u{0}",
            "a\u{202e}b",
            "a\u{200b}",
            "a\u{2028}b",
            "a\u{2029}b",
        ] {
            assert!(DeviceId::new(bad).is_err(), "{bad:?}");
        }
        assert!(Ticket::new("v1.abc+/=").is_ok());
        assert!(Ticket::new("with space").is_err());
        // A ticket is a credential: it never shows in debug output or logs.
        let ticket = Ticket::new("secret-marker").unwrap();
        assert_eq!(format!("{ticket:?}"), "Ticket(..)");
    }

    #[test]
    fn ids_reject_invalid_json() {
        assert!(serde_json::from_str::<MemberId>("\"ok_1\"").is_ok());
        assert!(serde_json::from_str::<MemberId>("\"line\nbreak\"").is_err());
        assert!(serde_json::from_str::<MemberId>("12").is_err());
    }

    #[test]
    fn ciphertext_is_base64url_text() {
        assert!(Ciphertext::new("AAEC_-z9").is_ok());
        assert!(Ciphertext::new("AA==").is_err());
        assert!(Ciphertext::new("a".repeat(MAX_CIPHERTEXT_LEN + 1)).is_err());
        let sealed: Result<Sealed, _> =
            serde_json::from_str(r#"{"epoch":3,"data":"AAEC","extra":1}"#);
        assert!(sealed.is_err());
    }

    #[test]
    fn numbers_saturate() {
        assert_eq!(Seq(u64::MAX).next(), Seq(u64::MAX));
        assert_eq!(UnixMillis(5).minus(10), UnixMillis(0));
        assert_eq!(UnixMillis(u64::MAX).plus(1), UnixMillis(u64::MAX));
    }
}
