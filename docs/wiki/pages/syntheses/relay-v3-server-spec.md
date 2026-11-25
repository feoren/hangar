---
title: "Relay protocol version 3: server specification (proof of possession, blob.delete)"
type: synthesis
tags: [collaboration, teams, protocol, relay, security, possession, blobs, quota, rollout]
sources: []
created: 2026-11-25
updated: 2026-11-25
---

# Relay protocol version 3: server specification

What the team relay (`hangar-server`) must do for protocol version 3. The app side and both
crates are built; the design and its reasons are in [[0020-proof-of-possession-at-hello]] and
[[team-protocol]] ("Protocol version 3"), the statement and its signature in [[team-crypto]]
("Connection proof"). Every rule below is implementable with the vendored crates alone; no rule
needs the relay to hold a key.

Two of the relay's open protocol wishes are answered:

- **Proof of possession at `hello`:** a connection shows that it holds the signing key the
  membership log lists for the device it named. Until then a listed device counts as unknown,
  and an unknown device in a team with a log may only register, prove itself and read.
- **`blob.delete {blob}`:** the author or an owner or admin deletes a blob, which frees the
  quota.

## 1. Version negotiation

| Item | Rule |
| --- | --- |
| Relay range | `ProtoRange::CURRENT`, `{1, 3}` after the vendor update. With `REQUIRE_DEVICE_PROOF=true` (new setting, default `false`) the relay offers `{3, 3}`. |
| Per `hello` | `hello.proto.negotiate(relay range)` as today. No common version: `outdatedClient` when the client's newest is older than the relay's oldest, `badRequest` otherwise. |
| Kinds | Unchanged rule: a client kind with `since()` above the team's negotiated version is `badRequest` (`device.prove`, `blob.delete` on versions 1 and 2). No server kind is new in version 3. |
| `welcome` | Carries `challenge` exactly when the negotiated version is 3 or newer. The crate refuses to encode a welcome that breaks this (`EncodeError::Invalid`), and leaves the field out of the text when it is `None`, so a version 2 connection gets byte for byte the welcome of today. |

## 2. Issuing the challenge

- On every accepted `hello` that negotiates 3 or newer, draw 32 bytes from the operating
  system's secure random number generator (`getrandom`, or `OsRng`; never a seeded or
  per-process generator) and send them as `Welcome::challenge`
  (`hangar_team_protocol::e2e::Challenge::from_bytes`).
- One challenge per (connection, team). The relay refuses a second `hello` for the same team on
  the same connection with `badRequest`, as it always has, so a challenge never changes within
  a subscription.
- The `welcome` that ends a resync snapshot is the one that carries it (there is one `welcome`
  per `hello`).
- **The departing `welcome` too.** The relay builds its own `welcome` for a removed device in
  its grace period (the `Departing` path of `join`). At version 3 or newer it carries a fresh
  challenge like every other one (the crate refuses to encode it without). On a departing
  connection `device.keys` and `device.prove` are answered with `ack` and have no effect (no
  key is registered, nothing is verified or recorded), never with `err`: the app sends both
  before its log fetch, and the departing device must still read the log up to the entry that
  removed it to learn that it was removed. Everything else stays as today (`memberlog.get` up
  to the removal, `forbidden` for the rest, close 4403 after the last page). The app reads its
  pending log answer even after a refused proof, but a relay must not rely on that.
- Keep the challenge in the connection's memory only. Never store it, never log it. It is not
  secret, but nothing needs it after the connection ends.

## 3. Verifying `device.prove`

`device.prove {sign, signature}`, version 3, not deduplicated (do not put it into `RecentOps`).

1. The team's `hello` must have been accepted on this connection (`badRequest` otherwise, as for
   every team intent), and the negotiated version must be 3 or newer (`badRequest` by `since()`).
2. `hangar_team_crypto::possession::verify_connection_proof(team, holder, proto, challenge,
   &prove)` with the frame's team, the connection's holder (ticket `sub` plus `hello.device`),
   the negotiated version and the connection's current challenge for the team.
3. `Ok(key)`: keep `key` as the connection's proven signing key for the team (state `Proven`,
   section 4), answer `ack`. Record the proof durably for downgrade protection (section 6) only
   when `counts_as_listed(state, holder, Possession::Proven(&key))` holds, that is when the log
   lists the holder with exactly this key: a proof with a key the log does not list writes no
   row. A device that proves before its admission gets its row at the first intent that passes
   as `Proven` afterwards, or when the entry that admits it with that key is stored, whichever
   comes first. Writing the row closes the holder's `Legacy` connections in the team (section 6).
4. `Err(e)`: answer `err` with `e.code()` (`forbidden`). The state stays as it was; a failed
   proof never removes a key proven before on this connection. Count it as a bad frame
   (`MAX_BAD_FRAMES`), so a connection cannot try signatures without bound.
5. Log only the outcome (proved or refused), never the holder, the team, the signature or the
   challenge.

The relay does not compare `sign` with anything here. It compares the proven key with the log
on every intent instead (section 4), so a proof made before the device was admitted counts from
the moment the log lists the device with that key, and a key the log no longer lists stops
counting at once.

## 4. The restriction state machine

Per (connection, team), in memory:

```text
                hello negotiates 1 or 2
   (none) ------------------------------------> Legacy
     |
     | hello negotiates 3+ (new challenge)
     v
   Unproven --device.prove passes (key K)--> Proven(K) --device.prove passes (key K')--> Proven(K')
     ^                                          |
     +----------- hello again for the team -----+         device.prove refused: no change
```

Before every intent of the team, **first**, after decoding and the `since()` check and before
the op id lookup, the epoch check and the intent's own checks:

```rust
let possession = Possession::of_connection(negotiated, proven_key.as_ref());
check_restricted(verifier.state(), &holder, possession, &message)
    .map_err(|e| e.code())?; // forbidden
```

| Team | Connection | Device in the verified log | May send |
| --- | --- | --- | --- |
| no log entry yet | any | n/a | everything, as today (the owner's first device creates the log) |
| with a log | `Proven(K)`, K equal to the log's sign key for the holder | yes | everything |
| with a log | `Unproven`, or `Proven(K)` with another key | yes | only `allowed_before_proof` |
| with a log | `Legacy` | yes | everything (transition rule, section 6) |
| with a log | any | no | only `allowed_before_proof` |

`ClientMessage::allowed_before_proof`: `hello`, `ticket.renew`, `device.keys`, `device.prove`,
`devices.get`, `memberlog.get`, `ping`. Everything else is `err forbidden`.

- **Order.** The frames of one connection and team are checked and applied in arrival order,
  one after the other: a `device.prove` takes effect (state `Proven`) before the next frame of
  that connection is checked. The app relies on it: it sends `device.keys`, `device.prove` and
  `memberlog.get` without waiting, and everything after them must meet the proven state.
- **Evaluate per intent, never cache "known".** The log changes under a live connection: an
  admission makes a proven pending device count, a removal or a new key for the holder makes it
  stop counting. `counts_as_listed` reads the current `TeamState`.
- **A refused intent is not remembered by op id.** The app retries after it proved; the retry
  must be applied, not answered from `RecentOps` with the old `forbidden`.
- **The rule for unlisted devices applies on every version**, also `Legacy`. Released apps
  (protocol 1 and 2) send nothing else before the log lists them: after `welcome` they send
  `device.keys` and `memberlog.get`, and `devices.get`, `key.get`, presence, claims, tasks and
  notes only once the log lists the device (checked in the app's engine; its test relay applies
  this rule on versions 1 and 2 and the app's tests run against it, see section 9). A device
  that left the log (removed) still sending is refused, which is right.
- **Logged events do not change.** Snapshot, replay and live logged events go out as today,
  also to a connection that does not count. Withholding logged events from one connection would
  leave gaps in its cursor. What it can still read there are hashes and the small sealed notes
  and statuses in events, readable only with an epoch key it may still have; `key.rotated`
  carries a package wrapped to the listed device's KEM key, which an impostor cannot open.
- **Targeted, unlogged messages go only to connections that count.** The relay delivers by
  holder or member, and a second connection with a stolen ticket and a listed device id has the
  same holder as the real one. So every message that is not logged and goes to chosen
  connections goes only to the connections of that holder or member for which
  `counts_as_listed` holds at sending time:
  - `claim.overlap` (`deliver_overlaps`, by holder; it carries the other side's sealed detail),
  - `key.wrapped` (by holder),
  - `notify` (to every connection of the member),
  - and the same for every targeted kind added later.

  A connection that starts to count later gets nothing replayed: `key.get` after its proof
  covers `key.wrapped`, a fresh `claim.set` of its own covers overlaps, and `notify` is only a
  hint. Events no connection of a holder counts for are dropped, not queued (the undelivered
  `notify` per member stays kept as today until a counting connection takes it).
- **Join and leave side effects only from connections that count.** Joining and leaving
  change shared state: presence (`presence.update` to the others, online and offline), the
  reservation book's online counting (`book.presence`, which stops and starts the holder's
  deadlines) and the claim board on leave. On version 3 these run when the connection starts to
  count (its proof passes while the log lists the key, or the admitting entry is stored for a
  proven connection) and on leave only if it counted; a connection that never counted joins and
  leaves without any of them. On versions 1 and 2 a listed device counts from `hello` (as
  today), an unlisted one never. Otherwise an unproven twin could keep a holder's reservations
  from running out, or report it offline by closing.
- **Rights after the rule are unchanged.** The rule only decides whether the device counts as
  the log's device; the intent's own checks (roles, holds, epochs) follow as today.

## 5. `blob.delete`

`blob.delete {blob}`, version 3, deduplicated by op id (the first answer is kept, as for
`blob.put`), no epoch, answered with `ack` without fields (`Ack { blob: None, content: None,
keys: None }`).

1. `check_restricted` (section 4) first: in a team with a log the sender must count as a device
   of the log.
2. Look the blob up by team and id: `SELECT member, size FROM relay_blobs WHERE team_id = ? AND
   id = ?`. None: `err notFound` (also for a blob deleted before, and for an id of another team,
   so nothing is told about other teams).
3. `hangar_team_crypto::relay::check_blob_delete(verifier.state(), ticket_role, &holder,
   &author)` with `author` the row's `member`: in a team with a log the sending device must be
   listed (defense in depth behind section 4); the author then deletes from any of its devices;
   anyone else needs owner or admin by the lower of the log role and the ticket role (without a
   log, the ticket role). `Err`: `err forbidden`.
4. In one write transaction (`begin_write`, as `put_blob`):
   `DELETE FROM relay_blobs WHERE team_id = ? AND id = ?`, and when it deleted a row,
   `UPDATE relay_teams SET blob_bytes = MAX(blob_bytes - ?, 0) WHERE id = ?` with that row's
   `size`. Zero rows deleted (a concurrent delete won): `notFound`.
5. No event. Nothing lists blobs yet, so no device holds an id it must forget; a later listing
   feature (`blob.list`, `blob.stored`, `blob.deleted`, ADR 0005 amendment) brings its own event
   and version.

**Quota accounting.** Rights and quota need only the blob's `size` and its author `member`; a
deleted blob leaves nothing behind (no tombstone). The team quota stays the running
`relay_teams.blob_bytes`, changed in the same transaction as every insert and delete (put,
delete, the transcript purge). The per member quota over all teams stays
`SUM(size) WHERE member = ?`. Its index exists already (`relay_blobs_member`, server migration
`0014_blobs_member_index.sql`); `blob.delete` needs no new one, since it deletes by the primary
key `id`. Check it with `EXPLAIN QUERY PLAN SELECT COALESCE(SUM(size), 0) FROM relay_blobs WHERE
member = ?` (expected: `SEARCH relay_blobs USING INDEX relay_blobs_member`). The `device` column is not
needed for rights or quota; it may stay as metadata (security.md lists the author) or go in a
later migration.

## 6. Transition rule and downgrade protection

A connection of version 1 or 2 cannot prove anything. Refusing it would cut off every released
app at once, so the relay serves it with the `Legacy` row of section 4, and narrows that in two
steps:

1. **Per device, automatic.** New table, written when a connection of the holder counts as the
   log's device with a proven key (section 3, step 3; never for a key the log does not list):

   The server's migrations run through `sqlx::migrate!` and need a version number no other
   migration has: take the next free one (`0016` when this was written, after
   `0015_license_floors.sql`; `0012` and `0013` are taken).

   ```sql
   -- migrations/0016_relay_device_proofs.sql
   -- A holder that proved possession once on protocol version 3 is never served on an older
   -- version again: a stolen ticket with its device id cannot fall back to the unproven rules.
   CREATE TABLE IF NOT EXISTS relay_device_proofs (
       team_id    TEXT    NOT NULL REFERENCES relay_teams (id) ON DELETE CASCADE,
       member     TEXT    NOT NULL,
       device     TEXT    NOT NULL,
       proven_at  INTEGER NOT NULL,
       PRIMARY KEY (team_id, member, device)
   ) STRICT;
   ```

   `ON DELETE CASCADE`: the rows go with the team. A `hello` whose negotiated version is below 3
   from a holder with a row is answered `err outdatedClient` (the released app stops and asks
   for an update, `AttentionReason::NeedsUpdate`); it does not count as a bad frame. Delete the
   holder's row when the device leaves the log (`device.remove`, `Admitted::removed` of a remove
   member entry), so a holder admitted again under the same ids starts fresh. (Device ids are
   made by the app, `team::new_device_id`, random per installation and server; the relay only
   sees them.)

   **When the row is written, close the holder's `Legacy` connections in the team** with
   `close_where(CLOSE_ROLE_CHANGED, ..)` (4401, an existing code: the app fetches a new ticket and
   reconnects). Their next `hello` below version 3 then gets `outdatedClient`. Without the close,
   a version 2 twin opened before the row would keep acting until the connection's maximum age.
2. **For everyone, by the operator.** `REQUIRE_DEVICE_PROOF=true` makes the relay range `{3, 3}`:
   every `hello` without version 3 is `outdatedClient`. The setting is read at the start, where
   no connection survives; a relay that offers switching it at run time closes every `Legacy`
   connection with `close_where(CLOSE_ROLE_CHANGED, ..)` the same way. Turn it on once the app
   release with protocol version 3 has been out long enough (the official service decides;
   self-hosters choose). Document the setting in `.env.example`, `docs/operations.md` and the
   README.

What stays open in the `Legacy` row until step 2: a stolen, unused ticket together with the id
of a listed device that never ran a version 3 app still acts as that device on version 2, as
today. On version 3, a stolen ticket with a listed device id cannot act as that device (no
intent beyond registering and reading), receives no targeted message and causes no presence or
book change; it still reads the logged events and the log, like any connection with a valid
ticket. The rule for unlisted devices (a blocked device under a new id) holds for every version
at once.

**Recovery devices.** A connection that acts as a recovery device of the log (an app restoring
from the recovery code, should it ever connect as that holder) proves with the recovery
identity derived from the code: the log lists that identity's sign key for the holder, and no
other key counts.

## 7. Compatibility matrix

| | Relay of version 2 (today) | Relay of version 3 (this spec) |
| --- | --- | --- |
| **App of version 2 (released)** | as today | negotiates 2, no challenge, served as `Legacy`; restricted only while the log does not list the device, which matches what it sends; refused with `outdatedClient` once its holder proved itself on version 3, or with `REQUIRE_DEVICE_PROOF` |
| **App of version 3 (this change)** | negotiates 2: no challenge, no `device.prove`, no `blob.delete`; everything else as today | negotiates 3: proves after every `welcome`, before the log fetch; restricted until the proof passes and the log lists its key |

An app of version 1 behaves like one of version 2 in this table (it negotiates 1).

## 8. Errors, logs, limits

- Codes: `forbidden` (restricted, refused proof, `blob.delete` by someone else),
  `badRequest` (a version 3 kind on an older connection, `device.prove` without an accepted
  `hello`), `notFound` (`blob.delete` of an unknown blob), `outdatedClient` (downgrade, required
  proof). No new code.
- Close codes: none new. The relay closes nothing for a missing proof; an unproven connection
  simply cannot act.
- Rate: `device.prove` and `blob.delete` count against the per connection rate limit like
  every frame; refused proofs also count as bad frames.
- Logs: outcome, holder, team and kind only; never the challenge, the signature or a blob id
  together with its content.

## 9. Tests the server needs

- Negotiation: a client `{1, 2}` gets a welcome without `challenge` (compare the text), a client
  `{1, 3}` one with a 43 character challenge, a new one per `hello`.
- `device.prove`: passes with `DeviceIdentity::prove_connection`; refused for another
  challenge (a second connection), team, member, device id, version or key; a refused proof
  keeps an earlier proven key.
- Restriction: a listed device on version 3 without a proof gets `forbidden` for
  `presence.set`, `key.get`, `blob.get`, `notes.keys.get`, `claim.set`, `task.reserve`,
  `blob.delete` and `ack` for `device.keys`, `devices.get`, `memberlog.get`, `ticket.renew`;
  after the proof everything passes; a refused intent's op id is applied after the proof; a
  pending device that proved before its admission counts from the admitting entry on; a device
  removed from the log stops counting on its live connection.
- Order: `device.keys`, `device.prove` and `presence.set` sent back to back without waiting:
  the presence is applied (the proof took effect before it was checked).
- Departing: a device removed while offline reconnects on version 3; its `welcome` carries a
  challenge, `device.keys` and `device.prove` get `ack` (no key registered, no proof row), and
  it still reads the log up to the entry that removed it, then close 4403.
- Twin connection: a second connection of a proven holder (same ticket subject and device id)
  that does not prove, or proves another key: its intents beyond the allowed ones get
  `forbidden`; it receives no `claim.overlap`, `key.wrapped` or `notify` meant for the holder;
  its `hello` and its close send no `presence.update` and change no hold's deadline; the proven
  connection keeps its state.
- Proof rows: a passing proof with a key the log does not list writes no row; a pending
  device's row appears with its first passing intent after the admitting entry (or with that
  entry); writing the row closes the holder's open version 2 connections with 4401, and their
  next version 2 `hello` gets `outdatedClient`.
- Version 2 connections: a listed device acts as today; an unlisted one gets `forbidden` except
  for the allowed kinds; a holder with a proof row gets `outdatedClient` at `hello`; with
  `REQUIRE_DEVICE_PROOF` every version 2 `hello` does, and switching it on closes open version 2
  connections with 4401.
- `blob.delete`: author from another of its listed devices, owner, admin pass; a member who is
  not the author, the author on a device the log does not list, an admin downgraded by the
  ticket, a member with an admin ticket, and an unlisted member with an admin ticket get
  `forbidden`; unknown, deleted and other team ids get `notFound`; the team's `blob_bytes` and the
  member sum drop by the size; a retry with the same op id gets the first `ack`; two concurrent
  deletes leave `blob_bytes` right.
- The new migration applies on a database of the previous release and keeps its rows;
  `EXPLAIN QUERY PLAN` shows `relay_blobs_member` for the member sum.
- The app's integration tests already cover the client side: `src-tauri/tests/team.rs` runs
  the app over real WebSockets against a small relay that negotiates `{1, 3}`, issues
  challenges, verifies proofs and applies `check_restricted` (and one test plays a relay of
  version 2); `relay_interop.rs` can then be run against the new image.

## 10. Rollout order

1. **App repository (done in this change):** both crates and the app's engine speak version 3;
   the app works with relays of version 2 and 3. It ships with the next app release.
2. **Server, in either order relative to the app release** (the matrix holds both ways):
   1. Vendor the crates (section 11).
   2. Implement sections 1 to 6 with the migrations, the tests of section 9, and update
      `docs/protocol.md`, `docs/security.md` (the two "proof of possession" and the "no way to
      delete a blob" residual risks change to what section 6 leaves open) and
      `docs/protocol-wishes.md` (wishes 6 and 7 done), and `docs/development.md` (the migration
      list in "Layout", the crate state "protocol version 3").
   3. Build and publish a new image; the owner deploys it.
3. **Later:** `REQUIRE_DEVICE_PROOF=true` on the official service, once the version 3 app has
   been out long enough.

The website needs no change.

## 11. Vendoring

As `docs/development.md` ("Updating the crates (by hand)") in the server repository describes:

1. In the app repository, after this change is committed on `main`:
   `git -C ../hangar log -1 --format=%H -- crates/hangar-team-protocol crates/hangar-team-crypto`.
2. Replace both copies, without `Cargo.lock`:
   `rm -rf vendor/hangar-team-protocol vendor/hangar-team-crypto`, `git -C ../hangar archive
   <hash> crates/hangar-team-protocol crates/hangar-team-crypto | tar -x -C vendor
   --strip-components=1`, `rm -f vendor/*/Cargo.lock`.
3. Enter the hash and the date in both `vendor/*/ORIGIN.md` ("protocol version 3, proof of
   possession and blob.delete").
4. `cargo update -p hangar-team-protocol -p hangar-team-crypto`, then `cargo fmt --check`,
   `cargo clippy --all-targets -- -D warnings`, `cargo test`. Expected compile breaks: every
   `Welcome { .. }` literal needs `challenge`, and every exhaustive `match` over
   `ClientMessage` needs `DeviceProve` and `BlobDelete`.
5. `diff -r` against a fresh `git archive` of the same hash.
6. Read the protocol crate's README changelog ("Version 3") and the crypto crate's
   ("protocol version 3").

## Open

> [!question] Unverified
> Nothing here ran against a real relay yet; the server side is a specification. That released
> apps older than 0.16.0 send nothing but `device.keys`, `memberlog.get`, `ticket.renew` and
> `ping` while the log does not list them is checked in the current engine only, not in every
> released build.
