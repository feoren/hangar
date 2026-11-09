//! The Hangar team protocol (team service step C): wire types of the WebSocket protocol, keyed
//! hashes for content-free matching, and the state machines the server applies (reservations,
//! claim overlaps, event log sequencing).
//!
//! No I/O, no clock, no async runtime, no network: the app and the separate team server both
//! depend on this crate and bring those themselves. Time is passed in as [`ids::UnixMillis`].
//! Encryption is step E; this crate only carries opaque [`ids::Ciphertext`] values and the
//! [`ids::Epoch`] of the key they were sealed with.
//!
//! Design and message table: `docs/wiki/pages/concepts/team-protocol.md`.

pub mod hash;
pub mod ids;
pub mod limits;
pub mod log;
pub mod overlap;
pub mod reservation;
mod text;
pub mod wire;

/// The protocol version in every frame (`v`). A change to any message's shape, to a limit or to
/// a hash rule needs a new version.
pub const PROTOCOL_VERSION: u16 = 1;
