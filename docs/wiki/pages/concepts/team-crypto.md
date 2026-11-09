---
title: "Team crypto: device keys, membership log, epochs, sealed records, recovery code"
type: concept
tags: [collaboration, teams, e2e, ed25519, x25519, hpke, xchacha20poly1305, hkdf, threat-model]
sources: []
created: 2026-11-09
updated: 2026-11-09
---

# Team crypto

Team service step E ([[team-service-v1]], section 3), implemented in `crates/hangar-team-crypto`
(MIT) on top of the protocol crate ([[team-protocol]]). Pure: no I/O, no keychain, no network;
the app wires storage and transport later. Byte formats are in each module's doc comment; this
page records the decisions and the threat model. Not reviewed externally yet: a review is a
release blocker.

## Building blocks

| Purpose                                  | Primitive                                                                            | Crate (license)                                      |
| ---------------------------------------- | ------------------------------------------------------------------------------------ | ---------------------------------------------------- |
| Device signatures (log entries, records) | Ed25519, strict verification                                                         | `ed25519-dalek` 3.0 (BSD-3-Clause)                   |
| Wrapping an epoch secret to a device     | HPKE base mode, DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, ChaCha20Poly1305 (RFC 9180) | `hpke` 0.15.1 (MIT/Apache-2.0), default features off |
| Sealed records, history links            | XChaCha20-Poly1305 (random 192-bit nonces)                                           | `chacha20poly1305` 0.11 (MIT/Apache-2.0)             |
| Key derivation, hashes                   | HKDF-SHA256, SHA-256                                                                 | `hkdf` 0.13, `sha2` 0.11 (MIT/Apache-2.0)            |
| Randomness                               | OS generator, fallible                                                               | `getrandom` 0.4 (MIT/Apache-2.0)                     |
| Encoding                                 | canonical base64url                                                                  | `data-encoding` 2.11 (MIT)                           |

All share one RustCrypto and dalek generation (`cargo tree -d` shows no duplicates). Versions
taken from `cargo info` on 2026-11-09. `hpke`'s own system RNG panics on failure; the crate draws
the 32 bytes of ephemeral keying material itself and hands them to `hpke` through a one-shot
generator that refuses any other use. That `hpke` asks exactly one 32-byte block per X25519
encapsulation was read in its source (`Kem::gen_keypair_with_rng`) and is pinned by the RFC 9180
A.2.1 test.

## Decisions

- **Membership log entries are signed plaintext, not sealed.** A new device verifies the log
  before it has any team key: the log tells it which keys to trust and which epoch secret to
  accept. The relay already knows members and roles (tickets), devices and public keys
  (`device.keys`) and epochs (`key.rotate`); the log adds only who approved whom and when.
- **The signed body binds team, index and predecessor hash.** The protocol's chain hash is
  unkeyed and does not contain the team, so anyone can compute it over any entries. With the
  binding a validly signed entry fits one position of one team's log only: a relay cannot move it
  to another team or splice it into a different history.
- **Epoch commitments in the log.** HPKE base mode does not authenticate the sender; anyone can
  wrap a secret of their choice to a device. The rotate epoch entry carries
  `SHA-256(label, team, epoch, secret)`, and a device accepts an unwrapped secret only when it
  matches. This also makes a member's own device or an admin equally able to deliver the key
  (`key.wrap`) without a second signature.
- **History by link, opt-in per rotation.** The rotate entry may carry the previous secret
  encrypted under a key derived from the new one. Whoever receives the new key (including members
  added later) can walk back; a rotation without a link cuts history off.
- **Seal key from the protocol's `EpochKeys`.** Records use `EpochKeys::seal_key` (HKDF label
  `hangar-team/v1/seal-key`), so matching and sealing never share a key; the history key uses the
  same HKDF framing with its own label.
- **Records are signed as well as sealed.** The shared epoch key alone would let any member
  write in another member's name. The signature covers the associated data (team, epoch, kind,
  record id, sender) plus nonce and ciphertext; it is checked before decryption.
- **Only owners and admins append**, matching the relay's `check_append_role`. The owner manages
  everyone; an admin manages members and its own devices; no one removes the owner or itself;
  only the owner changes roles; no second owner (no ownership transfer in version 1). Every public
  key is accepted once per team, never again after removal.
- **Safety numbers**: 30 digits (about 99.7 bits) from SHA-256 over holder and both keys, in
  groups of five; the same digits over a log head detect a relay that shows two members
  different logs.
- **Recovery code**: 128 random bits plus version and a 24-bit checksum, Crockford base32 in
  eight groups of four; HKDF derives an offline device's Ed25519 seed and X25519 key. It is a
  regular device of the log (`recovery: true`), so every rotation wraps to it and the relay keeps
  its keys. Using it means signing in as that device, then an admin adds a fresh device, removes
  the used recovery device and rotates.

## Threat model

Adds to the relay's view in [[team-protocol]] (threat model section):

- **Cannot** read sealed content or wrapped keys; substitute device keys; inject an epoch secret;
  move log entries between teams or positions; move records between teams, epochs, kinds, record
  ids or senders; forge a member's record or log entry.
- **Can** withhold new log entries or fork the log per member (detected only by comparing log
  safety numbers out of band); replay old records in the same context (the app uses
  `ReplayGuard` where a record must arrive once); drop or delay anything.
- **Members** keep what they saw after removal; the next rotation locks them out of new content.
  A compromised device has its keys and is out of scope.
- **First contact** is trust on first use unless safety numbers are compared.

## Tests

39 unit tests: whole-team flows (create, add, rotate, unwrap on every device, records across
devices, removal and history, `key.wrap` for a new device, recovery), every log rule and binding,
record context binding, size limits, malformed inputs. Anchors: Ed25519 against RFC 8032 test 1,
X25519 and HPKE against RFC 9180 A.2.1, XChaCha20-Poly1305 against draft-irtf-cfrg-xchacha-03
A.3.1. `test_vectors.py` reimplements every primitive with the Python standard library, checks
each against the same published vectors (and RFC 7748, RFC 5869), and recomputes the crate's 13
vectors (keys, safety number, two log entries, commitment, wrapped key, history link, record,
recovery code and its keys) independently.

> [!question] Unverified
> The RFC test vectors were read from the RFC Editor and IETF archive texts on 2026-11-09 and are
> not filed as source pages yet.

## Open

- External cryptographic review before release.
- The relay lets a member remove its own other device (`device.remove`) but not append to the
  log; the log entry for that removal needs an owner or admin. Either the relay accepts own-device
  entries from members (then the log rules must too) or the app asks an admin.
- `TeamState` keeps only current devices; records of a removed device need its old key (keep a
  key history per holder in the app or in `TeamState`).
- No persisted `TeamState`: the app re-verifies the log from entry 1 (fine for hundreds of
  entries).
- Automatic fork detection (gossiping the log head inside sealed records) is not built.
- Shared transcripts with their own key per session (step G) are not covered.
