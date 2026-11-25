//! The Hangar team protocol (team service step C): wire types of the WebSocket protocol, keyed
//! hashes for content-free matching, and the state machines the server applies (reservations,
//! claim overlaps, event log sequencing).
//!
//! No I/O, no clock, no async runtime, no network: the app and the separate team server both
//! depend on this crate and bring those themselves. Time is passed in as [`ids::UnixMillis`].
//! Encryption is step E; this crate only carries opaque [`ids::Ciphertext`] values and the
//! [`ids::Epoch`] of the key they were sealed with, plus the checks the relay makes without a
//! key ([`e2e`]: device public keys, the membership log's hash chain, key rotation rules).
//!
//! Design and message table: `docs/wiki/pages/concepts/team-protocol.md`.

pub mod close;
pub mod e2e;
pub mod hash;
pub mod ids;
pub mod limits;
pub mod log;
pub mod overlap;
pub mod reservation;
mod text;
pub mod wire;

/// The envelope version in every frame (`v`). It stays 1 for every protocol version of this
/// generation: a peer refuses a frame of another envelope version before it reads anything else,
/// so raising it would cut every older peer off. Which messages a connection may carry is
/// negotiated per team with [`PROTOCOL_VERSION`] instead (`hello.proto`, `welcome.proto`).
pub const ENVELOPE_VERSION: u16 = 1;

/// The newest protocol version this crate speaks: the set of message kinds, limits and hash
/// rules. A change to any message's shape, to a limit or to a hash rule needs a new version.
/// Version 2 adds the notes key ring (`notes.keys.get`, `notes.keys.set`, `notes.keys`,
/// `notes.keys.updated`). Version 3 adds the proof of possession (the `welcome`'s `challenge`,
/// answered with `device.prove`) and `blob.delete`. A kind is sent only on a team whose
/// negotiated version has it ([`wire::ClientMessage::since`], [`wire::ServerMessage::since`]),
/// and the `welcome` carries its challenge exactly when the negotiated version is 3 or newer.
pub const PROTOCOL_VERSION: u16 = 3;

/// The first protocol version with the proof of possession at `hello` (`welcome.challenge`,
/// `device.prove`) and `blob.delete`.
pub const PROOF_PROTOCOL_VERSION: u16 = 3;

/// The oldest protocol version this crate still speaks.
pub const MIN_PROTOCOL_VERSION: u16 = 1;
