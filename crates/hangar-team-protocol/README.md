# hangar-team-protocol

Wire types, keyed hashes and state machines of the Hangar team protocol. Shared by the Hangar app
and the team relay; MIT licensed like the app.

The crate does no I/O: no network, no files, no clock, no async runtime. Callers pass the time in
(`UnixMillis`) and bring their own transport. Encryption is not part of it yet; encrypted content
travels as opaque `Sealed` values with the `Epoch` of the key that sealed it, and `EpochKeys`
derives that epoch's separate matching and sealing keys.

## Modules

| Module        | Content                                                                                                                                                              |
| ------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `wire`        | Client and server messages, the JSON envelope, `encode_*` and `decode_*` with size, field and version checks, epoch checks (`check_epoch`), body digests for retries |
| `hash`        | `EpochKeys` (HKDF-SHA256), `MatchKey` (HMAC-SHA256), normalization of task ids, repository ids and paths (NFC, case rule), `PathHashes`                              |
| `reservation` | `Reservation` state machine and `ReservationBook`, the server's "first wins" authority; time events come apart from the outcome (`Applied`)                          |
| `overlap`     | `ClaimBoard`: same area and same file between sessions of different holders, by hash equality; passes the encrypted detail on                                        |
| `log`         | `EventLog` (sequence numbers, replay window, resync), `ClientCursor`, `RecentOps` (op id retries per sender and team)                                                |
| `e2e`         | What the relay checks for step E without a key: `PublicKey` (device keys), `LogHash`, `MemberChain` and `check_append` (membership log chain), `check_rotate`        |
| `close`       | WebSocket close codes of the relay and the `hello` deadline                                                                                                          |
| `ids`         | Identifiers, `Seq`, `UnixMillis`, `Epoch`, `Ciphertext`, `Sealed`                                                                                                    |
| `limits`      | Size limits both sides enforce                                                                                                                                       |

Identifiers that come from elsewhere (team = the website's project id, member = the website's user
id, device, blob, stream) are opaque strings: any characters but control, format and line or
paragraph separator characters, up to 128 bytes. The `hello` ticket is the website's compact
Ed25519 ticket, carried unchanged; verifying it, and binding the device to the ticket's member,
is the relay's job.

The full message table, the design notes and the threat model are in the repository wiki,
`docs/wiki/pages/concepts/team-protocol.md`.

## Using it

As a path dependency (the crate is not published):

```toml
[dependencies]
hangar-team-protocol = { path = "../hangar/crates/hangar-team-protocol" }
```

```rust
use hangar_team_protocol::wire::{decode_client, ClientMessage};

fn on_text(text: &str) {
    match decode_client(text) {
        Ok(frame) => match frame.message {
            ClientMessage::Ping => { /* answer with ack */ }
            _ => { /* apply */ }
        },
        // `code()` is the `err` code to send; the text quotes nothing from the frame.
        Err(error) => eprintln!("refused: {error} ({:?})", error.code()),
    }
}
```

## Checks

```sh
cargo fmt --manifest-path crates/hangar-team-protocol/Cargo.toml --check
cargo clippy --manifest-path crates/hangar-team-protocol/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/hangar-team-protocol/Cargo.toml
py -3 crates/hangar-team-protocol/test_vectors.py
```

The last command computes the keyed hash, key derivation and membership log hash test vectors with
Python's standard library and fails when one differs from its constant in `src/hash.rs` or
`src/e2e.rs`; it also checks the table of format characters in `src/text.rs` against Python's
`unicodedata`.

## Changelog

Protocol version 1 is not deployed yet, so additions before the first release keep version 1.

- **Version 1, initial:** envelope, presence, heartbeats, reservations and release requests, claims
  and overlaps, blobs, the event log with replay and resync, `key.rotated`, `notify`.
- **Version 1, additions (2026-11-09), for the relay:**
  - Step E groundwork without cryptography: `device.keys`, `devices.get` and `devices`;
    `memberlog.append`, `memberlog.get`, `memberlog.entries` and `memberlog.appended` (logged)
    with a SHA-256 hash chain; `key.rotate` (owner or admin, epoch exactly current + 1),
    `key.wrap` and `key.get` (answered by `ack` with the wrapped key as `content`). New limits
    `MAX_WRAPPED_KEYS` (512) and `MAX_LOG_ENTRIES` (256); `MemberLogEntries::page` builds a
    `memberlog.entries` answer that respects both the count and the 1 MiB frame.
  - `key.rotated` is documented as per device: each device gets only its own wrapped key, live
    and on replay.
  - `member.role_changed` (logged) instead of a second `member.joined`.
  - Presence state `offline`, sent only by the server when a device's last connection closed.
  - Documented rules: `welcome` and `resync_required` carry the `hello`'s op id; which intents
    are deduplicated by op id (`ClientMessage::deduplicated`); `notify` is created by the
    server only; frames over 1 MiB are refused by the relay's WebSocket reader before decoding.
- **Version 1, additions (2026-11-09), second round for the relay:**
  - `ticket.renew {ticket}`: a fresh website ticket about every 5 minutes on a live connection;
    verified like `hello`, applies the new role; answered with `ack`, not deduplicated.
  - Close codes and the `hello` deadline as constants (module `close`): `CLOSE_HELLO_TIMEOUT`
    (1008, no valid `hello` within `HELLO_TIMEOUT_SECS` = 15), `CLOSE_ROLE_CHANGED` (4401),
    `CLOSE_REMOVED` (4403).
  - Rights helpers with typed errors and `err` codes: `check_append_role` (`memberlog.append`
    by owner or admin only) and `check_wrap` (`key.wrap` by owner, admin or another device of
    the target's member; never over a key the device already has in the epoch: `conflict`).
  - `key.wrapped {epoch, holder}`, live only and sent only to that device, so a new device
    knows when to call `key.get`.
  - Documented: the team id must be part of a membership log entry's signed content (step E);
    the relay requires the ticket's `aud`; `ticket.renew` before an accepted `hello` is
    `badRequest`, and one for another team (`prj`) is `forbidden` followed by close 4403.
  - Devices are named by their full holder (member and device) wherever a target device is
    named: `WrappedKey.holder` (in `key.rotate` and `key.wrap`) and `key.wrapped.holder`
    replace the bare `device`, since clients choose device ids and they are unique only per
    member. `check_wrap` takes the target from the `key.wrap` message; `check_rotate` allows
    one key per holder. A rotation to 512 devices now fits with wrapped keys of up to about
    1.7 KiB each.

The crate is standalone (its own `Cargo.toml` and `Cargo.lock`, no workspace), so it builds
without Tauri and without touching the app's build folder.
