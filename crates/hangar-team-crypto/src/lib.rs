//! End-to-end cryptography of Hangar teams (team service step E), on top of the wire types and
//! checks of `hangar-team-protocol`.
//!
//! - [`identity`]: a device's Ed25519 and X25519 keys, their keychain format, proofs of
//!   possession and endorsements, safety numbers.
//! - [`memberlog`]: the signed, hash-chained membership log and its verification into a
//!   [`memberlog::TeamState`] (members, roles, devices with their key history, epochs), on the
//!   clients ([`memberlog::LogVerifier::apply`]) and on the relay before it stores an entry
//!   ([`memberlog::LogVerifier::admit`], [`memberlog::LogVerifier::admit_rotation`],
//!   [`memberlog::LogVerifier::admit_device_remove`]).
//! - [`relay`]: the relay's other checks that need the verified log (`key.wrap`, device keys
//!   registered over the holder's own connection).
//! - [`epoch`]: the random secret per epoch, its commitment in the log, HPKE wrapping to devices
//!   and the history link to the previous epoch.
//! - [`rotation`]: the message of a new epoch (`key.rotate` with its signed log entry) and of
//!   `key.wrap` for one newly approved device.
//! - [`record`]: sealing and opening `Sealed` content (XChaCha20-Poly1305 plus the sender's
//!   signature, log head and counter), checked against the verified log.
//! - [`recovery`]: the printable recovery code and the offline device it derives.
//!
//! Pure: no I/O, no clock, no keychain, no network, and no secrets needed to verify the log, so
//! the relay uses the same verifier as the clients. Randomness comes in through [`Entropy`]
//! ([`OsEntropy`] in the app). Secrets are wiped on drop and never shown by `Debug`.
//!
//! This is a hand-built protocol from standard parts. It needs an external cryptographic review
//! before it protects real data. Design and threat model: `docs/wiki/pages/concepts/team-crypto.md`.

#![warn(missing_docs)]

mod encoding;
pub mod entropy;
pub mod epoch;
pub mod identity;
pub mod memberlog;
pub mod record;
pub mod recovery;
pub mod relay;
pub mod rotation;

#[cfg(test)]
mod tests;

pub use entropy::{Entropy, EntropyError, OsEntropy};
