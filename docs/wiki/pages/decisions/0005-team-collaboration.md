---
title: "ADR 0005: Team collaboration of people and agents"
type: decision
status: proposed
superseded_by:
tags: [collaboration, teams, agents, server, e2e, git, worktree]
sources: []
created: 2026-11-08
updated: 2026-11-22
---

# ADR 0005: Team collaboration of people and agents

## Context
Several people, each with their own AI plan (Claude, Codex, Gemini …) and their own machine,
want to work on one project the way they already do with Git. With agents doing most of the
edits, four problems appear that Git alone does not solve:

1. **Same files:** two agents change the same files at the same time; the conflict shows up only
   at merge time.
2. **Duplicate work:** two agents solve the same task without knowing it.
3. **Contradicting decisions:** one agent changes the architecture, another builds on the old one.
   No textual conflict, but the result does not fit together.
4. **Missing context:** agent B does not know what agent A is doing or why.

The goal is a solution that is absolutely simple for the user. Requirements were collected in an
interview with the owner on 2026-11-08; the answers are recorded below.

## Requirements (owner's answers)
| Topic | Decision |
| --- | --- |
| Setup | Every person works on their own machine with their own Hangar and agents; code is exchanged through a shared Git repository. |
| Problems to solve | All four above. |
| Transport | All methods over time: Git, a sync service, direct peer to peer. |
| Scale | Open: from two people up to open source projects with many contributors. |
| Prevention | Task reservation, file and area claims, early conflict detection, automatic merging. All combined. |
| On overlap | The agent pauses, the human decides; both people get a message with the conflict and a proposal. |
| Precedence | Whoever reserved first wins. The second agent pauses; its person can request a release, the first person accepts or declines with one click. |
| Expiry | Reservations expire after inactivity (e.g. 30 minutes without changes) while the person is offline, with a warning first. |
| Visibility | Task and status, changed files (live, uncommitted), decisions with reasons, full transcripts. |
| Transcripts | Private by default, shared per session on purpose; detected secrets are redacted. |
| Participants | Hangar only (no open protocol for other front ends in version 1). |
| Identity | Hangar account with teams. |
| Service | An official service plus an open source server for self-hosting. |
| Data on the server | End-to-end encrypted: tasks, claims, status, decisions and shared transcripts are readable by the team only, not by the operator. Code stays in Git. |
| Claims | Observed automatically: Hangar watches which files an agent changes in its worktree and reports them at once. Agents can also announce areas in advance through a Hangar tool. |
| Platforms | Windows, macOS and Linux before collaboration ships, since teams are mixed. |
| Tasks from | A task list in Hangar, GitHub and other Git hosts' issues, other boards (Jira, Linear and similar), and derived from the first prompt. |
| Integration | Every task in its own worktree and branch. Finished tasks enter a merge queue: Hangar updates the branch, the agent resolves conflicts, tests run, the human confirms with one click; one at a time. |
| Shared memory | Every agent automatically gets decisions, running tasks and claimed areas before it starts; shared transcripts on demand. |
| Notifications | In Hangar and as a desktop notification, push to the phone, e-mail, chat (Slack, Teams, Discord). |
| Business model | Free for small teams, paid per person above that; self-hosting is always free. |
| Version 1 scope | Core first: account and team, task list with reservation, automatic file claims with warning and pause, live status of all agents, shared memory. Merge queue, issue import, transcripts, phone and chat follow. |

## Licensing and editions (owner's decision, 2026-11-08)
| Use | Account | Price | Where enforced |
| --- | --- | --- | --- |
| Alone, all local features | none | free | nothing to enforce |
| Team of up to 3 people | Hangar account | free | team server counts members |
| Team of 4 or more | Hangar account | paid package per seat | team server counts members |
| Self-hosted server, up to 3 people | none at Hangar | free | self-hosted server |
| Self-hosted server, 4 or more | none at Hangar | license key | self-hosted server checks a signed key |

- **Desktop app: MIT**, unchanged. Anyone may fork it, including removing account code; that is
  accepted. The app is never trusted to enforce limits and holds no service secrets.
- **Team server: separate repository under the Elastic License 2.0** (source available). Use,
  modification and self-hosting are allowed; offering it to third parties as a hosted or managed
  service and moving, changing, disabling or circumventing the license key functionality are not.
- **Enforcement on the server only:** joining a team is a server operation, so the seat limit holds
  for every client, including forks. End-to-end encryption does not conflict: the member list is
  metadata the server needs for key distribution anyway.
- **License keys for self-hosting:** an Ed25519-signed document (team id, seats, expiry) the server
  verifies offline with an embedded public key; issued by the website after purchase.
- **Brand:** "Hangar" and the logo are the protection for the official product; a trademark
  search and registration and a `TRADEMARKS.md` policy (forks must rename) follow.
- **Contributions:** CLA for the server repository (keeps relicensing and commercial licensing
  possible), DCO sign-off for the MIT app.
- **Residual risk:** someone can write their own compatible server from scratch; the protocol is
  not protected. The official service, the brand and the self-hosting license are the moat.
- Not legal advice: terms, license texts and trademark need review by an IT lawyer before sales.

## Options (to be decided after research)
1. **Coordination state in Git only** (files or a dedicated ref such as `refs/hangar/*`): no server,
   works offline, but not live and visible to everyone with repository access in plain text.
2. **Hangar service with end-to-end encryption** (chosen direction): live, works across Git hosters,
   accounts and teams; the server stores encrypted blobs and routes events.
3. **Peer to peer** (LAN, Tailscale): no server, but needs both peers online; later as an addition.

## Proposed decision
Option 2 as the backbone, with Git as the source of truth for code and as an offline fallback for
reservations, and peer to peer as a later transport. Version 1 scope as in the table.

## Open questions (research before building)
Answered by desk research on 2026-11-09 in [[team-service-v1]], which also holds the build plan.

- **Pausing an agent:** which CLIs allow Hangar to block an edit before it happens (e.g. Claude Code
  hooks such as PreToolUse, MCP tools) and how Codex CLI, Gemini CLI and others can be paused.
  Without a hook Hangar can only warn and stop the process; the catalog needs a per-agent
  capability flag.
- **End-to-end encryption for groups:** key management when people join and leave (MLS, RFC 9420,
  versus simpler per-team keys with rotation), device keys, recovery.
- **Server stack and hosting:** Rust service, storage, push channels; cost per team for the free tier.
- **Change observation:** file watching per worktree (notify crate) with debounce, mapping hunks to
  areas, early conflict detection against other people's live changes.
- **Secret redaction** for shared transcripts: detection rules and false positives.
- **Shared memory format:** what is injected into each agent (instruction file, MCP resource) and
  how large it may get.
- **Project profile:** the current non-goal "remote multiplexing like tmux" stays (no remote
  control of sessions), but Hangar gains a network service; the project rules must say so.

## Amendment draft (2026-11-22): team decisions as shared memory
Status: proposed, not built. The version 1 scope promises that every agent gets the team's
decisions with their reasons before it starts; today it gets only reservations and files.

### Context: what the context carries today
Claude Code takes no http hook at `SessionStart`, so the facts go in through `UserPromptSubmit`
before every turn ([[team-service-v1]], step B): `GateService::context` builds them with
`gate::context_facts_full` (`src-tauri/src/gate.rs`) from memory, at most `MAX_CONTEXT_CHARS`
(4000) characters, reservations first (at most `MAX_CONTEXT_TASKS` = 10 lines and half of the
characters, `team_tasks::TeamTask` from `TeamService`), then the files of other sessions and of
teammates (at most 30), one JSON line each with every string a `literal()`. The gate sends the
text again only when it differs from the last one or the conversation is new (`Gate::tell`).
### Why it cannot be built cleanly on the current protocol
Checked in the code on 2026-11-22 (`crates/hangar-team-protocol/src/wire.rs`,
`crates/hangar-team-crypto/src/record.rs`, `src-tauri/src/project_notes.rs`, the relay's
`src/relay/store.rs`):
1. **Blobs exist, a way to find them does not.** `blob.put` with `BlobKind::Decision` stores a
   sealed blob (`RecordKind::Blob(Decision)`, up to about 128 KiB) and answers its id;
   `blob.get` needs that id. There is no listing, no event that announces a new blob and no
   delete. The relay keeps decision blobs without expiry, within the team's blob quota. The only
   channel to pass ids would be the presence status, which is live state of online devices: a
   decision of a teammate who is offline, or one a new member never saw announced, would not
   reach anyone.
2. **Sealed under one epoch.** A blob is sealed under the epoch of its writing. Members admitted
   later hold only the current epoch, and every device keeps only the newest 8
   (`team::KEPT_EPOCHS`), so decisions become unreadable after rotations unless someone re-seals
   them, which is the churn [[0006-encrypted-project-notes]] rejected (option 2) for notes.
3. **Encrypted project notes are solo only.** Team vaults need the `notes-keys` record and its
   relay messages (phase 2 of [[0006-encrypted-project-notes]]); without them a decision written
   as a note is readable on the writer's computers only. ADR 0006 also lists "giving agents
   decrypted shared notes automatically" as a non-goal.
4. **Plain text in the repository** (for example a decisions file) contradicts the requirement
   that decisions are readable by the team only.

Building any of these would ship decisions that silently miss people, so nothing is built until
the protocol carries them.

### Proposed protocol addition
- **Key:** decisions are sealed with the newest key of the team's notes key ring
  (`notes-keys`, [[0006-encrypted-project-notes]] phase 2), which is not bound to an epoch, so
  new members read every decision and rotations re-encrypt nothing; each decision is signed by
  the writing device and checked against the verified log like every record.
- **Crypto crate change:** today `RecordKind::Blob(BlobKind::Decision)` (label `blob/decision`)
  seals only through `EpochCipher`, whose key comes from the epoch secret. Sealing with a notes
  key needs a new seal and open path in `crates/hangar-team-crypto` (a record kind
  `decision`, label `decision/v1`, next to `notes.rs`): the key is
  `HKDF-SHA256(ikm = notes key, info = field("hangar-decision/v1/key") || field(team))`, the
  associated data binds label, team, notes key id, record id (the decision's UUID, so a blob
  copied under another id does not open), sender device, log head and counter, and the device
  signs it as `EpochCipher` records are signed. The envelope's `epoch` stays the current epoch at
  the time of writing: it names the log head the signature is checked against and keeps the
  relay's stale epoch check, but chooses no key. The unused `blob/decision` label goes. This is a
  change to `hangar-team-crypto` and its test vectors, and to `hangar-team-protocol`
  (messages below), so the server needs a vendor update and a new image.
- **Messages:** `blob.list { kind, after }` answers `{blob, member, device, createdAt}` per blob of
  the kind (paged, newest last); the event `blob.stored { kind, blob, member }` tells every
  device of the team; `blob.delete { blob }` by its writer, an owner or an admin, with the event
  `blob.deleted`. The relay keeps metadata only.
- **Protocol version:** `PROTOCOL_VERSION` goes from 1 to 2, `ProtoRange::CURRENT` becomes
  `{min: 1, max: 2}` on both sides, and `welcome.proto` names the version the connection speaks.
  Checked in the code: a relay of version 1 answers an unknown kind such as `blob.list` with
  `err badRequest` without an op id and closes the connection after more than 20 bad frames
  (`MAX_BAD_FRAMES` in the server's `src/relay/conn.rs`), and an app of version 1 ends the connection on any unknown server kind
  (`Engine::on_text`, `SessionEnd::Lost`). So: the app sends the new messages only on a
  connection whose `welcome.proto` is 2, and the relay sends `blob.stored` and `blob.deleted`
  only to connections of version 2. On a version 1 relay the Team view shows no "Decisions" block
  and the agent context no decisions (only what works is offered); a quiet line under the tasks
  says "Decisions need a newer team server" for owners and admins only, who can update it.
- **Replay:** `blob.stored` and `blob.deleted` are live hints and are not logged: no `seq`, no
  replay after a reconnect. What counts is `blob.list`: after every `welcome` (also after a
  resync) the app lists from the newest blob it knows (`after`), and a live hint only triggers
  the same list call. This keeps the event log free of a second kind of state and needs no
  snapshot rules. `blob.list` returns at most 100 entries per page; a connection may start at
  most one list every 5 s (more answer `rateLimited` with `retryAfterMs`), and pages of one
  listing follow at once.
- **Limits:** at most 200 decisions per team that are neither withdrawn nor superseded; a
  superseded or withdrawn decision is deleted (`blob.delete`), so its bytes leave the quota. A
  `blob.put` beyond 200, or beyond the team's blob quota (64 MiB by default on the self-hosted
  relay, `BLOB_QUOTA_BYTES`), answers `err limit`; the form keeps its text and says "Your team
  has as many decisions as it can keep. Withdraw an old one first." Nothing is dropped silently.
- **Document** (plaintext inside the sealed blob): `{"v":1,"id":<uuid>,"decision":..,"reason":..,
  "by":<display name>,"at":<Unix ms>,"repo":<root commit or null>,"supersedes":<id or null>}`,
  decision at most 300 characters, reason at most 1000. A decision that supersedes another hides
  it; nothing is edited in place.

### Proposed client behaviour
- **Team view:** a "Decisions" block below "Tasks" with one button "Record decision" (two fields:
  what was decided, why) and the list, newest first, each with who and when and one action
  (Withdraw for its writer, owners and admins).
- **Agent context:** the newest decisions of the session's linked project (and of its
  repository), before the reservations: at most 10 lines and 1200 characters, the rest counted
  (`{"moreDecisions":n}`), one line each
  `{"decision":..,"reason":..,"decidedBy":..,"at":"YYYY-MM-DD"}` with every string a `literal()`,
  the reason cut to 300 characters. Reservations then take at most 1400 characters, the files the
  rest of the 4000. Facts, not orders, as everything else in the context.
- **Which agents get them:** the context reaches only agents whose catalog entry has
  `context_inject = true`: today Claude Code (prompt hook `UserPromptSubmit`, measured) and Qwen
  Code (same hook, not measured). Codex CLI, Gemini CLI, Copilot CLI, Cursor CLI, OpenCode, Pi,
  Oh My Pi, Droid and private sessions get no decisions automatically; their person sees them in
  the Team view and can send one to the session the way notes are sent to a terminal today
  (`sendFromNotes`, which pastes into the session through `pasteIntoSession`). The start dialog promises decisions only
  for the agents that get them. A new prompt hook in an adapter adds an agent; nothing writes an
  instruction file into the repository for this.
- **Not a note:** ADR 0006 keeps "giving agents decrypted shared notes automatically" as a
  non-goal, and this draft does not change that. A note is free text a person writes for people
  (drafts, credentials by mistake, anything up to 1 MiB); handing it to an agent unasked would
  leak whatever it holds into the agent's provider. A decision is a short, structured statement
  written for exactly this purpose: the form says that agents of the team read it, its fields
  are capped at 300 and 1000 characters, and the writer chooses to share it. Decisions also do
  not live in `.hangar/notes`, so no note ever reaches an agent through this path.
- **Removed members:** a person who held the notes key ring keeps reading the decisions written
  up to the removal (they saw them anyway). After a removal the rotating device adds a new notes
  key (ADR 0006, section 3), so decisions written afterwards are closed to them. This is accepted
  on purpose, as for notes and the Git history.
- **Tests:** document limits and refusals, the context budget with many and long decisions,
  hostile strings as literals, a superseded or withdrawn decision gone from the context, and two
  people over real WebSockets (one records, the other's agent context shows it, also after a
  rotation and for a member admitted later).

### Consequences of the draft
- Changes to both crates: `hangar-team-crypto` (decision seal and open path, test vectors) and
  `hangar-team-protocol` (protocol version 2, `blob.list`, `blob.stored`, `blob.delete`,
  `blob.deleted`); the server needs a vendor update, the new messages and a new image. It
  depends on the `notes-keys` record of ADR 0006 phase 2, so both ship together.
- Mixed versions work: version 1 apps and relays keep running, they only show no decisions.
- The external crypto review ([[team-crypto]]) must cover the decision document and its seal.

## Consequences
- macOS and Linux become release blockers before collaboration ships.
- Hangar gains accounts, a server component and a security surface that needs its own threat
  model and review.
- The 0.3.0 worktree, diff, notification and status features are the local building blocks.
