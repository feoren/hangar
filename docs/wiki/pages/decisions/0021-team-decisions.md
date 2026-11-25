---
title: "ADR 0021: Team decisions as shared memory (team protocol version 4)"
type: decision
status: accepted
superseded_by:
tags: [collaboration, teams, protocol, relay, e2e, notes-keys, agents, context, blobs, compatibility]
sources: []
created: 2026-11-25
updated: 2026-11-25
---

# ADR 0021: Team decisions as shared memory (team protocol version 4)

Accepts and builds the amendment draft of [[0005-team-collaboration]] ("Amendment draft
(2026-11-22): team decisions as shared memory"), with the version numbers of today: version 2
went to the notes key ring ([[0006-encrypted-project-notes]], phase 2), version 3 to the proof of
possession and `blob.delete` ([[0020-proof-of-possession-at-hello]]), so the listing comes with
version 4. The draft's reasoning (why it cannot be built on the older protocol, why a decision
is not a note) stays in ADR 0005; this page records what was built and why it differs where it
does.

## Context
The version 1 scope promises that every agent gets the team's decisions with their reasons
before it starts. Up to version 3 a blob of the kind `decision` could be stored, fetched and
deleted, but nobody could find it: no listing, no event. Blobs were sealed under one epoch, so
members admitted later and devices after eight rotations could not read them.

## Decision

### Crypto (`crates/hangar-team-crypto`, module `decision`)
- A decision is sealed with the **newest key of the team's notes key ring**, which is not bound
  to an epoch: a member admitted later reads every decision, a rotation re-seals nothing, and
  the key a rotating device adds after a removal closes what follows to whoever left.
- `key = HKDF-SHA256(ikm = notes key, info = field("hangar-decision/v1/key") || field(team))`.
- The associated data has the shape of an epoch record's with kind label `decision/v1` and the
  **notes key id** after it, and binds version, team, the envelope's epoch, the decision id,
  sender member and device, log head and counter. The device signs it as records are signed
  (`hangar-team/v1/record-sig`), and the opener checks it against the verified log like
  `EpochCipher::open_verified` (head known, key period covers it, signature, epoch start,
  decryption, then fork).
- **The decision id** is the writer's lower case UUID. It travels in the clear in the header
  (the opener needs it for the associated data before decrypting) and the document must name
  the same id, so a blob the relay copies opens only as the same decision. The relay's blob id
  is not bound: it is assigned after sealing. This differs from the draft's wording ("so a blob
  copied under another id does not open"); a copy shows once, deduplicated by id.
- The plaintext is padded to 256 bytes like a note; at most 16 KiB.
- `RecordKind::Blob(BlobKind)` became `TaskBlob` and `TranscriptBlob`; the label
  `blob/decision` is gone (`RecordKind::for_blob(Decision)` is `None`). Safe for compatibility:
  no released app ever sealed a blob, and the relay uses `BlobKind`, not `RecordKind`.
- Test vector `VECTOR_DECISION`, recomputed by `test_vectors.py`.

### Protocol (`crates/hangar-team-protocol`, version 4)
- `PROTOCOL_VERSION` 4, `ProtoRange::CURRENT` `{1, 4}`, `DECISIONS_PROTOCOL_VERSION` 4.
- `blob.list {kind, after}` (a read, not deduplicated, not allowed before the proof) answered
  with `blobs {kind, blobs: [{blob, by, createdAt}], more}`, at most `MAX_BLOB_LIST` (100) per
  page in storage order, newest last; an unknown `after` is `notFound`.
- Live hints `blob.stored {kind, blob, by}` and `blob.deleted {kind, blob}`: not logged, no
  `seq`, not targeted, only to version 4 connections that count as devices of the log.
- `MAX_DECISIONS` (200), `MAX_DECISIONS_PER_MEMBER` (50), `MAX_DECISION_CIPHERTEXT_LEN` (24 KiB) and
  `BLOB_LIST_EVERY_MS` (5000) in `limits`. Decision puts and listings only on version 4 and only
  of the kind `decision` (relay rules in [[relay-v4-server-spec]]).
- `by` is the full holder (member and device), not the member alone as in the draft: the opener
  needs the device to check the signature.

### App
- After every `welcome` of version 4 the engine lists from the first; the full listing drops
  what the team no longer has (a withdrawal missed while offline). `blob.stored` lists from the
  newest blob known, `blob.deleted` forgets the id. One listing per 5 s per connection; a hint
  within that time waits for the next ping. Pages of one listing follow at once.
- Fetches go a few at a time (8 `blob.get` in flight). A decision waits for a notes key this
  device lacks, or for a log fetch when it names a newer head; then it opens or counts as
  unreadable. Opened decisions stay in memory across connections, never on disk.
- **An owner's or admin's device makes the team's notes key ring on a relay of version 4, but
  only for a team that shows no trace of an earlier one:** the full listing found no decision
  and this computer's keychain keeps no notes keys of the team (the shared notes' cache, read by
  the worker before every connection). Otherwise the relay may be hiding the ring, and a new one
  would split the team's keys: nothing is written, and the Team view waits for the keys.
  Decisions need the ring, and a member cannot make one; shared notes still turn on per folder,
  so this only puts keys in place. Not in the draft.
- **While this device holds no notes key** (`DecisionsState::WaitingForKeys`) the card shows one
  calm line instead of its button: "Decisions start once an owner or admin of the team is online
  with this Hangar version."
- **Decisions are named by writer and id together**, commands by the relay's blob id: any member
  can seal a document with any id, so one member's id never hides, replaces or redirects a
  withdrawal of another's (security review M1). A replacement or withdrawal of another member's
  decision names that member (`supersedesMember`).
- Recording: `blob.put` of the sealed document; the window's request is answered by the relay's
  answer, so a refusal keeps the form's text. Local refusals: an invalid text, 200 decisions of
  the team or 50 of this member known, no ring yet, a key change pending (a removal not rotated
  yet, or the newest notes key not yet renewed after one), as notes refuse saving then. The
  relay's `limit` (count, per member count, size or quota) shows "Your team has as many
  decisions as it can keep. Withdraw an old one first." A `forbidden` answer to `blob.put` means
  the connection does not count as the log's device yet and shows as busy, not as a role.
- **Withdrawals give way at one person's limit** (acceptance round 3, R1). Withdrawal records
  count toward the 50 per member for 30 days, so an owner or admin who withdrew many decisions
  would otherwise record nothing for a month. When recording meets the per member limit, locally
  or as `limit` from the relay, the engine first deletes this person's own oldest withdrawals
  whose decision is gone (the ones the 30 day cleanup would take, only as many as needed; the
  relay takes the deletes before the put, one connection's frames in order) and sends the record
  once more. What they hid stays hidden on this device. If this person's own real decisions fill
  the limit, the answer is `decisionLimitOwn`: "You have recorded as many decisions as one person
  can. Withdraw one of your own first." The team's limit keeps its own text.
- **Withdraw asks first** ("Withdraw for the whole team?", Withdraw and Cancel, the focus on
  Cancel, Escape cancels), then stores a **signed withdrawal** (a document with `withdrawn`,
  naming the decision, without text) and only then deletes the decision (`blob.delete` by its
  writer, the owner or an admin). Whatever a withdrawal or replacement hides stays hidden on
  every device that saw it, also when a relay restored from a backup serves the decision again
  (security review N7). A withdrawal without room for its record deletes anyway. Withdrawals of
  decisions that are gone are cleaned up after 30 days by their writer, the owner or an admin, so
  they do not fill the team's room for good.
- A decision with `supersedes` hides the one it names only when its writer could withdraw that
  one (same member, owner or admin); the superseding device deletes the old blob after its own is
  stored. The UI offers Record and Withdraw; superseding is in the engine and the document format.
- **Former members** (security review M2): a decision whose writer the verified log no longer
  lists shows in the Team view marked "Former member", the owner and admins can withdraw it, and
  **no agent gets it as context**. That is the counterpart of ADR 0006's old-key rule: a removed
  device may still hold an older notes key and seal a backdated decision, so nothing of a writer
  who left reaches an agent unasked. Re-recording one under one's own name is a new decision
  (no extra control).
- Document v1: `{"v":1,"id","decision","reason","by","at","repo","supersedes"}` plus, only when
  set, `supersedesMember` and `withdrawn`; decision 1 to 300 characters on one line, reason up to
  1000, no control characters (a line break only in the reason) and nothing
  `text::is_invisible` covers (bidirectional and zero width characters, tags, the Arabic letter
  mark, variation selectors and the others); a document breaking a limit, or larger than 16 KiB,
  is refused when read too, and a blob larger than `MAX_DECISION_CIPHERTEXT_LEN` is not even
  decoded. The window records team-wide decisions (`repo` null); the format and the context
  filter already take a repository.
- A decision that cannot open yet keeps only what it waits for (the notes key id, or the log
  head) and is fetched again once that arrived; a listing stops 50 above the team's limit. A
  fork is reported only from the sender's current key period and the current epoch (security
  review N5); anything else is unreadable.
- Agent context (`gate::context_facts_team`): decisions before reservations, at most 10 lines
  and 1200 characters, each `{"decision","reason","decidedBy","at":"YYYY-MM-DD"}` with every
  string through `literal_cut`, which cuts by the length **as written** (a character shown as
  `<U+XXXX>` counts eight, review M3), white space runs as one space, the reason cut to 300. A
  line that no longer fits is counted and shorter ones after it still go in
  (`{"moreDecisions":n}`). `decidedBy` is the name cut to 40 characters and the full member id;
  `at` is the day the relay stored it, not the writer's clock (review N3). Only current members'
  decisions come here. With decisions the reservations take at most 1400 characters, the files
  the rest of 4000; without decisions the text is byte for byte the one of before.
- Who gets them: only agents whose catalog entry has `context_inject` and whose gate is built and
  measured, since the context goes out through their prompt hook alone: today Claude Code (Qwen
  Code has the flag, but its recipe is not measured, so it gets no gate). For every other running
  session the Team view offers "Send to session", which pastes the decision and its reason as
  notes are pasted (`sendFromNotes`). `ProfileView.contextInject` tells the window which is which.
- Team view: a "Decisions" card below "Tasks" (one "Record decision" with two fields, the list
  newest first with who and when, the day the relay stored it, Withdraw where allowed, asked
  once inline: "Withdraw for the whole team?" with the focus on Cancel). On a relay older than
  version 4 there is no card; owners and admins see the quiet line "Decisions need a newer team
  server." While this device holds no notes key of the team, everyone sees the quiet line
  "Decisions start once an owner or admin of the team is online with this Hangar version."
  instead of the card.

## Compatibility matrix

| | Relay of version 3 (today) | Relay of version 4 |
| --- | --- | --- |
| **App of version 3 (0.16.0)** | as today | negotiates 3: no listing, no hints (the relay sends no version 4 kind), no decisions; a decision blob another app stored stays invisible to it |
| **App of version 4 (this change)** | negotiates 3: no `blob.list`, no ring made for decisions, no Decisions card, the quiet line for owners and admins, no decisions in the context | negotiates 4: lists after every `welcome`, records, withdraws, the context carries decisions |

Apps and relays of versions 1 and 2 behave like version 3 in this table (they negotiate their
own version). Server side: [[relay-v4-server-spec]].

## Consequences
- Changes to both crates; the server needs a vendor update, the listing, the hints, the count
  limit and a new image ([[relay-v4-server-spec]]).
- The external crypto review ([[team-crypto]]) must cover the decision seal and the document.
- Residual risk: a removed device that still holds an older notes key could seal a decision
  naming a log head from before its removal; an honest relay refuses its `blob.put` (the log no
  longer lists it), a relay that colludes could store it. It shows only in the Team view, marked
  as a former member's, never in an agent's context; owners and admins can withdraw it. A
  device removed while its member stays in the team is still that member's: its old decisions
  count (the member vouches for them).
- Residual risk: a relay restored from a backup older than a withdrawal brings the decision back
  on devices that never saw the withdrawal (new devices, or after a restart, since hidden
  decisions are kept in memory). The owner or an admin withdraws it again.
- The relay sees how many decisions a team has, their sizes (padded), who stored them and when.

## Evidence
- `crates/hangar-team-crypto/src/decision.rs` and its tests in `src/tests.rs` (rotations,
  a member admitted later, a removed member's old and new decisions, a stranger, a copy under
  another id, another team, epoch and head checks, padding, the vector).
- `crates/hangar-team-protocol/src/wire.rs` (`BlobList`, `BlobListing`, `BlobEntry`,
  `BlobStored`, `BlobDeleted`), `src/limits.rs`; tests for strict decoding, the page limit, the
  version table.
- `src-tauri/src/team_decisions.rs` (document, limits, superseding, withdrawals, writer and id
  as the name), `src-tauri/src/team_engine/decisions.rs` and its tests in `team_engine.rs`
  against the in-memory relay of version 4 (two people, a withdrawal a restored relay cannot undo,
  a withdrawal without room, two members with the same id and a Withdraw that hits the blob
  shown, superseding, the team and per member limits with the text kept, `forbidden` as busy, a
  relay of version 3, waiting for keys, no ring where the team shows a trace of one, no record
  while the key waits for renewal, a blob larger than a decision, a reconnect that drops what
  went while away, a former member's decision kept out of the context and withdrawn by the
  owner, old withdrawals cleaned up), `src-tauri/src/gate.rs` (budgets, hostile strings, a
  decision of 300 question marks), `src/TeamDecisionsView.tsx` with its tests (waiting line,
  the Withdraw question, the former member's mark).
- Security review and acceptance (2026-11-25): findings M1 to M3 and N1 to N7 and the
  server spec gaps are addressed as above and in [[relay-v4-server-spec]].

> [!question] Unverified
> The relay side is a specification. The two-person flow ran against the engine's in-memory
> relay, not over real WebSockets (`src-tauri/tests/team.rs` has no blob support yet).
