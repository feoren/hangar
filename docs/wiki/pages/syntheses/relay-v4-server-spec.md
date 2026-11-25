---
title: "Relay protocol version 4: server specification (blob.list, blob.stored, blob.deleted)"
type: synthesis
tags: [collaboration, teams, protocol, relay, blobs, decisions, quota, rate-limit, rollout]
sources: []
created: 2026-11-25
updated: 2026-11-25
---

# Relay protocol version 4: server specification

What the team relay (`hangar-server`) must do for protocol version 4: the listing of blobs and
its two live hints, which carry the team's decisions. The app side and both crates are built;
the design and its reasons are in [[0021-team-decisions]], the messages in [[team-protocol]]
("Protocol version 4"), the seal in [[team-crypto]] ("Decision seal"). Every rule is
implementable with the vendored crates and the existing `relay_blobs` table; the relay keeps
metadata only and never opens a decision.

## 1. Version negotiation

| Item | Rule |
| --- | --- |
| Relay range | `ProtoRange::CURRENT`, `{1, 4}` after the vendor update. With `REQUIRE_DEVICE_PROOF=true` the relay offers `{3, 4}` (the proof rule of version 3 holds for 4). |
| Per `hello` | `hello.proto.negotiate(relay range)` as today. |
| Client kinds | `blob.list` has `since()` 4: on versions 1 to 3 it is `badRequest`, as for every newer kind. |
| Server kinds | `blobs`, `blob.stored`, `blob.deleted` have `since()` 4: never sent to a connection that negotiated 1 to 3 (an app of version 3 ends the connection on an unknown kind). |
| `welcome` | Unchanged from version 3 (carries the challenge). |
| Restriction | `blob.list` is not in `allowed_before_proof`: `check_restricted` first, as for every intent (a device that does not count as the log's device gets `forbidden`). |

## 2. `blob.list {kind, after}`

A read: answered every time, not put into `RecentOps`, no epoch.

1. `check_restricted` (version 3 spec, section 4). Only `kind = decision` is listed for now:
   `blob.list` of `task` or `transcript` is `badRequest` (nothing lists them, and their ids are
   not the team's business yet).
2. **Rate.** A connection starts at most one listing per `BLOB_LIST_EVERY_MS` (5000 ms) per team.
   A `blob.list` is the **continuation** of the connection's previous listing, and does not count,
   when its `kind` is the same and its `after` equals the last `blob` of the page this connection
   was sent last, and that page said `more: true`. Any other `blob.list` starts a listing; within
   5 s of the last start it is answered `err rateLimited` with `retryAfterMs` (the rest of the 5 s)
   and counts against the frame rate like every frame, not as a bad frame. Keep the last start
   time and the continuation marker (kind, last blob id) in the connection's memory per team.
3. **Cursor.** `after` null: from the first blob. Otherwise look the blob up in the team with the
   kind: `SELECT rowid FROM relay_blobs WHERE team_id = ? AND kind = ? AND id = ?`. None:
   `err notFound` (also for a blob deleted meanwhile and one of another team or kind; the app
   then lists from the first).
4. **Page.** `SELECT id, member, device, created_at FROM relay_blobs WHERE team_id = ? AND kind = ?
   AND rowid > ? ORDER BY rowid LIMIT 101`; the first 100 (`MAX_BLOB_LIST`) go out, `more` is
   whether a 101st came. Storage order (rowid) is the order the app shows, newest last; it never
   changes for a stored row. Never `data`.
5. Answer `blobs {kind, blobs: [{blob, by: {member, device}, createdAt}], more}` with the op id.
   `BlobListing` refuses to encode more than 100 entries. One page of 100 is far below the frame
   limit (each entry under 400 bytes even with 128-byte ids).

**Index.** The page reads by team, kind and rowid. The existing index
`relay_blobs_team_kind_created (team_id, kind, created_at)` serves the filter but orders by
`created_at`; add `CREATE INDEX IF NOT EXISTS relay_blobs_team_kind_row ON relay_blobs (team_id,
kind)` only if `EXPLAIN QUERY PLAN` of the page query shows a scan (an index on `(team_id, kind)`
carries the rowid, so SQLite walks it in rowid order). Take the next free migration number.

## 3. `blob.put` of decisions

Unchanged for `task` and `transcript`. For `kind = decision`:

- **Version:** a `blob.put` of a decision on a connection that negotiated 1 to 3 is
  `badRequest`. No released app writes decisions, and the count, size and hints below exist only
  from version 4 on, so none can be bypassed through an older connection.
- **Size:** content longer than `MAX_DECISION_CIPHERTEXT_LEN` (24 KiB of base64url) is
  `err limit` before anything is stored; a sealed decision of the largest document fits well
  below it. (Clients treat a larger one as unreadable and never decode it.)
- In the same write transaction as today's quota check (`put_blob`):
  - **Count:** `SELECT COUNT(*) FROM relay_blobs WHERE team_id = ? AND kind = 'decision'`; at
    `MAX_DECISIONS` (200) or more: `err limit` (the app says "Your team has as many decisions as
    it can keep. Withdraw an old one first.").
  - **Per member:** `SELECT COUNT(*) FROM relay_blobs WHERE team_id = ? AND kind = 'decision'
    AND member = ?`; at `MAX_DECISIONS_PER_MEMBER` (50) or more: `err limit`, so one member
    cannot take the whole team's room. Owners and admins can still withdraw anyone's.
  - The team's byte quota (`BLOB_QUOTA_BYTES`, 64 MiB by default) and the member sum apply as
    today and also answer `limit`.
- Withdrawals are decision blobs too (a signed record that hides a decision, stored before the
  decision is deleted): they count, and their writer, the owner or an admin deletes them 30 days
  after the decision went. The relay cannot tell them apart and needs not.
- **At the per member limit the app makes room itself:** before it sends a decision that would
  be the member's 51st blob, it deletes the member's own oldest withdrawals of decisions that are
  gone, on the same connection and before the `blob.put` (the relay handles one connection's
  frames in order, so the count has room when the put arrives); after a `limit` it does so once
  and sends the same record again. The relay needs no special rule for this, only the order of
  section 4 of the version 3 spec and the count in the same transaction as the insert.
- The epoch check stays: the envelope's epoch must be the team's current one (`staleEpoch`). It
  names the log head the signature is checked against; the key is the team's notes key, which
  the relay never sees.
- On success, after the commit: `blob.stored {kind, blob, by}` (section 4).

## 4. The hints `blob.stored` and `blob.deleted`

- `blob.stored {kind, blob, by}` after every stored blob of the kind `decision`;
  `blob.deleted {kind, blob}` after every `blob.delete` of a decision that removed a row (not for
  `notFound`). Not for `task` or `transcript` blobs, which nothing lists yet.
- **To whom:** every connection of the team that negotiated version 4 or newer **and counts as a
  device of the log** at sending time (`counts_as_listed`, version 3 spec section 4), the sender's
  own connections included. Not to connections of versions 1 to 3, not to unproven or unlisted
  ones: the hints name blob ids and writers, which only a device that may list should learn.
- **Never logged**: no `seq`, no replay, not kept for offline devices, not queued. A device that
  was offline lists from the first after its next `welcome`, which shows what was stored and
  drops what was deleted. Logging them would put a second kind of state into the event log.
- Not targeted (`recipient()` is `None`): they are team hints, sent like `notes.keys.updated`.

## 5. Errors, logs, limits

- Codes: `badRequest` (`blob.list` on versions 1 to 3 or of another kind than `decision`, a
  decision `blob.put` below version 4, a malformed body), `forbidden` (`check_restricted`),
  `notFound` (`after` unknown), `rateLimited` with `retryAfterMs` (a new listing within 5 s),
  `limit` (`blob.put` of a decision at 200 for the team or 50 for the member, larger than
  `MAX_DECISION_CIPHERTEXT_LEN`, or over a quota). No new code, no new close code.
- Rate: every `blob.list` counts against the per connection frame rate (`BURST` 120,
  `PER_SECOND` 30) as today; a refused listing is not a bad frame.
- Logs: team, holder, kind, page size and outcome only; never a blob id together with its
  content, never `data`.

## 6. Compatibility matrix

| | Relay of version 3 (today) | Relay of version 4 (this spec) |
| --- | --- | --- |
| **App of version 3 (0.16.0)** | as today | negotiates 3: `blob.list` would be `badRequest`, but the app never sends it; it gets no hint; decision blobs other apps store are invisible to it |
| **App of version 4** | negotiates 3: sends no `blob.list`, makes no notes key ring for decisions, shows no Decisions card (owners and admins see "Decisions need a newer team server") | negotiates 4: lists after every `welcome`, records, withdraws, follows the hints |

Apps of versions 1 and 2 behave like version 3 here.

## 7. Tests the server needs

- Negotiation: a client `{1, 3}` gets no `blobs`, `blob.stored` or `blob.deleted` and `badRequest`
  for `blob.list`; a client `{1, 4}` negotiates 4.
- Listing: empty team; 1, 100, 101 and 250 decisions over pages (`more`, continuation with the
  last id); `after` of a deleted blob and of another team's blob is `notFound`; other kinds are
  not listed under `decision`; `data` never appears in the answer.
- Rate: a second listing within 5 s is `rateLimited` with `retryAfterMs` at most 5000; the pages
  of one listing pass back to back; a listing after 5 s passes.
- Restriction: an unproven listed device on version 4 gets `forbidden` for `blob.list`; an
  unlisted device on any version too.
- Count limits: the 201st decision of the team and the 51st of one member are `limit`, a task
  blob beyond 200 decisions is not; after a `blob.delete` of a decision the next `blob.put`
  passes; the counts, the size and the byte quota answer the same code; a decision `blob.put`
  larger than `MAX_DECISION_CIPHERTEXT_LEN` is `limit`; a decision `blob.put` on a version 3
  connection is `badRequest`; `blob.list` of `task` is `badRequest`.
- Hints: `blob.stored` reaches every counting version 4 connection of the team including the
  sender's other devices, not a version 3 connection of the same team, not an unproven twin;
  `blob.deleted` likewise after `blob.delete`, not after `notFound`; neither carries `seq`;
  neither is replayed after a reconnect.
- The page query uses an index (`EXPLAIN QUERY PLAN`).
- The app's side: `src-tauri/src/team_engine.rs` tests run the engine against an in-memory relay
  of version 4 with these rules (no rate check there; the engine's own rate is tested);
  `relay_interop.rs` can run against the new image once it exists.

## 8. Rollout order

1. **App repository (this change):** both crates and the app speak version 4; the app works with
   relays of versions 1 to 4. Ships with app 0.17.0.
2. **Server, in either order relative to the app release** (the matrix holds both ways):
   1. Vendor the crates (section 9).
   2. Implement sections 1 to 5 with the tests of section 7; update `docs/protocol.md`,
      `docs/security.md` (decisions: metadata the relay keeps, the count limit) and
      `docs/protocol-wishes.md` (listing done).
   3. Build and publish a new image; the owner deploys it.
3. The website needs no change.

## 9. Vendoring

As for version 3 ([[relay-v3-server-spec]], section 11), with "protocol version 4, blob listing
and decisions" in both `vendor/*/ORIGIN.md`. Expected compile breaks: every exhaustive `match`
over `ClientMessage` needs `BlobList`, every one over `ServerMessage` needs `Blobs`, `BlobStored`
and `BlobDeleted`; `RecordKind::Blob` is gone (the relay does not use it; check with
`cargo check`). Read the protocol crate's README changelog ("Version 4") and the crypto crate's
("protocol version 4").

## Open

> [!question] Unverified
> Nothing here ran against a real relay. Whether SQLite walks `(team_id, kind)` in rowid order
> for the page query is expected from how SQLite stores index entries (the rowid is the last
> key column), not measured on the server's database.
