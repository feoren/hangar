---
title: "Team service version 1: research results and build plan"
type: synthesis
tags: [collaboration, teams, agents, server, e2e, git, worktree, hooks, claims]
sources: []
created: 2026-11-09
updated: 2026-11-09
---

# Team service version 1: research results and build plan

Answers the open questions of [[0005-team-collaboration]] from a desk research on 2026-11-09
(vendor docs, RFCs, crates.io). Nothing here was measured yet; items that need a measurement are
listed at the end. Versions are those published on crates.io on that day.

> [!question] Unverified
> The statements below come from desk research and are not yet filed as source pages or
> measurements. Each build step first files the sources or a measurement it relies on.

## 1. Pausing an agent before it edits a claimed file

| Agent | Block before an edit | Sees the file path | Context at start | Per-session config |
| --- | --- | --- | --- | --- |
| Claude Code | `PreToolUse` hook (command or http), matcher `Edit\|Write\|MultiEdit\|NotebookEdit`; deny via `permissionDecision` or exit 2 | yes, `tool_input` | `SessionStart` `additionalContext` (10 000 characters), `--append-system-prompt-file`, MCP | `--settings <file or JSON>` merges with the user's settings |
| Codex CLI | `PreToolUse` command hook covers `apply_patch`; deny or exit 2, no `ask` | `tool_input`; path shape of `apply_patch` > [!question] Unverified | `SessionStart` `additionalContext`, `AGENTS.md`, MCP | hooks need trust by hash (`/hooks`); no silent injection |
| Gemini CLI | `BeforeTool` command hook (`write_file\|replace`), `decision: deny` or exit 2 | yes | `SessionStart` `additionalContext`, `GEMINI.md` imports, MCP | `GEMINI_CLI_SYSTEM_SETTINGS_PATH` > [!question] Unverified whether user hooks still merge |
| Copilot CLI | `preToolUse` command or http hook; timeouts fail open (default 30 s) | yes, `toolArgs` | `sessionStart` `additionalContext`, MCP | > [!question] Unverified |
| Aider | no hooks, no MCP | only after the fact | `--read <file>` | CLI flags |

Consequences:
- Hooks are the primary mechanism. For Claude Code an **http hook to a local Hangar endpoint**
  fits best: Hangar holds the request open while the person decides, the session gets the hook
  through `--settings`, and a per-session token reaches the hook through an environment variable
  of the PTY child, never through a file.
- Hooks can fail open (timeouts, crashes, non-blocking exit codes, edits through the shell such as
  `sed -i`). The **file watcher runs for every agent as the second line**; for agents without hooks
  it is the only one. Suspending a process after the fact is a last resort: the file is already
  changed and a stopped agent may hold locks or stale content.
- The catalog gets per-agent capability flags: `preEditBlock`, `contextInject`,
  `perSessionConfig`.
- Context injection writes facts ("src/auth is claimed by A since 10:40"), not orders.

## 2. Observing changes and detecting conflicts early

- **Git is the truth, the watcher only a trigger.** `notify` 8.2 (CC0-1.0, a documented license
  exception) wakes a coalesced rescan (about 750 ms quiet, 3 s at most); the rescan runs
  `git status --porcelain=v2 -z` and `git diff -U0 HEAD` through the existing hardened `Git::run`.
  Git then decides what is ignored, and missed events (Windows buffer overflow, inotify limits,
  network paths) are repaired by the next scan and a slow safety poll (30 to 60 s). No `git2` or
  `gix`: the hardening stays in one place.
- **What leaves the machine:** base commit, path, kind of change, hunk ranges on the base side and
  an area (first directory levels or the nearest manifest, configurable in the repository). Never
  line content, diffs or commit messages.
- **Two stages of conflict detection:**
  1. Live and cheap: the same path on the same base with overlapping ranges (padded by a few
     lines) is "overlapping lines"; otherwise "same file" or "same area". On different bases only
     file level.
  2. Exact, on committed branches: `git merge-tree --write-tree --name-only -z` (Git 2.38 or
     newer; check the version at run time and fall back to stage 1).
- **Merge queue (later):** strictly serial as in the ADR, order held by the server, per entry the
  states Queued, Updating, ResolvingConflicts, Testing, AwaitingConfirm, Merging, Done, Removed; the
  final merge goes through the forge so branch protection stays in charge.
- Pitfalls: close watchers before removing a worktree (Windows keeps the directory open); branch
  switches flood events (watch the worktree's `HEAD`); case-insensitive paths need normalized
  keys; benchmark status and diff on a 50 000 file repository on Windows before fixing intervals.

## 3. End-to-end encryption for teams

- **No MLS in version 1.** OpenMLS 0.9.2 is pre-1.0, had a critical advisory in 2026 and its
  default crypto provider pulls an MPL-2.0 dependency. Version 1 uses team key epochs instead,
  behind a versioned `TeamCrypto` interface so MLS can follow:
  - every device has an Ed25519 and an X25519 key; private halves in the OS keychain (`keyring`);
  - a **signed, hash-chained membership log** per team: clients accept only keys the log vouches
    for, which is the actual protection against a server that substitutes keys;
  - a random 256-bit team key per epoch, wrapped to every device with HPKE (RFC 9180, crate
    `hpke` 0.15, MIT/Apache-2.0);
  - records sealed with XChaCha20-Poly1305 (team, epoch, record id and sender as associated data)
    and signed by the sending device; shared transcripts get their own key per session;
  - a new epoch on every leave and device removal, plus a periodic rotation. Removed members keep
    what they already saw; the UI says so.
- **New device:** an existing device shows a short code, approves, signs the log entry and wraps
  the current key. **Recovery:** a printable recovery key that acts as an offline device; no
  escrow by admins or the server.
- The server still sees the member and device list, epochs, times, sizes and who talks to whom.
- A hand-built protocol needs a threat model, test vectors and an external crypto review before
  launch.

## 4. Server

- One Rust binary: axum 0.8 + tokio, **WebSocket** as the only live channel (client messages need
  acknowledgements), JSON envelopes with a protocol version, ping every 25 s.
- Every team event gets a rising sequence number; after a reconnect the client sends its last
  number and the server replays or asks for a snapshot. Client intents carry an operation id so
  retries are idempotent. Reservations are never applied optimistically: "first wins" is decided
  by the server.
- **Exclusivity without reading content:** clients send keyed hashes (team-derived HMAC key) of
  task ids and path prefixes; the server compares them for equality only.
- Storage: SQLite (WAL) via sqlx 0.9 for self-hosting and for the hosted service in version 1;
  Postgres only when several nodes are needed.
- Login: OAuth device flow (the app shows a code, the browser confirms), magic links, later
  passkeys and OIDC (GitHub, self-hosted forges). Opaque, hashed, rotating tokens; in the app in the keychain.
  Self-hosting works without mail through admin invite links.
- Seats: joining a team is one transaction that checks the seat count. License keys are signed
  documents (Ed25519, `ed25519-dalek`, BSD-3-Clause) with a key id so the signing key can rotate;
  on expiry nothing is deleted, only joins above three are blocked.
- Notifications in version 1: inside Hangar and as desktop notifications over the live channel.
  Mail, chat and phone push carry no content ("something needs your attention in team X").
- Packaging: static musl binary, distroless image for amd64 and arm64, one data volume, Caddy in
  front for TLS. Cost per free team of three is in the cent range per month on a small VPS (an
  estimate, not a load test).
- Design lessons: control plane and data plane stay separate (Git carries the code), the app
  degrades to "warn only" when the server is unreachable, and client and server share one
  protocol crate.

## 5. Build plan

Each step is useful on its own and ships behind a setting until the team parts exist.

| Step | Content | Needs a server |
| --- | --- | --- |
| A | **Local claims**: watcher plus git rescan per session worktree, a claims view, overlap warnings between the person's own sessions (two agents on one machine already collide today) | no |
| B | **Agent gate**: local http hook endpoint, `--settings` injection for Claude Code, pause with "Allow once / Wait / Stop" in Hangar, catalog capability flags, watcher fallback for the others | no |
| C | **Protocol crate** (MIT, in this repository): message types, envelopes, keyed hashes, state machines for reservation and expiry, tests | no |
| D | **Team server** (separate repository, Elastic License 2.0): axum, SQLite, device flow login, teams, invites, seats, live events, replay | yes |
| E | **Team crypto**: device keys, membership log, epochs, HPKE wrapping, recovery key, test vectors, external review | yes |
| F | **Team UI in Hangar**: sign in, create or join a team, live status of everyone's agents, task list with reservation, claims of others, pause on overlap with a release request, shared decisions as context at start | yes |
| G | Later: merge queue, issue import, shared transcripts, phone push, chat, peer to peer | yes |

Shipping the team feature needs macOS and Linux builds (ADR 0005, consequences).

## Still to verify by measurement
- Claude Code http hook: does a connection failure fail open or closed; do sub-agent tool calls
  pass `PreToolUse`.
- Codex: path shape of `apply_patch` input; Gemini: hook merging with
  `GEMINI_CLI_SYSTEM_SETTINGS_PATH`; Copilot: per-session hooks.
- Cost of `git status` and `git diff -U0` on a large repository on Windows.
- Exact versions of `chacha20poly1305` and the dalek crates before pinning.
