---
title: "Team protocol: messages, keyed hashes, reservations and the event log"
type: concept
tags: [collaboration, teams, protocol, websocket, hmac, hkdf, reservation, claims, threat-model]
sources: []
created: 2026-11-09
updated: 2026-11-09
---

# Team protocol

The protocol between Hangar and the team relay (team service step C, [[team-service-v1]]),
implemented in `crates/hangar-team-protocol` (MIT). This page explains the design; the crate's
doc comments hold the details. Nothing here was run against a real relay yet.

## Envelope

One JSON object per WebSocket text message, at most 1 MiB (`MAX_FRAME_BYTES`), checked before
parsing.

| Field | Client frame | Server frame |
| --- | --- | --- |
| `v` | protocol version, now 1 | the same |
| `id` | op id, required; chosen per intent, a retry repeats it | the op this answers, if any |
| `seq` | never | position in the team's event log, only on loggable events |
| `team` | required except on `ping` (also refused when encoding): the website's project id, opaque | the team the event belongs to |
| `type` | message kind (tables below) | message kind |
| `body` | the message's fields; absent for `ping` | the message's fields |

Decoding order: size; then `v` alone, read tolerantly (other fields ignored), so a frame of
another version gets a version error even when it carries fields this version does not know;
then the envelope (unknown and duplicate fields rejected); then the body, parsed straight from the
raw JSON into the message's type (`serde_json::value::RawValue`, no `Value` buffer), so unknown
and duplicate fields are rejected at every level instead of the last duplicate silently winning;
then the limits of lists and ciphertexts.

Version errors: on the relay, a client older than the relay gets `outdatedClient`, a newer one
`badRequest`; on the client, a newer relay means Hangar needs an update
(`DecodeError::needs_update`). `hello.proto` is a range `{min, max}` with `min <= max` (checked
when parsing); the client checks that `welcome.proto` is one it offered (`check_welcome`).

Error texts name only known message kinds and end serde's message before its first quote or
backtick (` ..`), since a misplaced ticket or path could otherwise reach a log; quoted parts
cannot be hidden one by one, as serde quotes field names without escaping. Bodies are written in
field declaration order; receivers must not depend on key order.

Identifiers from elsewhere (team, member = the website's user id, device, blob, stream) are
opaque: any characters except control (`Cc`), format (`Cf`) and line or paragraph separators
(`Zl`, `Zp`), at most 128 bytes. Identifiers the client makes (op id, session key) are
`[A-Za-z0-9_-]`, at most 64. The session key is random per session, never the local session
number.

## Client messages

| Type | Body | Answer |
| --- | --- | --- |
| `hello` | `ticket`, `device`, `proto {min,max}`, `lastSeq {stream, seq}` or null | `welcome` and replay, or `resync_required` |
| `presence.set` | `state` (active, idle, away), `status` sealed or null | `ack`; others get `presence.update` |
| `heartbeat` | `tasks` (keyed hashes, at most 64) | `ack`; held tasks get a new deadline |
| `task.reserve` | `task`, `epoch`, `prev` (the task's hashes under earlier epochs, at most 4), `note` sealed or null | `ack` and `task.reserved`, or `err conflict` with `holder` |
| `task.release` | `task` | `ack`, `task.released` |
| `task.release_request` | `task`, `note` | `ack`; the holder gets `task.release_requested` |
| `task.release_withdraw` | `task` | `ack`, `task.release_request_ended` (withdrawn) |
| `task.release_answer` | `task`, `requester`, `accept` | `ack`, `task.release_answered`, on accept `task.released` and `task.reserved` |
| `task.force_release` | `task` | owner or admin only: `ack`, `task.released` (forceReleased) |
| `task.rekey` | `from`, `to`, `epoch` | `ack`; moves the sender's hold of an earlier epoch to the current epoch's hash |
| `claim.set` | `session`, `repo`, `epoch`, `files` (at most 1000 `{area, file}`), `truncated`, `detail` sealed | `ack`; changed overlaps as `claim.overlap`; replaces the session's claims (one repository per session) |
| `claim.clear` | `session` | `ack`; ended overlaps as `claim.overlap` with level `none` |
| `blob.put` | `kind` (decision, task, transcript), `content` sealed | `ack` with `blob` |
| `blob.get` | `blob` | `ack` with `blob` and `content` |
| `ping` | none | `ack` |

The ticket is the website's short-lived Ed25519 ticket in compact form
(`base64url(payload).base64url(signature)`, payload with `v`, `kid`, `sub`, `prj`, `role`,
`seat_ok`, `iat`, `exp`, `jti`, `aud: "hangar-relay"`), carried unchanged; the relay verifies it,
the crate only checks printable ASCII up to 2048 characters and never prints it (`Debug` shows
`Ticket(..)`). `hello` is sent per team on a connection. The member of every intent is the
ticket's `sub` and the role its `role`; the relay accepts `hello.device` only if the website
lists that device for that member.

## Server messages

| Type | Body | Logged (`seq`) |
| --- | --- | --- |
| `welcome` | `proto`, `stream`, `head`, `member`, `epoch` | no |
| `resync_required` | `stream`, `head`, `reason` (noCursor, otherStream, tooOld, ahead) | no |
| `ack` | `blob`, `content` (both optional) | no |
| `err` | `code`, `retryAfterMs`, `holder` | no |
| `presence.update` | `holder {member, device}`, `state`, `status`, `at` | no |
| `task.reserved` | `task`, `holder`, `epoch`, `since`, `expiresAt` (null while it cannot expire), `note` | yes |
| `task.released` | `task`, `holder`, `reason` (released, handedOver, memberRemoved, forceReleased) | yes |
| `reservation.expiring` | `task`, `holder`, `expiresAt` | yes |
| `reservation.expired` | `task`, `holder` | yes |
| `claim.overlap` | `repo`, `session`, `other {holder, session}`, `level` (none, sameArea, sameFile), `file`, `files`, `detail` (the other side's, only at sameFile) | no |
| `task.release_requested` | `task`, `holder`, `requester`, `note` | yes |
| `task.release_answered` | `task`, `holder`, `requester`, `accepted` | yes |
| `task.release_request_ended` | `task`, `holder`, `requester`, `reason` (withdrawn, timedOut) | yes |
| `member.joined` | `member`, `role` (owner, admin, member) | yes |
| `member.removed` | `member` | yes |
| `key.rotated` | `epoch`, `wrapped` (the new key for this device, step E) | yes |
| `notify` | `topic` (attention), `content` sealed | yes |

`err` codes: `conflict` (held by someone else, or a release request is open), `limit` (frame,
list, ciphertext, reservations per member, also for a handover to a full member), `forbidden`
(not the holder, not a member, not owner or admin), `rateLimited` (with `retryAfterMs`; also a
release request during the cooldown), `outdatedClient` (the client's newest version is older than
the server's oldest), `staleEpoch` (a hash or ciphertext of another epoch), `badRequest` (also an
op id reused with another body), `notFound`.

Presence and overlaps are live state and never logged: a reconnecting client sends its claims
again and receives presence anew.

## Keys and keyed hashes

The relay must tell "same task", "same area" and "same file" without learning names or paths.

- **Keys per epoch:** the epoch secret (step E distributes it) is expanded with HKDF-SHA256
  (RFC 5869, no salt) into separate keys; `info` is a label, the team id and the epoch, each
  length-prefixed: `hangar-team/v1/match-key` for keyed hashes, `hangar-team/v1/seal-key` for
  sealed content. The secret itself never keys anything the relay sees, and the two uses never
  share a key (`EpochKeys`). Key bytes and hash states are wiped on drop (`zeroize`).
  `MatchKey::from_bytes` is crate-internal, so a caller cannot key hashes with the raw secret.
- **Hash input:** HMAC-SHA256 over a domain label and fields, each prefixed by its length
  (32-bit big endian), so two different inputs never form the same byte string. Labels: `task`,
  `repo`, `area`, `file`.
- **Task id:** trimmed, Unicode NFC, ASCII letters folded to lower case, no `Cc`, `Cf`, `Zl` or
  `Zp` characters, at most 256 bytes.
- **Repository:** its root commit (the oldest commit without parents; with several the smallest
  id), 40 or 64 hex digits in lower case. The same for every clone and forge, no URL parsing.
- **Path:** relative to the repository's top folder, taken from git's own output
  (`git status -z`, `git diff --name-only -z`), never raw hook input; Unicode NFC (macOS may
  store names decomposed); `\` becomes `/`, empty and `.` segments go; `..`, absolute paths
  (`/x`, UNC, a drive `C:` or `C:/x`) and `Cc`, `Cf`, `Zl`, `Zp` characters are refused; segments
  at most 255 bytes, the path at most 4096. `a:b` and `c:x` are ordinary names on Linux and
  macOS and stay allowed: git never prints a drive, and a name only some platforms can hold is
  a hash that matches nothing elsewhere. Rejecting `Cf` also refuses names with a zero width
  joiner (some emoji, some scripts); such a file is simply not claimed.
- **Case rule:** `CaseRule::Fold` (Unicode lower case, then NFC again) is the default and a
  repository setting of the team, not of the platform: a mixed team must hash alike, and a false
  "same file" only warns while a missed one hides a conflict. The rule is part of the hash input,
  so members with different settings never match by accident.
- **Area only:** per file the client sends the hash of its first folder (`area`, null for a file
  at the top) and of the whole path. Deeper folder levels are not sent (data minimization); a
  finer area later needs a new protocol version.

Test vectors in `src/hash.rs` (keyed hashes, the NFC case, the derived matching key and sealing
key) are computed independently by `test_vectors.py` (Python standard library), which also checks
the `Cf` table in `src/text.rs` against `unicodedata` (Unicode 16.0). HMAC is anchored with
RFC 4231 test case 2, HKDF with RFC 5869 test case 3.

What the relay still learns: whether a changed file lies in a folder at all, which changed files
share their first folder, how many files a session changes, and that a value repeats within an
epoch.

## Overlap levels

The relay compares claims of sessions of **different holders** (member and device; device ids
are only unique per member) in the same repository hash and epoch. One device's own sessions are
compared locally by step A with exact paths.

| Step A level | Relay | Rule |
| --- | --- | --- |
| `sameArea` | `sameArea` | equal area hash, no equal file hash |
| `sameFile` | `sameFile` | equal file hash |
| `overlappingLines` | not decided | base commit and line ranges travel encrypted in `claim.set`'s `detail`; the relay passes each side's detail to the other in `claim.overlap` while they share a file, and each client refines `sameFile` locally with step A's rule |

The relay sends a `claim.overlap` to both sides when a pair's result changes, level `none` once
when an overlap ends, and the other side's new detail when only the detail changed.

## Reservations

Decided only by the relay, in arrival order per team (`ReservationBook::apply`); clients never
apply a reservation optimistically.

```
Free/Expired --reserve--> Held --(deadline - warn)--> Expiring --deadline--> Expired
Held/Expiring --heartbeat or reserve by holder--> Held (new deadline)
Held/Expiring --release_request--> ReleaseRequested --answer accept--> Held (requester)
                                   ReleaseRequested --decline, timeout, withdraw--> Held or Expiring
any hold --release, force release, member or device removed--> Free
old-epoch hold --rekey, or reserve with prev by the holder--> hold under the new hash
old-epoch hold --grace ends--> Expired
```

- **Time first, apart from the outcome:** every book operation first applies time to all
  entries (request timeouts, warnings, expiry, the end of the rekey grace) and returns those
  events in `Applied::timing`, separate from the intent's `outcome`. The relay sends the timing
  events whether the intent was applied or refused, and a later tick never repeats them. Conflict
  and limit checks therefore never see a hold that already ran out (a rekey onto an expired hold
  succeeds).
- **First wins:** a `task.reserve` for a task held by another device is `conflict` with the
  holder. The holder's own repeat counts as activity.
- **Deadline:** `last activity + 30 min` (`Policy::default`, ADR 0005). While the holder's device
  is connected it does not run (configurable: `expire_while_online`). When the device disconnects
  the deadline is at least `offline_since + warn_before`, so a warning always comes first.
  Presence counts connections: a device with two connections stays online until both closed.
- **Warning** at `deadline - 5 min` (inclusive), **expiry** at `deadline` (inclusive). A
  heartbeat exactly at the deadline is too late (`badRequest`); the client has to reserve again
  and may lose. A tick later than the deadline expires without a warning.
- **Release requests:** one open request per task; it does not stop expiry. It ends with the
  holder's answer, after 10 minutes without one (`timedOut`), or when the requester withdraws it
  (`withdrawn`, also when the requesting device is removed). After a decline, a timeout or a
  withdrawal the requesting member waits 10 minutes before asking for the same task again
  (`rateLimited` with `retryAfterMs`), from any of its devices, so a holder cannot be flooded;
  the cooldown ends when the task gets a new holder. When the requester leaves the team, the
  request is withdrawn without an event (`member.removed` tells).
- **Limits:** at most 32 reservations per member. A member at the limit cannot ask for a
  release, and an accept to a member at the limit is refused with `limit` (the request stays
  open, so the holder can decline).
- **Force release:** an owner or admin (role from the ticket) can end any hold
  (`forceReleased`), against squatting. There is no maximum hold duration: a long task is not
  squatting, and a person decides that better than a timer.
- **Epochs and rotation:** `task.reserve` and `task.rekey` must use the current epoch
  (`staleEpoch`). After `key.rotated`, holds of earlier epochs stand for a grace period
  (`rekey_grace`, 30 minutes, as long as an offline hold lasts). Their holders move them with
  `task.rekey` (from must be a hold of an earlier epoch; from equal to to is refused) or simply
  reserve the new hash with the old one in `prev`. Everyone else's `task.reserve` names the
  task's hashes under the earlier epochs whose keys the client still has in `prev`; a hold of an
  earlier epoch under one of them is a `conflict`, so a holder who is offline during the rotation
  keeps the task. Holds still old when the grace ends expire. A rotation during the grace starts
  it again for every old hold. Alternative considered: accepting rekeys only for some minutes
  after a rotation; it still loses the task of an offline holder to whoever reserves the new
  hash first, which `prev` prevents.

## Event log

Per team: sequence numbers from 1, a replay window of the newest events, and a `stream` id per
incarnation of the log (a restored backup starts a new stream).

| Client's `lastSeq` | Answer |
| --- | --- |
| null | `resync_required noCursor` |
| other stream | `resync_required otherStream` |
| ahead of the head | `resync_required ahead` |
| equal to the head | `welcome`, nothing to replay |
| its next event still in the window | `welcome`, then the events after it with `seq` |
| older | `resync_required tooOld` |

After `resync_required` the client drops its team state; the relay sends the current state as
events without `seq` (`member.joined`, `presence.update`, `task.reserved` per hold, plus
`reservation.expiring` and `task.release_requested` for holds in that state) and ends with
`welcome`, whose `head` is the new cursor. On the client, `ClientCursor::observe` applies the next
event, ignores duplicates and reports a gap (reconnect).

`RecentOps` remembers the answers to recent ops keyed by sender (member and device), team and op
id, with the SHA-256 digest of the body (`ClientMessage::digest`): the same op again gets its
first answer instead of being applied twice; the same op id with another body is refused with
`badRequest`.

## Threat model

What the relay can and cannot do, and what the protocol relies on:

- **The relay operator** sees the member and device list, epochs, times, sizes, who talks to
  whom, which tasks are held by whom (as hashes), and the overlap structure described under
  keyed hashes. It cannot read sealed content or names without a key.
- **Keyed hashes protect only against a relay that never held a key.** An operator who is or was
  a member of the team knows the matching keys of the epochs they saw and can test guesses
  (common task ids, typical paths) against every hash of those epochs, offline. Rotation on every
  leave limits this to the epochs the person saw; it does not protect past data from them.
- **Stale epochs:** the relay refuses `task.reserve`, `task.rekey`, `claim.set`, `blob.put` and
  every sealed field of an epoch other than the current one (`ClientMessage::check_epoch`,
  `staleEpoch`), so nothing new is stored under keys a removed member still has. Old-epoch holds
  remain only for the rekey grace.
- **Identity binding:** the member comes from the verified ticket, the device must belong to
  that member, the role comes from the ticket. Without this binding a member could claim another
  member's device and its holds.
- **Abuse by members:** a member can reserve tasks without working on them (owner and admin
  force release), flood release requests (cooldown), or leave `prev` out to win a task during a
  rotation (honest clients send it; a dishonest member has simpler ways to cause trouble, such as
  reserving the task in the first place).
- **Logs:** error texts never quote frame content; `Debug` of tickets, keys, task ids and paths
  shows nothing.

## Limits

| Constant | Value |
| --- | --- |
| `MAX_FRAME_BYTES` | 1 MiB |
| `MAX_ID_LEN` / `MAX_OPAQUE_ID_LEN` | 64 / 128 |
| `MAX_TICKET_LEN` | 2048 |
| `MAX_CIPHERTEXT_LEN` (blobs, claim detail in `claim.set` and `claim.overlap`) | 175 000 characters (about 128 KiB) |
| `MAX_INLINE_CIPHERTEXT_LEN` (status, notes, notify content, wrapped key) | 16 KiB |
| `MAX_CLAIM_FILES` | 1000 (more: `truncated`) |
| `MAX_HEARTBEAT_TASKS` | 64 |
| `MAX_PREV_HASHES` | 4 |
| `MAX_RESERVATIONS_PER_MEMBER` | 32 |

The largest `claim.set` (1000 files with area and the largest detail) and the largest
`claim.overlap` fit in a frame; a test checks both. Changing a limit or a hash rule needs a new
protocol version.

> [!question] Unverified
> No relay exists yet; the message set, the limits and the timing defaults (request timeout,
> cooldown, rekey grace) are a design, not a measurement. The cost of comparing 1000-file claim
> sets on every `claim.set` is not measured. That the `zeroize` features of `hmac` and `sha2`
> wipe every copy of the key state (including inside `hkdf`) is taken from their feature
> descriptions, not checked in memory.
