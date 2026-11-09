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

One JSON object per WebSocket text message, at most 1 MiB (`MAX_FRAME_BYTES`). The relay's
WebSocket reader refuses a larger message before decoding (the connection ends, no `err`);
`decode_client` and `decode_server` check the size again as a second line, before parsing.

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
| `ticket.renew` | `ticket` (a fresh website ticket, same type as in `hello`) | `ack`; a changed role as `member.role_changed`; close 4403 on no seat, another subject or an invalid ticket; another team: `err forbidden`, then 4403; before `hello`: `badRequest` |
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
| `device.keys` | `sign` (Ed25519), `kem` (X25519), each 32 bytes as 43 characters of canonical base64url | `ack`; replaces the device's keys |
| `devices.get` | none | `devices` |
| `memberlog.append` | `index` (from 1, no gaps), `prev` (hash of the entry before, null on the first), `entry` (opaque, at most 16 KiB) | `ack`, `memberlog.appended`; `conflict` when the index was taken first |
| `memberlog.get` | `after` (0: from the start) | `memberlog.entries` |
| `key.rotate` | `epoch` (exactly current + 1), `wrapped` (at most 512 `{holder, data}`, one per holder) | owner or admin only: `ack`, `key.rotated` per device |
| `key.wrap` | `epoch` (the current one), `wrapped {holder, data}` | `ack`, the target device gets `key.wrapped`; hands the current key to one (newly approved) device; `conflict` when the device already has one in this epoch |
| `key.get` | `epoch` | `ack` with `content` = `{epoch, data}`, the sender's own wrapped key; `notFound` if none |
| `ping` | none | `ack` |

`presence.set` with `offline` is refused (`badRequest`): only the server reports a device offline.
A client never sends `notify` (decoding refuses it like an unknown type).

**Answers and retries.** The answer to a client frame carries its op id as `id`: `ack` or `err`,
`welcome` or `resync_required` for `hello`, `devices` and `memberlog.entries` for the two reads.
The relay remembers the answer by op id only for intents that change shared state once
(`ClientMessage::deduplicated`): every `task.*`, `blob.put`, `memberlog.append`, `key.rotate`,
`key.wrap`; a retry gets the first answer and is not applied again. `presence.set`, `heartbeat`,
`claim.set`, `claim.clear` and `device.keys` replace the sender's own state and are applied and
acknowledged every time, and so is `ticket.renew` (the same ticket verifies to the same result);
reads, `hello` and `ping` are answered every time.

The ticket is the website's short-lived Ed25519 ticket in compact form
(`base64url(payload).base64url(signature)`, payload with `v`, `kid`, `sub`, `prj`, `role`,
`seat_ok`, `iat`, `exp`, `jti`, `aud`), carried unchanged; the relay verifies it, the crate only
checks printable ASCII up to 2048 characters and never prints it (`Debug` shows `Ticket(..)`).
The relay requires `aud` and accepts only its own name: `"hangar-relay"` for the official
service, the relay's `PUBLIC_URL` when self hosted, so a ticket for one relay is useless at
another; the crate has nothing to validate there. `hello` is sent per team on a connection. The
member of every intent is the ticket's `sub` and the role its `role`; the relay accepts
`hello.device` only if the website lists that device for that member.

**Ticket renewal.** Tickets are short-lived, connections are not: on a live connection the app
sends `ticket.renew` with a fresh website ticket about every 5 minutes per team. The relay
verifies it exactly like the `hello` ticket (signature, key id, expiry, `aud`, `prj` equal to
the frame's team) and applies its role in place (`member.role_changed` when it differs). A
ticket without a seat (`seat_ok` false), for another subject than the connection's member, or
one that does not verify ends the connection with 4403. A ticket whose `prj` is not the frame's
team is answered with `err forbidden`, and the connection is then closed with 4403 as well.
Before the team's `hello` was accepted, `ticket.renew` is `badRequest`.

**Devices are named by holder.** Clients choose device ids, so they are unique only per member.
Wherever a message names a target device it carries the full holder (member and device):
`WrappedKey.holder` in `key.rotate` and `key.wrap`, `key.wrapped.holder`, `devices` entries; the
relay delivers per device by holder, never by the device id alone.

## Server messages

| Type | Body | Logged (`seq`) |
| --- | --- | --- |
| `welcome` | `proto`, `stream`, `head`, `member`, `epoch` | no |
| `resync_required` | `stream`, `head`, `reason` (noCursor, otherStream, tooOld, ahead) | no |
| `ack` | `blob`, `content` (both optional) | no |
| `err` | `code`, `retryAfterMs`, `holder` | no |
| `presence.update` | `holder {member, device}`, `state` (active, idle, away, offline; offline without `status`, when the device's last connection closed), `status`, `at` | no |
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
| `member.role_changed` | `member`, `role` | yes |
| `key.rotated` | `epoch`, `wrapped` (only the receiving device's own wrapped key) | yes |
| `key.wrapped` | `epoch`, `holder`; only to that holder's connections | no |
| `notify` | `topic` (attention), `content` sealed | yes |
| `devices` | `devices` (at most 512 `{holder, sign, kem, at}`) | no |
| `memberlog.entries` | `entries` (at most 256 `{index, hash, entry, by, at}`), `more` | no |
| `memberlog.appended` | `index`, `hash`, `by` | yes |

`welcome` and `resync_required` carry the `hello`'s op id as `id`. `key.rotated` is one log entry
per rotation, but each device receives it with only its own wrapped key: the relay fills `wrapped`
from storage per receiving holder (member and device), live and on replay, so two devices see the
same `seq` with different bodies (`null` when the relay holds no key for that device and epoch).
`key.wrapped` is stricter: after a `key.wrap` the relay sends it only to the connections bound to
the target holder for the team, so the device knows to call `key.get`. It is never logged, since
one device's event in the shared log would be a gap for every other device. Fallback: after every
`welcome`, a device without the current epoch's key calls `key.get`; `notFound` means it still
waits for a `key.wrap`. `notify` is created by the relay only, topic `attention`: a release request for a task or an upcoming expiry
of a hold whose holder has no connection; it is logged, so the holder's devices see it on replay,
and the relay may also tell the person out of band. The relay holds no team key, so `content` is
null unless it passes on a sealed note it received.

`err` codes: `conflict` (held by someone else, or a release request is open), `limit` (frame,
list, ciphertext, reservations per member, also for a handover to a full member), `forbidden`
(not the holder, not a member, not owner or admin), `rateLimited` (with `retryAfterMs`; also a
release request during the cooldown), `outdatedClient` (the client's newest version is older than
the server's oldest), `staleEpoch` (a hash or ciphertext of another epoch), `badRequest` (also an
op id reused with another body), `notFound`.

Presence and overlaps are live state and never logged: a reconnecting client sends its claims
again and receives presence anew.

## Close codes

A close ends every team on the connection, so the relay closes only where the connection itself
is no longer acceptable; a refused intent is an `err` (module `close`).

| Code | Constant | When | Client |
| --- | --- | --- | --- |
| 1008 | `CLOSE_HELLO_TIMEOUT` | no valid `hello` within 15 s of opening (`HELLO_TIMEOUT_SECS`) | reconnects with a fresh ticket |
| 4401 | `CLOSE_ROLE_CHANGED` | the member's role changed since the connection's ticket was issued | fetches a new ticket and reconnects |
| 4403 | `CLOSE_REMOVED` | the member was removed; `ticket.renew` without a seat, for another subject, for another team (after `err forbidden`) or invalid | does not reuse the ticket; asks the website, which tells whether the seat remains |

1008 is the standard "policy violation" code; 4401 and 4403 lie in the range RFC 6455 leaves to
applications and mirror HTTP 401 and 403.

## Rights

The relay passes the role from the connection's verified ticket and the holder it bound to the
connection (ticket `sub` plus `hello.device`) into pure helpers (module `e2e`); nothing a frame
says about itself counts. The e2e helpers check rights before per-device state, so a refused
sender does not learn whether a device already has a key. `task.force_release` refuses a task
nobody holds with `badRequest` before checking the role, which reveals nothing: holds are in the
shared log.

| Intent | Allowed for | Refused | Helper |
| --- | --- | --- | --- |
| `task.force_release` | owner, admin | `forbidden` | `ReservationBook` |
| `memberlog.append` | owner, admin (entries are signed by admin devices in step E) | `forbidden` | `check_append_role` |
| `key.rotate` | owner, admin | `forbidden` | `check_rotate` |
| `key.wrap` | owner, admin, or another device of the target holder's member (adding one's own new device); never the target itself | `forbidden`; `conflict` when the device already has a wrapped key in the current epoch | `check_wrap` |

`check_wrap` takes the `key.wrap` message itself, whose `wrapped.holder` names the target with its
member, and whether the relay already stores a wrapped key for that holder in the current epoch. A
wrapped key is never replaced: a new key for a device comes only with a new epoch (`key.rotate`).
No device wraps for itself, since a key it stored for itself would block the real one for the
whole epoch.

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

## End-to-end groundwork

The relay holds no key, but it enforces the structure step E relies on (module `e2e`; the
cryptography itself is step E).

- **Device keys:** `PublicKey` is exactly 32 bytes as 43 characters of canonical base64url (no
  padding, unused trailing bits zero), so one key has one spelling. Whether the bytes are a valid
  curve point is step E's check. Clients trust a device's keys only when the membership log
  vouches for them; `devices` is a directory, not a trust anchor.
- **Membership log hash:** entry `index` hashes to
  `SHA-256(field("hangar-team/v1/memberlog") || field(index, 8 bytes big endian) || field(prev, 32 bytes or empty) || field(entry bytes))`,
  `field(x)` being `x`'s length as 32-bit big endian followed by `x` (as in the keyed hashes).
  The entry is hashed as the bytes its canonical base64url decodes to. Test vectors in
  `src/e2e.rs`, computed independently by `test_vectors.py`.
- **Team binding (step E):** the chain hash does not contain the team id. The signed content of
  every entry must therefore name the team, and clients must check it, or a relay could move a
  validly signed entry (such as "device X approved") from one team's log into another's.
- **Chain rule** (`check_append`, `MemberChain`): indexes from 1 without gaps, `prev` the
  predecessor's hash (absent on the first). A taken index is `conflict` (another device appended
  first; fetch and retry), a gap, a wrong `prev` or a non-canonical entry `badRequest`. Clients
  verify every received entry against their trusted head (`MemberChain::verify`) and never take
  `hash` from the relay unchecked.
- **Rotation rule** (`check_rotate`): owner or admin, epoch exactly current + 1 (`staleEpoch`
  otherwise), at least one wrapped key and at most one per holder. Covering every device is the
  rotating client's job. `key.wrap` must name the current epoch (`check_epoch`), follows the
  rights under [Rights](#rights) and never replaces a wrapped key; `key.get` may name any epoch
  the device had.
- **Sizes:** a log entry and a wrapped key are each at most 16 KiB, but the 1 MiB frame binds a
  list first: 512 wrapped keys fit when each is under about 1.7 KiB (an HPKE wrapped 32-byte key
  is about 110 characters), and the relay ends a `memberlog.entries` page early (`more`) when the
  next entry would not fit. `MemberLogEntries::page` builds such a page from a lazy source of
  stored entries (count and frame budget, worst case envelope included).

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
| `MAX_INLINE_CIPHERTEXT_LEN` (status, notes, notify content, wrapped key, membership log entry) | 16 KiB |
| `MAX_WRAPPED_KEYS` (per `key.rotate`, devices in `devices`) | 512 |
| `MAX_LOG_ENTRIES` (per `memberlog.entries`) | 256 |
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
