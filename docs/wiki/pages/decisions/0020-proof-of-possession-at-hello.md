---
title: "ADR 0020: Proof of possession at hello and blob.delete (team protocol version 3)"
type: decision
status: accepted
superseded_by:
tags: [collaboration, teams, protocol, relay, security, possession, blobs, compatibility]
sources: []
created: 2026-11-25
updated: 2026-11-25
---

# ADR 0020: Proof of possession at hello and blob.delete (team protocol version 3)

## Context
The relay binds a connection to a holder from the ticket's subject and the device id in
`hello`. The device id is the client's word: the app makes it (`team::new_device_id`), and
nothing outside the membership log ties it to a key. So the relay could
not tell whether a connection holds the signing key the membership log lists for that device
([[team-protocol]], threat model). Two consequences the relay documents as residual risks:

1. A device removed with `device.remove`, whose account session on the website still works,
   comes back under a new device id and reads what the relay hands any member (snapshot, claims,
   blobs, the notes key ring) until the next `key.rotate`.
2. A stolen, unused ticket of an owner or admin, used with the id of one of that member's listed
   devices, acts as that device: it can overwrite the notes key ring (version 2) with garbage,
   which makes notes unreadable until an admin writes the ring again.

Separately, blobs could not be deleted: a member could fill the team's storage quota for good,
and decisions and tasks stayed until the team was deleted.

Both need new messages, and released apps must keep working with a new relay and new apps with
an old one. A new field in a server message breaks every released app (server bodies are
`deny_unknown_fields`), so the change needs a protocol version ([[team-protocol]], "why a new
server field breaks released apps").

## Options
1. **Challenge in `welcome`, answered with a signed `device.prove`; the proof carries the key
   and the relay compares it with the log per intent.** One round trip after `welcome`, no
   ordering dependency on `device.keys`, works for a device that is not admitted yet.
2. **Proof inside `hello`.** Needs a challenge before `hello`: a server frame at WebSocket open,
   which every released app would refuse as an unknown kind, or a handshake header. Rejected.
3. **Verify the proof against the keys of `device.keys`.** Ties the proof to the order of two
   frames and to keys a device may still replace before its admission; the log is what counts
   anyway. Rejected in favour of comparing with the log on every intent.
4. **Withhold the event stream from unproven connections.** Logged events left out for one
   connection are gaps in its cursor, and a resync snapshot comes before the `welcome` that
   carries the challenge; it would need a new order of the handshake. Rejected: the sends are
   restricted, the events stay as they are (residual risk below).
5. **Refuse versions 1 and 2 as soon as the relay speaks 3.** Closes the hole at once, but cuts
   off every released app on the day the relay updates. Rejected for a transition rule.

## Decision
Option 1, as protocol version 3 (`PROTOCOL_VERSION` 3, `ProtoRange::CURRENT` `{1, 3}`):

- **Challenge:** `welcome.challenge`, 32 random bytes, present exactly when the negotiated
  version is 3 or newer and left out of the text otherwise, so a version 2 connection gets the
  welcome of today.
- **Proof:** `device.prove {sign, signature}`, the Ed25519 signature over
  `field("hangar-team/v1/connection-proof") || field(team) || field(member) || field(device) ||
  field(sign) || field(challenge) || field(version as 2 bytes)` ([[team-crypto]], "Connection
  proof"). The label is used by no other signature; team, holder and version bind it to one
  `hello`; the fresh challenge makes it worthless on another connection.
- **Restriction:** in a team with a log, a device counts as the log's device only when the log
  lists it and, on version 3, the key the connection proved is the log's sign key for it now.
  A device that does not count may send only `hello`, `ticket.renew`, `device.keys`,
  `device.prove`, `devices.get`, `memberlog.get` and `ping`
  (`ClientMessage::allowed_before_proof`, `possession::check_restricted`); everything else is
  `forbidden`. A team without a log restricts nothing.
- **Transition rule:** a connection of version 1 or 2 cannot prove; a device the log lists
  counts there as before. The rule for devices the log does not list applies on every version,
  because released apps send nothing else before their admission. The relay narrows the legacy
  case per holder (once a holder proved itself on version 3, its `hello` of an older version is
  `outdatedClient`) and for everyone with `REQUIRE_DEVICE_PROOF`.
- **App:** after every `welcome` of version 3 the device sends `device.keys`, then
  `device.prove`, then the log fetch, so nothing it sends later meets an unproven connection
  (the relay checks one connection's frames in arrival order). A refused proof ends the
  connection once the pending log answer is read, so a removed device served only the log up
  to its removal still learns that it was removed; the reconnect brings a new challenge.
- **Per connection, not per holder:** challenges, proofs, targeted unlogged messages and the
  presence and reservation side effects of joining and leaving belong to connections that
  count, so a second connection with a stolen ticket and a listed device id inherits nothing.
- **Departing devices:** the relay's `welcome` for a removed device in its grace period carries
  a challenge too, and `device.keys` and `device.prove` there are acknowledged without effect.
- **`blob.delete {blob}`** (version 3, deduplicated): the author (the member that stored it, from
  any device) or an owner or admin by the lower of log and ticket role
  (`relay::check_blob_delete`); `ack`, `notFound`, `forbidden`; the relay frees the size in the
  team's and the member's quota and keeps nothing of the blob. No event. The app stores no blob
  today, so it deletes none; the first feature that writes blobs (team decisions, ADR 0005
  amendment) deletes a decision when it is superseded or withdrawn.

The server's duties are specified in [[relay-v3-server-spec]].

## Consequences
- Both crates change; the relay needs a vendor update, the new rules, one migration (the proof table) and a new
  image. Until then a new app talks version 2 to the old relay and behaves as before.
- New apps on a new relay: a blocked device under a new id can only register and read the log
  and the device directory, on every version. A stolen ticket with a listed device id cannot act
  as that device on version 3: no intent beyond registering and reading, no targeted message
  (`claim.overlap`, `key.wrapped`, `notify` go only to connections that count), no presence or
  reservation side effect of its joining or leaving ([[relay-v3-server-spec]], section 4). It
  still receives the logged events and reads the log, like any connection with a valid ticket.
  Once that holder proved itself, the relay refuses its version 2 `hello` and closes its open
  version 2 connections.
- Residual risks:
  - a connection that does not count still receives the logged event stream (hashes, sealed
    small notes and statuses readable only with an epoch key it may still have); `key.rotate`
    after a removal stays the cure;
  - until `REQUIRE_DEVICE_PROOF`, a stolen ticket with the id of a listed device that never ran
    a version 3 app still acts as that device on version 2;
  - the challenge is not bound to the relay's identity. That is not needed: the app keeps one
    device identity and one device id per relay base and project (`team::team_key(base,
    project)` names the keychain entries), so a foreign relay that forwards a challenge of the
    real one to the app gets a proof made with the identity the app keeps for the foreign
    relay's base, whose key and device id the real relay's log does not list; it gains
    nothing. What remains is a TLS-terminating proxy in front
    of the same base, which sees and can change every frame anyway, so binding the relay's
    identity into the statement would not stop it. The decision not to bind it stays.
- Decisions draft of ADR 0005: its messages now need protocol version 4 (version 2 went to the
  notes key ring, 3 to this change); `blob.delete` exists already.

## Evidence
- Crates: `crates/hangar-team-protocol/src/wire.rs` (`Welcome::challenge`, `DeviceProve`,
  `BlobDelete`, `allowed_before_proof`, the welcome rule in `ServerMessage::check_rules`) and
  `src/e2e.rs` (`Challenge`); `crates/hangar-team-crypto/src/possession.rs` and
  `src/relay.rs` (`check_blob_delete`). Tests: encode and decode, the exact version 2 welcome
  text, replay against another challenge, misbinding (team, member, device, version, key),
  domain separation against `device.keys` proofs and endorsements, the statement byte for byte,
  the restriction table, the blob rights.
- App: `src-tauri/src/team_engine.rs` (`Op::Prove`), unit tests against the in-memory relay on
  versions 1, 2 and 3 with the restriction applied (the whole engine test set also ran once
  against a version 3 relay; the only refusals were a removed device's frames after its removal
  and the refused-proof test), and `src-tauri/tests/team.rs`, where `TeamService` runs over real
  WebSockets against a relay negotiating `{1, 3}` and one test against a relay of version 2.

> [!question] Unverified
> The relay side is a specification, not run yet. Released apps older than the current engine
> were not each checked for what they send before their admission.
