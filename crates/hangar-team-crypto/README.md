# hangar-team-crypto

End-to-end cryptography of Hangar teams: device keys, the signed membership log, key epochs with
HPKE wrapping, sealed and signed records, and the printable recovery code. Builds on the wire
types and checks of [`hangar-team-protocol`](../hangar-team-protocol). MIT licensed like the app.

> **Needs an external cryptographic review before it protects real data.** It is a hand-built
> protocol from standard parts (Ed25519, X25519, HPKE, XChaCha20-Poly1305, HKDF, SHA-256), not
> MLS. Test vectors and an independent Python reference exist; neither replaces a review.

The crate does no I/O: no network, no files, no clock, no keychain. Randomness comes in through
the `Entropy` trait (`OsEntropy` in the app), time as `UnixMillis`. Secrets are wiped on drop,
compared in constant time (`subtle`), and never shown by `Debug`.

## Modules

| Module      | Content                                                                                                                                                                                                                                       |
| ----------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `identity`  | `DeviceIdentity` (Ed25519 + X25519), its versioned keychain format, `prove` (proof of possession, sent in `device.keys`), `endorse`, `safety_number` and `log_safety_number`                                                                  |
| `memberlog` | `EntryBody` (canonical, versioned, signed), `LogVerifier` (`apply` on clients, `admit`, `admit_rotation` and `admit_device_remove` on the relay), `TeamState` (members with re-admission, roles, devices with key history and issuer, epochs) |
| `relay`     | The relay's checks that need the verified log: `check_wrap` (`key.wrap`), `check_registered_keys` (device keys equal to the holder's own `device.keys`)                                                                                       |
| `epoch`     | `TeamSecret`, `EpochCommitment`, `wrap`, `unwrap` and `unwrap_any` (HPKE RFC 9180), `HistoryLink` (previous secret under the new one)                                                                                                         |
| `rotation`  | `plan_rotation` (`key.rotate` with the signed rotate epoch entry), `wrap_to_all` (reports failures per device), `wrap_for_device` (`key.wrap`)                                                                                                |
| `record`    | `EpochCipher` (`for_epoch`, `seal`, `open_verified`), `RecordKind`, `SendCounter`, `ReplayGuard` (persistable high-water mark per sender and signing key)                                                                                     |
| `recovery`  | `RecoveryCode`: 128 bits, Crockford base32 with checksum, derives an offline device that may only restore its member                                                                                                                          |

Byte formats are documented at the top of each module.

## Threat model in short

- **The relay** (and whoever runs it) stores and forwards everything. It cannot read sealed
  records or wrapped keys, cannot substitute device keys (clients trust only keys the signed log
  vouches for, each with a proof of possession), cannot inject an epoch secret (a device accepts
  only the secret whose commitment an owner or admin signed into the log), cannot move a log
  entry to another team or position (the signed body names team, index and predecessor), and
  cannot move a record to another team, epoch, kind, record id or sender (associated data plus
  signature).
- **The relay checks the log too.** It runs every `memberlog.append`, `key.rotate` and
  `device.remove` through the same `LogVerifier` as the clients before storing it (signature of
  the sending device, rights, team, index, predecessor), stores a rotate epoch entry only
  together with its wrapped keys and removes a device only together with its signed removal. It
  caps the issuer's rights at the ticket's role, so a downgrade on the website applies at once.
  It needs no secret for that.
- **The relay still can** see the member and device list, roles, epochs, sizes and times; withhold
  new log entries or show two members different logs. A fork is detected when a record crosses
  it (each record names its sender's log head) or when people compare `log_safety_number`; the
  epoch keys bind the log hash of the rotation, so the two sides of a fork cannot read each
  other. It can drop or delay messages.
- **Members** share each epoch's key. A removed member keeps what it saw; the next rotation locks
  it out of new content. A member's device signatures stop other members from writing in its
  name. Owners and admins manage members and roles; every member adds devices to itself (with an
  existing device's endorsement) and removes its own other devices. No owner or admin can add a
  device to another member's identity alone.
- **A first contact** is trust on first use unless the safety numbers are compared: the creator's
  device keys and every approved device's keys rest on that comparison.
- Out of scope: a compromised device (it holds the keys), traffic analysis, denial of service.

## For the relay

The relay depends on this crate (MIT, pure, no secrets) for `memberlog` and `relay`. It passes
the connection's holder and the role of its verified ticket to every check:

- One `LogVerifier` per team, rebuilt from the stored entries with `apply_all` after a restart.
- `memberlog.append`: `relay::check_registered_keys(append, registered)` (every device the entry
  adds carries the keys that holder sent in `device.keys` over its own connection; recovery
  devices excepted), then `admit(append, sender, ticket_role)`. A rotate epoch or remove device
  entry is refused here.
- `key.rotate`: `admit_rotation(rotate, sender, ticket_role)`; store entry and keys together.
- `device.remove`: `admit_device_remove(remove, sender, ticket_role)`, then drop the device
  (`Admitted::removed`: reservations, claims, keys, connections with close 4403).
- `key.wrap`: `relay::check_wrap(state, ticket_role, sender, wrap, stored_from)`, where
  `stored_from` lists the senders of the packages already kept for that device in the epoch.
- `LogError::code()` and `KeyWrapError::code()` give the `err` answer. `Admitted::removed`
  also names the devices a remove member entry took out.

## For the app

- Store a rotation's secret in the OS keychain before sending `key.rotate`.
- Store `SendCounter::last` before sending each record and `ReplayGuard::marks` after accepting
  one, so a restart neither reuses nor re-accepts a counter.
- Delete fetched wrapped keys once the epoch secret is in the keychain, and wipe the device
  identity from the keychain when the log removes the device.
- Say in the UI that a rotation with history lets every future member read back through it.
- Show `Member::readmitted_after` with `added_by` and `added_at`: a member id the website handed
  out again comes back with new devices.

## Using it

```toml
[dependencies]
hangar-team-crypto = { path = "../hangar/crates/hangar-team-crypto" }
```

```rust
use hangar_team_crypto::identity::DeviceIdentity;
use hangar_team_crypto::OsEntropy;

fn new_device() -> Result<(), Box<dyn std::error::Error>> {
    let identity = DeviceIdentity::generate(&mut OsEntropy)?;
    let secret = identity.export(); // into the OS keychain, wiped on drop
    let keys = identity.public_keys(); // with `prove`, send as `device.keys`
    let _ = (secret, keys);
    Ok(())
}
```

## Changelog

- **Unreleased:** `EpochCipher::open_verified` checks the sender's keys and signature before the
  log head's hash. A record with a known head index and a different hash now reports
  `OpenError::Fork` only when its signature verifies; otherwise it fails with `Signature`,
  `UnknownSender` or `SenderNotValid`. Behaviour change for callers that matched on `Fork`:
  an unsigned or forged record can no longer make an opener stop trusting the team.
  It also checks the epoch's start and decrypts before comparing the head's hash: `Fork` now
  needs a record sealed with the current epoch's key, so the old signing key of a device id
  removed and admitted again (whose key period ends before the epoch once the rotation the
  removal made due has happened) fails with `HeadBeforeEpoch` instead of reporting a fork
  against the honest device.

## Checks

```sh
cargo fmt --manifest-path crates/hangar-team-crypto/Cargo.toml --check
cargo clippy --manifest-path crates/hangar-team-crypto/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/hangar-team-crypto/Cargo.toml
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --manifest-path crates/hangar-team-crypto/Cargo.toml
py -3 crates/hangar-team-crypto/test_vectors.py
```

The last command reimplements Ed25519, X25519, ChaCha20-Poly1305, XChaCha20-Poly1305, HKDF and
HPKE base mode with Python's standard library, checks each against its published test vector
(RFC 8032, RFC 7748, RFC 9180 A.2.1, draft-irtf-cfrg-xchacha-03 A.3.1, RFC 5869, plus the five X25519
low-order points), and fails when
a `VECTOR_*` constant in `src/tests.rs` differs from its own result. The Rust tests check the same
RFC vectors against the Rust libraries.

Design notes: `docs/wiki/pages/concepts/team-crypto.md` in the repository wiki.
