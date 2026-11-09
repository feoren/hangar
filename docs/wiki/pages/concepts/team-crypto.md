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
the app wires storage and transport later; the relay depends on it for the membership log
checks. Byte formats are in each module's doc comment; this page records the decisions and the
threat model. Not reviewed externally yet: a review is a release blocker. Hardened after an
internal security review (2026-11-09); the findings' ids (H1 to L7) are cited below.

## Building blocks

| Purpose                                  | Primitive                                                                            | Crate (license)                                      |
| ---------------------------------------- | ------------------------------------------------------------------------------------ | ---------------------------------------------------- |
| Device signatures (log entries, records) | Ed25519, strict verification                                                         | `ed25519-dalek` 3.0 (BSD-3-Clause)                   |
| Wrapping an epoch secret to a device     | HPKE base mode, DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, ChaCha20Poly1305 (RFC 9180) | `hpke` 0.15.1 (MIT/Apache-2.0), default features off |
| Sealed records, history links            | XChaCha20-Poly1305 (random 192-bit nonces)                                           | `chacha20poly1305` 0.11 (MIT/Apache-2.0)             |
| Key derivation, hashes                   | HKDF-SHA256, SHA-256                                                                 | `hkdf` 0.13, `sha2` 0.11 (MIT/Apache-2.0)            |
| Randomness                               | OS generator, fallible                                                               | `getrandom` 0.4 (MIT/Apache-2.0)                     |
| Encoding                                 | canonical base64url                                                                  | `data-encoding` 2.11 (MIT)                           |
| Constant-time comparison                 | `ConstantTimeEq`                                                                     | `subtle` 2.6 (BSD-3-Clause), already in the tree     |

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
- **The relay validates every entry with the clients' verifier** (security review H1). The relay
  depends on this crate for `memberlog` only (pure, no secrets) and keeps one `LogVerifier` per
  team. `LogVerifier::admit(append, sender, ticket_role)` checks chain, signature, team, index,
  predecessor, the rights of the entry's kind, and that the sending connection is the issuer;
  nothing is stored before it passes. After a restart the relay rebuilds the verifier from its
  stored entries. Clients still verify every entry themselves; the relay check keeps a member
  from stopping everyone's log with an entry the clients would refuse.
- **The ticket role caps the log role on the relay** (second round, N2). The relay passes the
  role of the sender's verified ticket to `admit`, `admit_rotation` and `admit_device_remove`;
  the issuer acts with the lower of its log role and its ticket role
  (`TeamState::effective_role`). Entry 1 needs an owner's ticket. A member the website
  downgraded therefore loses the rights of its old role at once, before an owner records the
  change in the log; a ticket never raises a role. Clients see no tickets and check the log
  alone, so the relay is the stricter side here.
- **The relay's other checks live in this crate** (N2b, R2). `relay::check_wrap` needs the
  verified state and so cannot live in the protocol crate: sender and target must be devices the
  log lists now, rights follow the effective role, one package per sender, target and epoch
  (`conflict`), at most four per target (`limit`), `notFound` for a target the log does not
  list. `relay::check_registered_keys` requires that every non-recovery device an entry adds
  carries the keys its holder sent in `device.keys` over its own authenticated connection (R1),
  so an approver cannot write keys of its choice for a holder.
- **A rotation is one message** (H2). `key.rotate` carries the signed rotate epoch entry and the
  wrapped keys; the relay accepts both or neither (`admit_rotation`: rights, entry valid, epoch
  equal to the log's next one, every wrapped key for a distinct device of the log) and refuses a
  rotate entry sent through `memberlog.append`. So the log never names an epoch whose keys the
  relay does not hold. The rotating client stores the new secret in its keychain before it sends.
- **A device removal is one message too** (second round). `device.remove` carries the signed
  remove device entry; the relay admits it (`admit_device_remove`: entry valid under the log's
  rule, removing exactly the named holder) and only then drops the device's reservations,
  claims, keys and connections. A remove device entry through `memberlog.append` is refused, and
  a bare `device.remove` does not decode, so the log and the relay's directory never disagree.
  Removing a member stays a `memberlog.append`; `Admitted::removed` names its devices.
- **Proof of possession and endorsement** (M1). Every device in the log carries its own Ed25519
  signature over team, holder, both keys and the recovery flag (`DeviceIdentity::prove`, sent in
  `device.keys`). An add device entry also carries the endorsement of a current device of the
  member who gets it (`DeviceIdentity::endorse`). An owner or admin therefore cannot add a device
  to another member's identity alone; a member's first devices come with the add member entry of
  an owner or admin. Possession of the X25519 key cannot be shown by a signature; a wrong one
  only keeps that device from receiving keys. `TeamState` keeps the issuer and the endorsing
  device of every key period (`added_by`, `key_periods`) for the app's "added by" note.
- **Recovery devices only restore their member** (M2). A recovery device may issue an add device
  entry for its own member and remove its member's other devices; nothing else. Records signed
  by a recovery device are refused (`OpenError::RecoverySender`), so a stolen code cannot speak
  for the member.
- **Every member manages its own devices** (M3). Any role adds a device to itself (with an
  endorsement) and removes another device of its own; the owner removes any other device, an
  admin a member's device. The relay applies the same rule through the entry `device.remove`
  carries. Removing a device makes a rotation due; only an owner or admin rotates.
- **Device limits** (N1). A member has at most `MAX_DEVICES_PER_MEMBER` (16) devices and the
  team at most `MAX_WRAPPED_KEYS` (512), recovery devices included; an add member or add device
  entry over either is refused (`TooManyDevices`, `limit`). So every rotation fits one
  `key.rotate`, and one member cannot fill the team's device budget.
- **Fork resistance** (M4). The epoch keys bind the log hash of the epoch's rotate entry
  (`EpochKeys::derive(secret, team, epoch, rotation)`), and that hash commits to the whole log up
  to the rotation: two sides of a fork derive different keys from the same secret. Every record
  carries its sender's log head (index and hash) in the signed context; `open_verified` refuses a
  head with another hash at a known index (`Fork`), asks to fetch the log for a newer head
  (`AheadOfLog`) and refuses a head from before the epoch. `Fork` is reported only after the
  sender's signature verified with its keys valid at that index, the head is not from before
  the epoch, and the record decrypted with the current epoch's key: anyone can write a head with
  a wrong hash, and an unsigned record must not make the app distrust anyone (otherwise it fails
  with `Signature`, `UnknownSender` or `SenderNotValid`). Once the rotation the removal made due
  has happened, whoever holds the old signing key of a device id that was removed and admitted
  again can only name heads of the old key period, which end before the epoch: `HeadBeforeEpoch`,
  never a fork against the honest device (before that rotation, the removed device's residual
  below applies). A signed fork in a live record
  sets only that sending device aside: the app hides its presence and overlaps, ignores its live
  records until this device's verified log changes, and names it in the Team view ("reports a
  different team history"). It does not stop the team: one listed device (compromised, buggy, or
  shown another log by the relay) could otherwise stop everyone's sharing with a single record,
  and this device's own log is still the one it verified. The fork may also be this device's
  (the relay showed it an entry the others never got, past the head it stored), so every such
  record makes it fetch the log again, and when devices of two distinct members report another
  history, it stops (`HistoryMismatch`). Devices of one member never stop it, however many and
  whether or not other devices are heard: which devices are heard is the relay's word (unsigned
  `offline`, a new connection), and one person must not stop everyone. It also stops when its
  own log fetch contradicts the head it verified before; which side of a fork
  is real only the people comparing log safety numbers can tell. Fork detection from live
  records is best effort: since decryption comes before the hash comparison, a real fork whose
  branch derived another epoch key (the fork lies before the epoch's rotate entry) ends as a
  decryption failure (`Decrypt`), not as `Fork`, and such a record is simply not shown. The
  authoritative check is the device's own log sync against the head it verified. `TeamState::log_safety_number_at`
  gives the safety number at any verified index, so two people compare at an entry both have.
- **Freshness** (M5). Each device numbers its records with a 64-bit counter in the signed
  context (`SendCounter`, stored before each send). `ReplayGuard` keeps the highest counter per
  sender and signing key, refuses anything at or below it, and exports its marks for the app to
  store, so a restart does not reopen the window. Keying by the signing key too (N3) gives a
  device id that was removed and added again with new keys a fresh mark. The app resumes every
  counter at a clock floor (Unix milliseconds times 1024), so such a device continues above its
  old counters; Hangar's live marks are therefore kept per device alone, which also refuses its
  records signed with the old keys before the next rotation. It is meant for records that arrive once and in order (the live stream);
  state the relay sends again after a resync is opened without it.
- **Key validation** (L1). X25519 keys must be canonical (below `2^255 - 19`, bit 255 clear) and
  not one of the five canonical low-order points; Ed25519 keys valid points of large order.
  `wrap_to_all` reports a failure per device instead of failing the whole rotation.
- **Key history** (L2). `TeamState` keeps every key period per holder, also after removal.
  `EpochCipher::open_verified` takes the sender's key from it, valid at the record's log head, so
  records of a removed device from before its removal still verify and later ones do not.
- **Hygiene** (L3). `chacha20poly1305` with `zeroize` (key and cipher state wiped on drop),
  `subtle` for constant-time comparisons of secrets and commitments, HKDF through
  `hash::hkdf_sha256`, which wipes its own copy of the pseudorandom key (the copy inside the
  `Hkdf` value lives in the HMAC state the `hmac` `zeroize` feature wipes; temporaries left on the
  stack by moves are out of reach). `RecoveryCode::generate` returns `EntropyError`.
- **Up to four wrapped keys per device and epoch** (L5). Since HPKE does not authenticate the
  sender, a member could store garbage for a device first and block the real key. The relay
  keeps up to `MAX_KEY_PACKAGES` (4) per holder and epoch, first come first kept, `key.get`
  returns all of them, and the device accepts the one matching the commitment
  (`TeamState::unwrap_any`).
- **Epoch commitments in the log.** HPKE base mode does not authenticate the sender; anyone can
  wrap a secret of their choice to a device. The rotate epoch entry carries
  `SHA-256(label, team, epoch, secret)`, and a device accepts an unwrapped secret only when it
  matches.
- **History by link, opt-in per rotation** (L6). The rotate entry may carry the previous secret
  encrypted under a key derived from the new one. Whoever receives the new key, including every
  member added later, can walk back through every linked rotation. The UI must say so when an
  admin grants history; a rotation without a link cuts history off.
- **Records are signed as well as sealed.** The shared epoch key alone would let any member
  write in another member's name. The signature covers the associated data (team, epoch, kind,
  record id, sender, log head, counter) plus nonce and ciphertext; it is checked before
  decryption.
- **Roles.** The owner manages everyone; an admin manages members and itself; only owners and
  admins add members, remove members and rotate; only the owner changes roles; no one removes the
  owner or itself; no second owner. **No ownership transfer in version 1** (L7, open). Every
  public key is accepted once per team, never again after removal.
- **Re-admission is visible** (R1). The website decides who is a member, so a member id that was
  removed can be added again with new devices. `Member` records `added_at`, `added_by` and
  `readmitted_after` (the index of the earlier removal); the app shows that the member came back
  and who let it in.
- **Safety numbers**: 30 digits (about 99.7 bits) from SHA-256 over holder and both keys, in
  groups of five; the same digits over a log head detect a relay that shows two members
  different logs.
- **Recovery code**: 128 random bits plus version and a 24-bit checksum, Crockford base32 in
  eight groups of four; HKDF derives an offline device's Ed25519 seed and X25519 key. It is a
  device of the log (`recovery: true`), so every rotation wraps to it. Using it means signing in
  as that device, adding the new installation to its own member, removing the recovery device
  from the new one, and an owner or admin rotating.

## Obligations of the app

- Store a rotation's secret in the OS keychain before sending `key.rotate`.
- Store `SendCounter::last` before sending a record and `ReplayGuard::marks` after accepting
  one. The app (step F part 2a) also resumes the counter from at least Unix milliseconds times
  1024 on every start, so a lost or restored state file cannot repeat a counter; a failed write
  of the state file ends the connection before the record leaves.
- Show a member about to be added again (`TeamState::departed_at` while the member is not
  listed) before the person confirms.
- Delete fetched wrapped keys it no longer needs once the epoch secret is in the keychain, and
  wipe the device identity from the keychain when the log removes the device (L4).
- Show who added a device (`TeamState::added_by`) and warn that granted history reaches every
  future member.
- Show a re-admitted member (`Member::readmitted_after`, with `added_by` and `added_at`).
- Send a device removal as `device.remove` with its signed entry, never through
  `memberlog.append`.
- Rotating a device's own X25519 key is future work (L4): today a device keeps its keys until it
  is removed and added again under a new id.

## Trust of relay events in the app

The relay's events are not signed; the app trusts each only for what a lying relay could do
anyway (delay, drop, disconnect), never for deleting keys or raising rights:

| Event | Trusted for |
| --- | --- |
| `memberlog.appended`, `key.rotated`, `key.wrapped` | a hint to fetch the log or the key; what counts is the verified log and `unwrap_any` against its commitment |
| `key.rotated` with `wrapped` | kept only for epochs above the verified current one, at most 4, dropped on every `hello` |
| `device.removed`, `member.removed` | a log fetch; keys go only when the verified log removed this device |
| `member.removed` naming this member, close 4404 | a check of the website's project list (authenticated HTTPS); keys go and sharing turns off only when the project is gone there |
| `member.role_changed` | a reread of the website's project list; the role comes from the website, capped by the log |
| close 4403 | stop with "removed", keys kept (a relay can only disconnect) |
| close 4401, `err forbidden` on `hello` | a fresh ticket; one immediate retry per settled connection |
| `memberlog.entries` with `more` | only with at least one entry (progress), else the connection ends |
| `presence.update` | shown only for a device the verified log lists (one fetch and retry for an unknown one); the status only when it opens with `open_verified` under the current epoch or a newer one and its counter is not below the newest one shown for that device (marks in the state file, kept across `offline`, reconnects and restarts, and for removed devices the log still knows); without a status the sessions stay as last signed; `offline` (unsigned) only hides the device; at most one entry per listed device; a signed fork sets only that device aside |
| `task.reserved`, `task.released`, `reservation.expiring`, `reservation.expired`, `task.release_requested`, `task.release_answered`, `task.release_request_ended` | applied in the relay's order, exactly once (the event log cursor), only for a holder the verified log lists; an event whose holder the log does not list yet waits in order for the next log fetch (at most 1024, else a fresh snapshot; such fetches at most every 5 s, else at the next ping), then it is dropped; at most 32 holds per member and as many as the log lists members; title, area and linked session only from the holder's note, opened with `open_verified` as a reservation note bound to the task's hash and the holder (record id: `record::task_record_id`, the hash's 64 lowercase hex digits), its title must hash to the task under the note's epoch and its name must have no control, invisible or reordering character; a note older than one shown is ignored, and so is the last note of a hold that ended (counter marks per device and note in the state file, at most 64 per device with a floor, kept across resyncs and restarts); a release request gets the holder's card only with a note one of the requester's listed devices signed and not shown before, and the answer must name that requester; notices only when the hold or this device's request was there; the reservations of before a restart come from the relay's snapshot (the first `hello` of an engine sends no cursor) |
| `err conflict` with `holder` on `task.reserve` | shows the attempt as waiting for that holder (and pauses its linked session) only when the log lists it; an unlisted one is checked again after the next log fetch, then the attempt ends with a notice |
| `task.released`, `reservation.expired`, handover | accepted risk: not signed by the holder; the relay decides first wins and can end or hand over a hold |
| notices about reservations | one per event the log vouches for (deduplicated warnings), at most 8 kept; desktop notifications at most once a minute |
| `notify` | ignored: the events it stands for reach every device of the person through the log |
| `claim.overlap` | for one of this device's own session keys and a listed device, and only for a session the device's newest signed status lists (without any status: at most "same area", no notice); the detail only as for a status (counter mark per device and session key, at most 64 sessions per device, older ones refused by a floor); a signed detail with no file of ours caps the overlap at "same area" whatever the relay's level; at most 64 overlaps per own session and listed device; desktop notices at most once a minute |

## Threat model

Adds to the relay's view in [[team-protocol]] (threat model section):

- **Cannot** read sealed content or wrapped keys; substitute device keys; enter keys it does not
  hold; inject an epoch secret; store an entry the clients would refuse; separate a rotation
  from its keys; move log entries between teams or positions; move records between teams,
  epochs, kinds, record ids or senders; forge a member's record or log entry; add a device to a
  member without that member's consent, even as owner or admin.
- **Can** withhold new log entries or fork the log per member: detected when a record crosses
  the fork, or by comparing log safety numbers; the sides of a fork cannot read each other's
  records. It can replay old records (the app's `ReplayGuard` refuses them where order holds),
  and drop or delay anything.
- **Members** keep what they saw after removal; the next rotation locks them out of new content.
  A removed device can still seal records naming an old log head until the next rotation (the
  relay refuses content of an old epoch afterwards). A compromised device has its keys and is out
  of scope.
- **First contact** is trust on first use unless safety numbers are compared.

## Tests

56 unit tests: whole-team flows where every entry passes the relay's `admit` (a removal through
`admit_device_remove`) and a client's `apply` with the same verdict (create, add, rotate as one
unit, unwrap on every device, records across devices, removal and history, `key.wrap` with
garbage packages first, recovery and its limits), every log rule and binding, proofs and
endorsements, member self-service, relay-only refusals (`NotSender`, rotation outside
`key.rotate`, removal outside `device.remove`, wrapped keys for unknown holders, the ticket role
capping the log role, entry 1 only with an owner's ticket), device limits (17 for one member, 513
for the team), `relay::check_wrap` and `relay::check_registered_keys`, re-admission with a
reused device id and its fresh replay mark, key history of removed devices, fork and newer-log detection through record heads, replay marks
across a restart, X25519 key validation (the low-order constants are pinned by HPKE refusing
them), record context binding, size limits, malformed inputs. Anchors: Ed25519 against RFC 8032
test 1, X25519 and HPKE against RFC 9180 A.2.1, XChaCha20-Poly1305 against
draft-irtf-cfrg-xchacha-03 A.3.1. `test_vectors.py` reimplements every primitive with the Python
standard library, checks each against the same published vectors (and RFC 7748, RFC 5869, and the
five low-order X25519 points giving an all-zero shared secret), and recomputes the crate's 16
vectors (keys, safety number, three log entries with their hashes, commitment, wrapped key,
history link, record with log head and counter, recovery code and its keys) independently.

> [!question] Unverified
> The RFC test vectors were read from the RFC Editor and IETF archive texts on 2026-11-09 and are
> not filed as source pages yet. That the `zeroize` features wipe every copy of key material (HMAC
> state inside `hkdf`, ChaCha state) is taken from the crates' feature descriptions and source,
> not checked in memory.

## Questions for the external review

The internal security review raised these ten questions for an external cryptographer. Its
findings are addressed above; the questions stay open until the external review answers them.

1. Is the commitment model (the secret's hash in the signed log) enough instead of HPKE Auth or
   PSK mode, also with several admins rotating at once?
2. Should the rotate entry's hash go into the key derivation, and which fork detection is
   adequate without a transparency log? (The hash is now bound, see fork resistance above.)
3. Is the rights model (an admin adds other members' devices, a recovery device with full rights)
   compatible with the threat model's promises? (Both were changed: endorsements and recovery
   devices restricted to their own member, see above.)
4. May the relay validate the log, and what metadata consequences does that have?
5. Record freshness: a per-sender counter or a timestamp, and how does either behave with several
   devices per member?
6. Are forward secrecy and post-compromise security with static device KEM keys and persistently
   stored wrapped keys acceptable for the product?
7. History link policy: per rotation instead of per member, and what happens on re-admission?
8. One Ed25519 key for log entries and records, separated only by labels: enough, or separate
   keys?
9. Key reuse across teams: one device key in several teams, while the safety number does not bind
   the team.
10. Trust on first use: the relay can show a newcomer an entirely invented log from entry 1; how
    is the owner's identity tied to the website account?

Further questions from the hardening round:

1. Is binding the rotate entry's log hash into HKDF `info` (with the commitment checked against
   the same log) sufficient fork resistance, or should the epoch secret itself be bound to the
   log (for example in the HPKE `info` of the wrapped key)?
2. Does a proof of possession that covers the Ed25519 key only (X25519 cannot sign) leave a
   practical attack, given that every key is accepted once per team?
3. Are the HPKE `info` contents (team, epoch, member, device) and the empty associated data the
   right binding for wrapped keys, or should the commitment be included?
4. Is a random 192-bit nonce for XChaCha20-Poly1305 under one key per epoch safe for the expected
   record volume, or is a counter-derived nonce preferable now that each sender has a counter?
5. Is the history link construction (previous secret under an HKDF key of the new secret, checked
   against the previous commitment) sound, including for members added much later?
6. Does the record signature over `field(aad) || field(nonce) || field(ciphertext)` with the aad
   also inside the AEAD add or lose anything compared with signing the plaintext?
7. Is the five-point low-order list plus canonical encoding a complete X25519 public key
   validation for HPKE with `hpke` 0.15 (which also refuses an all-zero shared secret)?
8. Are 30-digit safety numbers (about 99.7 bits, SHA-256 truncated per five bytes modulo 100000)
   adequate for device and log comparison, and is the slight modulo bias acceptable?
9. Is a 128-bit recovery code with HKDF (no password hashing) adequate when its public keys are in
   the plaintext log for offline guessing?
10. Which properties of MLS (RFC 9420) does this design lack that matter for teams of up to 512
    devices (post-compromise security beyond rotation, forward secrecy within an epoch,
    authenticated group state), and is moving to MLS advisable before release?

## Open

- External cryptographic review before release (questions above).
- Ownership transfer is not part of version 1 (L7).
- Rotating a device's own X25519 key (L4).
- No persisted `TeamState`: clients and the relay re-verify the log from entry 1 (fine for
  hundreds of entries).
- Shared transcripts with their own key per session (step G) are not covered.
