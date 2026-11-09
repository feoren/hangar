//! Size limits of the protocol. Both sides enforce them: the server rejects a frame over a limit
//! with `err { code: "limit" }`, the client never builds one. Raising a limit is a protocol
//! change and needs a new [`crate::PROTOCOL_VERSION`].

/// Largest frame, in bytes of UTF-8 JSON, checked before anything is parsed.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;
/// Longest identifier the client chooses (op id, session key).
pub const MAX_ID_LEN: usize = 64;
/// Longest identifier assigned outside the client (team = the website's project id, member =
/// the website's user id, device, blob, stream). Opaque: never parsed, only compared.
pub const MAX_OPAQUE_ID_LEN: usize = 128;
/// Longest sign-in ticket in `hello` (the website's compact Ed25519 ticket,
/// `base64url(payload).base64url(signature)`, passed through unchanged).
pub const MAX_TICKET_LEN: usize = 2048;
/// Longest ciphertext, in characters of its base64url text (about 128 KiB of bytes): `blob.put`,
/// `blob.get`'s answer, and the claim detail (`claim.set` and `claim.overlap`), which lists the
/// base commit, paths and line ranges of up to [`MAX_CLAIM_FILES`] files.
pub const MAX_CIPHERTEXT_LEN: usize = 175_000;
/// Longest ciphertext in the other messages: presence status, notes, notify content and the
/// wrapped key of `key.rotated`.
pub const MAX_INLINE_CIPHERTEXT_LEN: usize = 16 * 1024;
/// Files in one `claim.set`; a session with more sends the first ones and sets `truncated`.
/// The largest `claim.set` stays well below [`MAX_FRAME_BYTES`] (a test checks it).
pub const MAX_CLAIM_FILES: usize = 1000;
/// Longest normalized path, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;
/// Longest single path segment, in bytes (the common file system limit).
pub const MAX_SEGMENT_BYTES: usize = 255;
/// Longest normalized task id, in bytes.
pub const MAX_TASK_ID_BYTES: usize = 256;
/// Tasks named in one `heartbeat`.
pub const MAX_HEARTBEAT_TASKS: usize = 64;
/// Hashes of the same task under earlier epochs in one `task.reserve` (`prev`).
pub const MAX_PREV_HASHES: usize = 4;
/// Reservations one member may hold in a team at once.
pub const MAX_RESERVATIONS_PER_MEMBER: usize = 32;
