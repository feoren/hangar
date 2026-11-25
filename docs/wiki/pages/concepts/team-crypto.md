---
title: "Team crypto: device keys, membership log, epochs, sealed records, recovery code"
type: concept
tags: [collaboration, teams, e2e, ed25519, x25519, hpke, xchacha20poly1305, hkdf, threat-model, possession]
sources: []
created: 2026-11-09
updated: 2026-11-25
---

# Team crypto

Team service step E ([[team-service-v1]], section 3), implemented in `crates/hangar-team-crypto`
(MIT) on top of the protocol crate ([[team-protocol]]). Pure: no I/O, no keychain, no network;
the app wires storage and transport later; the relay depends on it for the membership log
checks. Byte formats are in each module's doc comment; this page records the decisions and the
threat model. Not reviewed externally yet: a review is a release blocker. Hardened after an
internal security review (2026-11-09); the findings' ids (H1 to L7) are cited below. A second
round on the app side (2026-11-15, ids M1 to N9 with the prefix "round 2") is cited the same way,
and so is a third (2026-11-15, ids M-1 to N-4, prefix "round 3").

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
- **Log length** (round 2, M1). A log has at most `MAX_LOG_ENTRIES` (8192) entries; the last
  `RESERVED_LOG_ENTRIES` (64) places take only removals (remove member and, since round 4,
  remove device) and rotate epoch entries, so an owner or admin can still remove the member who
  filled the log, a person can still remove a stolen device of their own, and the key can still
  change. Remove device entries take only the first half of the reserve: the last
  `DEVICE_REMOVAL_RESERVE` (32) places stay for remove member and rotate epoch entries. A member
  has up to 16 devices and each removal makes an urgent rotation due, so without that split a few
  members removing their own devices could use up the whole reserve, after which they could
  never be removed (round 4 review). The very last place takes only a rotate epoch entry (round 3, N-3): a removal needs a
  place after it for the rotation it makes due, so a log can never end with a removal it cannot
  follow with a key change; an owner's or admin's device does not even try such a removal. The
  engine maps a refused removal to `TeamError::LogFull`, not to an unknown request (round 4). Beyond,
  `LogError::LogFull` (`limit`); the relay refuses such appends through the same verifier, and a
  client given a longer log than that stops with `AttentionReason::LogFull`. Every device
  verifies the whole log on each connection, so its length must stay bounded; 8192 entries are a
  few thousand device lifecycles. Without a relay-side rate limit one member can still fill the
  log within its own device budget (add and remove its own devices); see Open.
  A log that reached the reserved places does not stop anyone (`TeamState::log_full`,
  `TeamActivity::log_full`): sharing, receiving, removals and key changes go on, and every device
  shows a notice that new devices and members can no longer be admitted. A request that no
  longer fits stays in the confirmation list marked as such (`PendingConfirmation::log_full`,
  confirming answers `TeamError::LogFull`), so a waiting device is not left unseen.
  **Way out** (what exists today): owners and admins remove the member who filled the log from
  the project on the website, and the log follows with a remove member entry (reserved place);
  that stops further churn but frees no place, since the cap counts every entry ever written.
  New devices and members then need a new project created on the website (a new team with its
  own log). When the log is at its very last place, a member the website no longer lists cannot
  be removed at all: after the grace period every device, owners and admins included, seals
  nothing new and says so (`KeyChangeWait::LogFull`: the history is full, create a new project),
  instead of waiting for a removal that cannot come (round 4). There is no log reset or compaction; both would need a new trust anchor and are not
  built.
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
  record makes it fetch the log again. Reports never stop it, however many devices and members
  send them (round 2, N7: earlier, reports of two distinct members stopped it, so two members
  working together could stop everyone's sharing with one record each). It stops
  (`HistoryMismatch`) only when its own log fetch contradicts the head it verified before.
  Accepted consequence: when the relay forks this device itself and keeps showing it a
  consistent branch, it goes on alone; every other device then shows as "reports a different
  team history" in its Team view, records across the fork do not open (the epoch keys bind the
  log), and which side of a fork is real only the people comparing log safety numbers can
  tell. Fork detection from live
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
- **Membership follows the website** (third review, H1). The website decides who belongs to a
  project; the log follows it on an owner's or admin's device: it reads the project's members
  over authenticated HTTPS (`GET /api/v1/projects/{id}/members`, see [[hangar-account-api]]) on
  every new connection, after `member.removed` or `member.role_changed` of anyone (unsigned,
  so only a cue) and at least every 10 minutes, and appends a remove member entry for a member
  the list no longer names, one entry at a time. That makes the rotation due, and the next epoch
  is wrapped only to the devices left. A member the log added after the list was asked for
  (log index at the moment of asking) is never removed by that list. On the owner's device a
  role the website lowered (admin to member) becomes a change role entry at once; a role it
  raised (member to admin) only once the owner confirms it in the app (round 2, M2:
  `PendingKind::RoleChange`, shown with the other confirmations, rejected ones stay hidden until
  the member is added again), so the website alone cannot make anyone an admin of the
  end-to-end team. A new member gets the website's role (admin only when the owner confirms the
  new member). The list failing (no answer, an older
  server without the endpoint) only skips this step; a relay that refuses such an entry stops
  further ones until the next list. A list that names neither the reading member nor the log's
  owner (an empty answer, ids of another kind) removes nobody. A list that says it is only part
  of one (round 3, N-2: `next`, `next_cursor` or `cursor` set, `has_more` other than `false`,
  `total` other than the number of entries, `page` without such a `total`) counts as no answer,
  for `/members` and for `/projects` alike, so a partial list never removes a member and never
  makes a project look gone (which would retire the team's keys). Members are never removed by
  hand in the app; a person removes a lost device of their own ("Remove device" in the Team
  view, `device.remove`).
- **New members join as members** (round 3, M-1). Confirming a new member's first device always
  writes the add member entry with the member role, whatever the website lists; a higher role
  there then shows as its own confirmation on the owner's device (`PendingKind::RoleChange`), so
  confirming a device never makes anyone an admin unseen (before, a card without any role on it
  admitted an admin). When the device has the website's list, only someone it names is offered
  for admission at all (round 3, N-4): a device of anyone else the relay reports stays hidden
  and makes the device read the list again (within the 30 second rate), so a person who just
  joined on the website is offered soon.
- **Every admitted device watches the website's list** (round 3, M-2). A relay that withholds a
  remove member entry and the rotation after it from some devices would let a member the
  website removed keep reading what those devices seal. So every device whose admission the
  person confirmed reads the member list too (on each connection and every 10 minutes, same
  rate as owners and admins; these regular reads have their own budget apart from the 30 second
  rate of event checks, so reading the list on connecting never delays the check a
  `member.removed` of this member asks for). When the log still names a member the list left out (only a list
  that names this member and the log's owner, never a member the log added after the list was
  asked for) for longer than `DEPARTURE_GRACE_MS` (10 minutes on this device's clock, from the
  first list that left the member out), a device that cannot remove members itself seals
  nothing new (`TeamStatus::WaitingForKeyChange` with `KeyChangeWait::MemberLeft`: "the team
  key has not been changed since a member left; waiting for the owner or an admin"); receiving
  goes on. It ends when the log removes the member (then the urgent rotation decides), or when
  the list names the member again. Without a list (no answer, a server without the endpoint)
  nothing is concluded and sharing goes on; absence of the list is no removal. A read that fails
  after a list was read keeps the last list in force until the next one answers. Observations
  live in memory: a restart of the app starts the grace period again.
- **Held back history is shown** (round 3, M-2). A teammate's record whose log head lies beyond
  this device's log makes it fetch the log (`AheadOfLog`). When the record's signature verifies
  under the sender's current keys (`AheadOfLog { signed: true }`; the keys valid at that index
  are not known yet, so the current ones are the best check) and the next log fetch still ends
  before that head, the Team view says calmly that the server holds back part of the team
  history (`TeamActivity::history_withheld`); it goes once a fetch reaches the head. It is a
  notice only and stops nothing: a member could sign a head far ahead and raise it on purpose.
- **Admission is confirmed** (third review, H2). A device the log lists but that another device
  admitted shares nothing (no presence, claims, reservations, confirmations, removals) until the
  person compares two numbers with the admitting device and confirms: that device's safety
  number and the log safety number at the head (`TeamStatus::ConfirmAdmission`; the admitting
  device shows both under People, Your devices). The confirmation is stored with the head in the
  keychain (`StoredKeys::admitted`, format `v2`; a `v1` entry counts as confirmed only with a
  head and an epoch secret, since `v1` stored the head after every log fetch, also on a device
  the log never listed; round 2, N4). The device that created the team admits itself. The
  confirmation is bound to what the window showed (round 2, N5): `team_confirm_admission` carries
  the log index and the admitting device's safety number, and the engine refuses them
  (`TeamError::Changed`) when the status shows others by now; the window then reads the new
  numbers. Without an admitting device or with an empty number there is nothing to compare:
  the status stays waiting, and `confirm_admission` refuses an empty number with
  `TeamError::UnknownRequest` (round 4; a listed device always has the entry that added it, so
  this is a guard, not a path seen). Its button stays disabled for a moment after any number changes, like the device
  confirmations, whose cards also start disarmed again when their content changes under the
  same id. Independently, the log's owner (entry 1's issuer) must be the project's `owner_id` on
  the website, else the team stops (`OwnerMismatch`). A website that names no owner (an older
  server) stops a device that was not admitted before with its own reason, `NoWebsiteOwner`
  (round 2, N2);
  devices admitted before go on.
- **A due rotation holds sharing** (third review, M1). While the log says an urgent rotation is
  due and this device cannot rotate, it seals nothing new (`TeamStatus::WaitingForKeyChange`):
  the current epoch key may be in the removed device's hands. Receiving goes on.
- **Not every due rotation is urgent** (round 2, M1). Any member may add and remove devices of
  its own; if each removal forced a rotation at once and held sharing, one member could keep the
  team changing keys and waiting for them for good. `TeamState::rotation_urgent` is true when a
  member left since the epoch began or a device was removed that the log listed when the epoch's
  key was wrapped. A due rotation that is not urgent only removes devices added after the epoch
  began, of members still in the team: such a device got the key, if at all, through `key.wrap`
  from a device of its own member or an owner or admin, so its member had it anyway. For these
  an owner's or admin's device rotates at most every `DEFERRED_ROTATION_EVERY_MS` (10 minutes,
  checked on each ping), and sharing goes on meanwhile. Whether the removed device ever received
  a `key.wrap` is not used: a member may wrap to its own devices (`relay::check_wrap`), and the
  relay's `key.wrapped` events are unsigned. Accepted consequence: a lost device added after the
  epoch began keeps the current key for up to 10 more minutes of sharing while an owner or admin
  is online. Without one the wait is bounded too (round 3, N-1): a device that cannot rotate
  notes when it first saw the deferred rotation, and `DEFERRED_ROTATION_EVERY_MS` later it treats
  it as urgent and seals nothing new (`KeyChangeWait::Overdue`) until an owner or admin rotates;
  so such a device keeps the key for at most about 10 minutes of sharing, however long the
  managers stay away. Only a removal of one of this device's own member's devices starts that
  wait (round 4): another member's removed device got nothing its member did not already have,
  so one member's churn can no longer stop everybody else's sharing. Removing a member, or a
  device from before the epoch, rotates at once as before.
- **Rejected confirmations are remembered up to a bound** (round 3, info). The state file keeps
  the ids of rejected device and role confirmations, at most 256, oldest dropped first. A relay
  or member that makes more than 256 different requests can bring an old rejected one back into
  view; it still needs a person to confirm it, so the bound stays.
- **Rotation retries are bounded** (round 2, N3). A refused `key.rotate` counts per connection;
  after two refusals the device does not rotate again until it reconnects. A refusal other than
  `conflict` or `staleEpoch` (another rotation came first, which the log decides at once) waits
  for the next ping, and an acknowledged rotation the next log fetch does not show counts as
  refused. A relay that refuses every rotation therefore gets at most two per connection.
- **The relay names the member only once it is the signed-in account** (third review, L5): the
  first `welcome` binds the device to its member id only when that id is the account `/me`
  names; otherwise the device stops with `OtherAccount` and binds nothing. While `/me` is not
  known yet, an unbound device does not connect, and a `welcome` that still reaches it ends the
  connection without binding (`SessionEnd::Lost`); it connects again with the reconnect backoff
  once the account read `/me` (round 2, N1: before, an unknown account bound to whatever the
  relay named).
- **Recovery code**: 128 random bits plus version and a 24-bit checksum, Crockford base32 in
  eight groups of four; HKDF derives an offline device's Ed25519 seed and X25519 key. It is a
  device of the log (`recovery: true`), so every rotation wraps to it. Using it means signing in
  as that device, adding the new installation to its own member, removing the recovery device
  from the new one, and an owner or admin rotating.
- **No team salt in the recovery derivation** (round 2, N9, accepted). HKDF takes no salt and no
  team id; only the label separates the derivation. Each code is 128 fresh random bits, so a
  salt adds nothing against guessing one code, and against all codes at once (multi-target) even
  2^30 published recovery keys leave about 2^98 guesses, out of reach. Binding the team in now
  would give every existing code other keys than the ones its log lists, so printed codes would
  stop working. A later code version (version byte 2) may add the team as domain separation for
  new codes only.

## Connection proof

Protocol version 3 ([[0020-proof-of-possession-at-hello]], module `possession`). The relay
sends a random 32-byte challenge in every `welcome` of version 3; the device answers with
`device.prove {sign, signature}`:

```text
statement = field("hangar-team/v1/connection-proof") || field(team) || field(member)
            || field(device) || field(sign public key) || field(challenge, 32 bytes)
            || field(negotiated protocol version, 2 bytes big endian)
signature = Ed25519 (strict verification) with the device's sign key over statement
```

- **Domain separation:** the label is used by no other signature, so a `device.keys` proof of
  possession, an endorsement, a log entry or a sealed record never verifies as a connection
  proof (tested with both kinds of proof the same key makes over the same team and holder).
- **Binding:** team, member, device and version tie the proof to one `hello`; the challenge,
  new with every `welcome`, makes it worthless on another connection; `sign` is in the
  statement, so the signature cannot be presented under another key.
- **What the relay does with it** (`verify_connection_proof`, `Possession`,
  `counts_as_listed`, `check_restricted`): it keeps the proven key per team and connection and,
  on every intent of a team with a log, lets the device act as the log's device only while the
  log's sign key for that holder is the proven key. Any other device may only register, prove
  itself and read the log and the directory. On versions 1 and 2 (`Possession::Legacy`) a
  listed device counts without a proof (transition rule); an unlisted one is restricted there
  too. The relay's state machine and transition steps: [[relay-v3-server-spec]].
- **It proves possession of the signing key only.** As with the `device.keys` proof, the X25519
  key cannot sign; a connection that proved the signing key but holds another KEM key gains
  nothing, since wrapped keys are opened with the KEM key of the log.
- Test vector: the statement is checked byte for byte against a hand-built layout
  (`possession::tests::the_statement_has_its_documented_layout`). `test_vectors.py` does not
  recompute it yet (Python's standard library has no Ed25519; the layout test covers the bytes
  that are signed).

## Obligations of the app

- Store a rotation's secret in the OS keychain before sending `key.rotate`.
- On a `welcome` of protocol version 3, send `device.prove` for its challenge before anything
  but `device.keys` (the engine sends it right after `device.keys`, before the log fetch).
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
- Follow the website's member list on an owner's or admin's device, watch it on every other
  admitted device, show the admission check before the first share, and seal nothing while a
  rotation is due that this device cannot make, or while a departure or a deferred rotation is
  overdue (decisions above).

## Trust of relay events in the app

The relay's events are not signed; the app trusts each only for what a lying relay could do
anyway (delay, drop, disconnect), never for deleting keys or raising rights:

| Event | Trusted for |
| --- | --- |
| `memberlog.appended`, `key.rotated`, `key.wrapped` | a hint to fetch the log or the key; what counts is the verified log and `unwrap_any` against its commitment |
| `key.rotated` with `wrapped` | kept only for epochs above the verified current one, at most 4, dropped on every `hello` |
| `device.removed`, `member.removed` | a log fetch; keys go only when the verified log removed this device |
| `member.removed` naming this member, close 4404 | a check of the website's project list (authenticated HTTPS); keys go and sharing turns off only when the project is gone there |
| `member.removed`, `member.role_changed` of another member | on an owner's or admin's device a read of the website's member list, which may lead to a remove member or change role entry |
| `member.role_changed` | a reread of the website's project list; the role comes from the website, capped by the log |
| any event asking for a website check | at most one check per 30 s per team connection; a later wish waits for the next ping after that (a flood of unsigned events costs one request) |
| close 4403 | stop with "removed", keys kept (a relay can only disconnect) |
| close 4401, `err forbidden` on `hello` | a fresh ticket; one immediate retry per settled connection |
| `memberlog.entries` with `more` | only with at least one entry (progress), else the connection ends |
| `presence.update` | shown only for a device the verified log lists (one fetch and retry for an unknown one); the status only when it opens with `open_verified` under the current epoch or a newer one and its counter is not below the newest one shown for that device (marks in the state file, kept across `offline`, reconnects and restarts, and for removed devices the log still knows); without a status the sessions stay as last signed; `offline` (unsigned) only hides the device; at most one entry per listed device; a signed fork sets only that device aside |
| `task.reserved`, `task.released`, `reservation.expiring`, `reservation.expired`, `task.release_requested`, `task.release_answered`, `task.release_request_ended` | applied in the relay's order, exactly once (the event log cursor), only for a holder the verified log lists; an event whose holder the log does not list yet waits in order for the next log fetch (at most 1024, else a fresh snapshot; such fetches at most every 5 s, else at the next ping), then it is dropped; at most 32 holds per member and as many as the log lists members; title, area and linked session only from the holder's note, opened with `open_verified` as a reservation note bound to the task's hash and the holder (record id: `record::task_record_id`, the hash's 64 lowercase hex digits), its title must hash to the task under the note's epoch and its name must have no control, invisible or reordering character; a note older than one shown is ignored, and so is the last note of a hold that ended (counter marks per device and note in the state file, at most 64 per device with a floor, never dropping the marks of holds and signed requests that still run, kept across resyncs and restarts; a hold missing from a resync's snapshot counts as ended, and a note that never opened is marked from its unverified header when its hold ends: only under a key period the log gives its sender, at most 10 minutes of counters ahead of this device's clock, never in place of another mark or raising the floor; a note with the counter `u64::MAX` is never shown); a release request gets the holder's card only with a note one of the requester's listed devices signed and not shown before, and the answer must name that requester; notices only when the hold or this device's request was there; the reservations of before a restart come from the relay's snapshot (the first `hello` of an engine sends no cursor) |
| `err conflict` with `holder` on `task.reserve` | shows the attempt as waiting for that holder (and pauses its linked session) only when the log lists it; an unlisted one is checked again after the next log fetch (at most every 5 s, else at the next ping), then the attempt ends with a notice; the pause lasts only while the blocking hold is known |
| `task.released`, `reservation.expired`, handover | accepted risk: not signed by the holder; the relay decides first wins and can end or hand over a hold |
| notices about reservations | one per event the log vouches for (deduplicated warnings), at most 8 kept; desktop notifications at most once a minute |
| `notify` | ignored: the events it stands for reach every device of the person through the log |
| `claim.overlap` | for one of this device's own session keys and a listed device, and only for a session the device's newest signed status lists (without any status: at most "same area", no notice, and no detail is opened or kept); the detail only as for a status (counter mark per device and session key, at most 64 sessions per device, older ones refused by a floor); a signed detail with no file of ours caps the overlap at "same area" whatever the relay's level; at most 64 overlaps per own session and teammate device (one device inventing session keys cannot take the others' room); desktop notices at most once a minute |
| teammates' names, agents, titles (status, detail) | shown and passed on without control, invisible or reordering characters and without line or paragraph separators (U+2028, U+2029), cut at 120 characters |
| the website's texts (project names, people's names, error details) | without control, invisible or reordering characters and without U+2028, U+2029 (`account::clean_text`); project names are also rendered with any remaining invisible characters shown as placeholders (`Revealed`) |

## Threat model

Adds to the relay's view in [[team-protocol]] (threat model section):

- **Cannot** read sealed content or wrapped keys; substitute device keys; enter keys it does not
  hold; inject an epoch secret; store an entry the clients would refuse; separate a rotation
  from its keys; move log entries between teams or positions; move records between teams,
  epochs, kinds, record ids or senders; forge a member's record or log entry; add a device to a
  member without that member's consent, even as owner or admin.
- **Can** withhold new log entries or fork the log per member: detected when a record crosses
  the fork, or by comparing log safety numbers; the sides of a fork cannot read each other's
  records. Withholding a removal ends in a stop of sealing on every device that watches the
  website's list (10 minutes after it first saw the departure); withholding entries a teammate
  signed a head for shows as held back history. It can replay old records (the app's `ReplayGuard` refuses them where order holds),
  and drop or delay anything.
- **Members** keep what they saw after removal; the next rotation locks them out of new content.
  A removed device can still seal records naming an old log head until the next rotation (the
  relay refuses content of an old epoch afterwards). A compromised device has its keys and is out
  of scope.
- **First contact** is no longer silent trust on first use: a newcomer checks the admitting
  device's and the log's safety numbers before it shares, and the log's owner must be the
  website's. A person who confirms without comparing still trusts the relay.

## Tests

71 unit tests (round 2 added the log cap with its reserved places and `rotation_urgent`, round 3
the last place kept for the rotation and the signed flag of `AheadOfLog`, round 4 device removals in a full log up to their half of the reserve): whole-team flows where every entry passes the relay's `admit` (a removal through
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
    is the owner's identity tied to the website account? (Partly answered in the third review:
    the log's owner must be the website's `owner_id`, and the newcomer confirms the admitting
    device's and the log's safety numbers before it shares. The owner's member id is still not
    bound to a key the website vouches for, so a person who confirms without comparing trusts
    the relay.)

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
- **Server requirement** (round 2, M1a): the relay must rate-limit `memberlog.append` and
  `device.remove` per member (for example a handful per minute and a few hundred per day). The
  clients bound the damage (log cap, deferred rotations), but only the relay can stop one member
  from filling the log with add and remove pairs of its own devices up to the cap, which would
  then block new devices for the whole team (server work, outside this repository).
- A full log has no recovery inside the team: only a new project on the website gives room for
  new devices and members (see Log length). A log compaction or an owner-signed restart of the
  log would remove that dead end; neither is designed yet.
- The website must offer `GET /api/v1/projects/{id}/members` (members with their roles, for any
  member of the project, not only owners and admins) for the log to follow removals on the
  website and for member devices to notice a withheld removal; until it does, a member removed
  there keeps receiving new epoch keys (server work, outside this repository).
- **Server requirement** (round 3, N-2): `/members` and `/projects` must answer with the whole
  list in one response. A response with paging markers is refused as incomplete, so paging
  would turn off following the website.
- Ownership changes on the website stop the team (`OwnerMismatch`) until ownership transfer
  exists in the log.
- Ownership transfer is not part of version 1 (L7).
- Rotating a device's own X25519 key (L4).
- No persisted `TeamState`: clients and the relay re-verify the log from entry 1 (fine for
  hundreds of entries).
- Shared transcripts with their own key per session (step G) are not covered.
