# hangar-team-protocol

Wire types, keyed hashes and state machines of the Hangar team protocol. Shared by the Hangar app
and the team relay; MIT licensed like the app.

The crate does no I/O: no network, no files, no clock, no async runtime. Callers pass the time in
(`UnixMillis`) and bring their own transport. Encryption is not part of it yet; encrypted content
travels as opaque `Sealed` values with the `Epoch` of the key that sealed it, and `EpochKeys`
derives that epoch's separate matching and sealing keys.

## Modules

| Module        | Content                                                                                                                                                                                                                                  |
| ------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `wire`        | Client and server messages, the JSON envelope, `encode_*` and `decode_*` with size, field and version checks, epoch checks (`check_epoch`), body digests for retries                                                                     |
| `hash`        | `EpochKeys` (HKDF-SHA256, bound to the epoch's rotate entry hash), `MatchKey` (HMAC-SHA256), normalization of task ids, repository ids and paths (NFC, case rule), `PathHashes`                                                          |
| `reservation` | `Reservation` state machine and `ReservationBook`, the server's "first wins" authority; time events come apart from the outcome (`Applied`); `BookSnapshot` for a restart                                                                |
| `overlap`     | `ClaimBoard`: same area and same file between sessions of different holders, by hash equality; passes the encrypted detail on                                                                                                            |
| `log`         | `EventLog` (sequence numbers, replay window, resync), `ClientCursor`, `RecentOps` (op id retries per sender and team)                                                                                                                    |
| `e2e`         | What the relay checks for step E without a key: `PublicKey` (device keys), `KeyProof`, `LogHash`, `MemberChain` and `check_append` (membership log chain), `check_rotate`; checks that need the verified log are in `hangar-team-crypto` |
| `close`       | WebSocket close codes of the relay and the `hello` deadline                                                                                                                                                                              |
| `ids`         | Identifiers, `Seq`, `UnixMillis`, `Epoch`, `Ciphertext`, `Sealed`                                                                                                                                                                        |
| `limits`      | Size limits both sides enforce                                                                                                                                                                                                           |

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
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --manifest-path crates/hangar-team-protocol/Cargo.toml
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

- **Version 1, additions (2026-11-09), third round for the relay:**
  - Reservations survive a relay restart: `ReservationBook::export` writes a versioned
    `BookSnapshot` (serde; holds with `since`, last activity, offline time and warning, open
    release requests, cooldowns, the epoch and a running rekey grace; no connection counts),
    `BookSnapshot::from_json` reads one (version first), and `ReservationBook::restore` checks it
    (unknown version, a task twice, epochs, times, requests, the per member limit) and returns
    the book with the events the restart caused. After a restore every holder counts as offline
    from the restore's time: a deadline becomes at least restore time plus the warning, so the
    downtime never expires a hold without a warning; release requests and the rekey grace run
    on wall-clock time. `EventLog::position` gives the stream and head to store with the
    snapshot for `EventLog::resume`. The `ClaimBoard` is not persisted: claims are live state
    that clients send again after each `welcome`.
  - `notify` names its recipient (`member`) and is no longer logged: targeted like
    `key.wrapped` (`ServerMessage::recipient`), kept by the relay for an unconnected member and
    delivered after that member's next `welcome`. `Notify::attention` builds it for
    `task.release_requested` and `reservation.expiring` when the holder's member has no
    connected device. E-mail is the website's business.
  - `CLOSE_TEAM_DELETED` (4404): the project was deleted on the website.
  - `device.remove {holder}` (deduplicated; owner or admin for any other device, a member for
    another device of its own: `check_device_remove`) and `device.removed {holder}` (logged).
  - `CLOSE_ROLE_CHANGED` (4401) is documented as the exception: a role change with a known new
    role applies silently on the live connection (`ticket.renew`, the website's revocation
    feed) and `member.role_changed` reports it; 4401 only when the new role is unknown.

- **Version 1, changes (2026-11-09), fourth round after the step E security reviews.** Breaking
  for the relay; nothing is deployed, so the version stays 1. Every role below is the effective
  one: the lower of the member's role in the verified log and the role of the connection's
  verified ticket, so a downgrade on the website applies at once.
  - **The relay validates the membership log.** It now depends on `hangar-team-crypto` (MIT,
    pure, no secrets) and keeps one `memberlog::LogVerifier` per team, rebuilt from its stored
    entries with `apply_all` after a restart. Every `memberlog.append` goes first through
    `hangar_team_crypto::relay::check_registered_keys(append, registered)` (each non-recovery
    device the entry adds carries the keys that holder sent in `device.keys` over its own
    authenticated connection; otherwise `KeysNotRegistered`, `forbidden`), then through
    `LogVerifier::admit(append, sender, ticket_role)` before anything is stored (signature of
    the sending device, rights per entry kind, team, index, `prev`). Entry 1 (create team)
    needs an owner's ticket. Refusals answer with `LogError::code()`. `Admitted::removed` lists
    the devices a remove member entry took out; the relay drops them.
  - **Removed from `e2e`:** `check_append_role`, `MemberLogForbidden`, `check_wrap`,
    `KeyWrapError`, `check_device_remove` and `DeviceRemoveForbidden`. They need the verified
    `TeamState`, which this crate cannot depend on. Members may now append (their own new device
    with an endorsement); `admit` decides per entry.
  - **`key.rotate` carries its log entry:** `KeyRotate` gains `log: MemberLogAppend` (the signed
    rotate epoch entry, within `MAX_INLINE_CIPHERTEXT_LEN`). The relay runs `check_rotate`, then
    `LogVerifier::admit_rotation(rotate, sender, ticket_role)` (entry valid, its epoch equal to
    `epoch` and the log's next one, every wrapped holder a distinct device of the log), and
    stores the entry and the wrapped keys in one step or neither; it emits `memberlog.appended`
    and `key.rotated`. A rotate epoch entry sent through `memberlog.append` is refused
    (`RotationOutsideKeyRotate`, `badRequest`).
  - **`device.remove` carries its log entry:** `DeviceRemove` gains `log: MemberLogAppend` (the
    signed remove device entry, within `MAX_INLINE_CIPHERTEXT_LEN`); without it the message does
    not decode (`badRequest`). The relay runs
    `LogVerifier::admit_device_remove(remove, sender, ticket_role)` (entry valid under the log's
    rule: anyone removes another device of its own, the owner any other device, an admin a
    member's device; removing exactly `holder`), stores the entry and only then removes the
    device (`remove_device`, `clear_device`, keys, close 4403) and emits `memberlog.appended`. A
    device the log does not list is `badRequest` now, not `notFound`. A remove device entry
    sent through `memberlog.append` is refused (`RemovalOutsideDeviceRemove`, `badRequest`).
    Removing a member stays a `memberlog.append`.
  - **`key.wrap`:** checked after the epoch check with
    `hangar_team_crypto::relay::check_wrap`, which takes the team's state, the ticket role, the
    sender, the message and `stored_from`. Sender and target must be devices of the
    log, an owner or admin wraps for any other device, a member only for another device of its
    own (`forbidden`); a target the log does not list is `notFound`; one package per sender,
    target and epoch (`conflict`); up to the new `limits::MAX_KEY_PACKAGES` (4) per target and
    epoch, the `key.rotate` package included, first come first kept, never replaced (`limit`).
    `stored_from` lists the senders of the stored packages.
  - **`key.get` returns every package:** `Ack` gains `keys: Option<Vec<Sealed>>` (at most
    `MAX_KEY_PACKAGES`, each within `MAX_INLINE_CIPHERTEXT_LEN`); `content` is now for
    `blob.get` only. The device tries each package against the epoch's commitment.
  - **`device.keys` and `devices` carry a proof of possession:** `DeviceKeys.proof` and
    `DeviceInfo.proof` (`e2e::KeyProof`, 64 bytes as 86 characters of canonical base64url). The
    relay stores it with the keys and lists it.
  - **Device limits:** new `limits::MAX_DEVICES_PER_MEMBER` (16, recovery devices included).
    The log refuses an entry that gives a member more than 16 devices or the team more than
    `MAX_WRAPPED_KEYS` (512): `LogError::TooManyDevices`, `limit`.
    `memberlog::MAX_DEVICES_PER_ENTRY` drops from 32 to 16.
  - **`EpochKeys::derive(secret, team, epoch, rotation)`** binds the log hash of the epoch's
    rotate entry (new vectors `VECTOR_DERIVED_TASK`, `VECTOR_DERIVED_SEAL_KEY`). The relay never
    derives keys; listed for completeness. New public helper `hash::hkdf_sha256`, which wipes
    its copy of the pseudorandom key.
  - `e2e::PublicKey` now implements `PartialOrd` and `Ord` (the clients' replay marks are keyed
    by holder and signing key).

The crate is standalone (its own `Cargo.toml` and `Cargo.lock`, no workspace), so it builds
without Tauri and without touching the app's build folder.
