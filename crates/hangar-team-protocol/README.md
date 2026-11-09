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

The last command computes the keyed hash and key derivation test vectors with Python's standard
library and fails when one differs from its constant in `src/hash.rs`; it also checks the table of
format characters in `src/text.rs` against Python's `unicodedata`.

The crate is standalone (its own `Cargo.toml` and `Cargo.lock`, no workspace), so it builds
without Tauri and without touching the app's build folder.
