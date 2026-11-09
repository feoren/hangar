# hangar-team-crypto

End-to-end cryptography of Hangar teams: device keys, the signed membership log, key epochs with
HPKE wrapping, sealed and signed records, and the printable recovery code. Builds on the wire
types and checks of [`hangar-team-protocol`](../hangar-team-protocol). MIT licensed like the app.

> **Needs an external cryptographic review before it protects real data.** It is a hand-built
> protocol from standard parts (Ed25519, X25519, HPKE, XChaCha20-Poly1305, HKDF, SHA-256), not
> MLS. Test vectors and an independent Python reference exist; neither replaces a review.

The crate does no I/O: no network, no files, no clock, no keychain. Randomness comes in through
the `Entropy` trait (`OsEntropy` in the app), time as `UnixMillis`. Secrets are wiped on drop,
compared without early exit, and never shown by `Debug`.

## Modules

| Module      | Content                                                                                                                                                       |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `identity`  | `DeviceIdentity` (Ed25519 + X25519), its versioned keychain format (`export`, `import`), `safety_number` and `log_safety_number` for out-of-band comparison   |
| `memberlog` | `EntryBody` (canonical, versioned, signed), `LogVerifier` (chain, team and position binding, signature, rules), `TeamState` (members, roles, devices, epochs) |
| `epoch`     | `TeamSecret`, `EpochCommitment`, `wrap` and `unwrap` (HPKE RFC 9180), `HistoryLink` (previous secret under the new one)                                       |
| `rotation`  | `plan_rotation` (`memberlog.append` + `key.rotate`), `wrap_to_all`, `wrap_for_device` (`key.wrap`)                                                            |
| `record`    | `EpochCipher` with `seal` and `open` for `Sealed` content, `RecordKind`, `ReplayGuard`                                                                        |
| `recovery`  | `RecoveryCode`: 128 bits, Crockford base32 with checksum, derives an offline device                                                                           |

Byte formats are documented at the top of each module.

## Threat model in short

- **The relay** (and whoever runs it) stores and forwards everything. It cannot read sealed
  records or wrapped keys, cannot substitute device keys (clients trust only keys the signed log
  vouches for), cannot inject an epoch secret (a device accepts only the secret whose commitment an
  owner or admin signed into the log), cannot move a log entry to another team or position (the
  signed body names team, index and predecessor), and cannot move a record to another team,
  epoch, kind, record id or sender (associated data plus signature).
- **The relay still can** see the member and device list, roles, epochs, sizes and times; withhold
  new log entries or show two members different logs (detected by comparing `log_safety_number`
  out of band, not automatically); and drop or delay messages.
- **Members** share each epoch's key. A removed member keeps what it saw; the next rotation locks
  it out of new content. A member's device signatures stop other members from writing in its
  name. Only owners and admins change the log.
- **A first contact** is trust on first use unless the safety numbers are compared: the creator's
  device keys and every approved device's keys rest on that comparison.
- Out of scope: a compromised device (it holds the keys), traffic analysis, denial of service.

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
    let keys = identity.public_keys(); // send as `device.keys`
    let _ = (secret, keys);
    Ok(())
}
```

## Checks

```sh
cargo fmt --manifest-path crates/hangar-team-crypto/Cargo.toml --check
cargo clippy --manifest-path crates/hangar-team-crypto/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/hangar-team-crypto/Cargo.toml
py -3 crates/hangar-team-crypto/test_vectors.py
```

The last command reimplements Ed25519, X25519, ChaCha20-Poly1305, XChaCha20-Poly1305, HKDF and
HPKE base mode with Python's standard library, checks each against its published test vector
(RFC 8032, RFC 7748, RFC 9180 A.2.1, draft-irtf-cfrg-xchacha-03 A.3.1, RFC 5869), and fails when
a `VECTOR_*` constant in `src/tests.rs` differs from its own result. The Rust tests check the same
RFC vectors against the Rust libraries.

Design notes: `docs/wiki/pages/concepts/team-crypto.md` in the repository wiki.
