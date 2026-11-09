---
title: "Team service version 1: research results and build plan"
type: synthesis
tags: [collaboration, teams, agents, server, e2e, git, worktree, hooks, claims]
sources: ["[[claims-rescan-measurement]]", "[[claude-http-hook-measurement]]"]
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

## 4a. Accounts and invitations (owner's decision, 2026-11-09)

- Hangar gets a sign-in that is bound to a team server (official or self-hosted). The account
  tells Hangar which teams the person belongs to; without signing in Hangar stays fully usable
  alone.
- Sign-in from the app: the app opens the browser, the person confirms with a magic link sent
  by mail (later passkeys, GitHub or other OIDC providers); the app receives its token through
  the device flow and keeps it in the OS keychain.
- People join a team **only by invitation to an e-mail address**:
  1. a team admin enters one or more addresses in Hangar and picks a role;
  2. the server sends a single-use, expiring invitation mail; the seat limit is checked when the
     invitation is accepted, not when it is sent;
  3. the link opens a page that starts Hangar (or offers the download), the person signs in or
     creates the account with that address; the address must match;
  4. before the team key is shared, an existing member's device confirms the new device
     (short code comparison for admins who want it); then the person sees the team.
- Invitations can be resent and revoked; pending ones show in the team view.
- **Mail in Hangar style:** one shared template set for invitation, sign-in link, "invitation
  accepted" and "something needs your attention" mails: Hangar logo and colors from the design
  tokens, light and dark variants, table layout with inline CSS that works in Outlook, Gmail
  and Apple Mail, an equivalent plain-text part, no remote images or tracking pixels, all texts
  in the 12 app languages, the button link also as plain text. Mails carry no task or file
  content (end-to-end encryption). Sender authentication (SPF, DKIM, DMARC) for the official
  service; self-hosted servers use their own SMTP or, without mail, admin-copied invitation
  links.

## 5. Build plan

Each step is useful on its own and ships behind a setting until the team parts exist.

| Step | Content | Needs a server |
| --- | --- | --- |
| A | **Local claims**: watcher plus git rescan per session worktree, a claims view, overlap warnings between the person's own sessions (two agents on one machine already collide today) | no |
| B | **Agent gate**: local http hook endpoint, `--settings` injection for Claude Code, pause with "Allow once / Wait / Stop" in Hangar, catalog capability flags, watcher fallback for the others | no |
| C | **Protocol crate** (MIT, in this repository): message types, envelopes, keyed hashes, state machines for reservation and expiry, tests | no |
| D | **Team server** (separate repository, Elastic License 2.0): axum, SQLite, device flow and magic link sign-in, teams, e-mail invitations with the Hangar mail templates, seats, live events, replay | yes |
| E | **Team crypto**: device keys, membership log, epochs, HPKE wrapping, recovery key, test vectors, external review | yes |
| F | **Team UI in Hangar**: sign in, create or join a team, live status of everyone's agents, task list with reservation, claims of others, pause on overlap with a release request, shared decisions as context at start | yes |
| G | Later: merge queue, issue import, shared transcripts, phone push, chat, peer to peer | yes |

Shipping the team feature needs macOS and Linux builds (ADR 0005, consequences).

### Step A as built (2026-11-09)

Local claims, behind the setting "Detect overlaps between sessions" (on by default). Measurement
and timing: [[claims-rescan-measurement]].

- **Backend:** `src-tauri/src/claims.rs` (pure: porcelain v2 and `-U0` hunk parsing, claims,
  overlap levels, coalescer with injected time) and `src-tauri/src/claims_service.rs` (one worker
  thread, one notify 8.2 watcher per distinct worktree, rescans through `Git::run`).
- **Which sessions:** running agent sessions of the session manager (no update sessions, no
  exited ones). Each folder is resolved once with `rev-parse --show-toplevel --git-dir
  --git-common-dir`; sessions in one worktree share its watcher and its rescan. The folder key
  is cached per session folder, so a hung network path is not resolved every second.
- **Work tree check:** before anything is resolved, watched or scanned, the reported work tree
  and both git folders must not lead from the session folder (as named or resolved, so a mapped
  network drive may name its own share; unverified that Git for Windows prints the share there,
  not measured on a real share) to another share or device, the work tree
  must contain the session folder and must hold a `.git` (`git::check_work_tree`). A repository
  whose own config moves the work tree (`core.worktree`, for example to `C:\` or a share) is
  refused with a clear message; the Changes panel and worktree creation use the same check.
  git itself still reads that config while answering `rev-parse` (not preventable without
  parsing the config ourselves).
- **Rescan:** filter overrides, `status --porcelain=v2 -z --branch --no-renames`, and only with
  a base commit and changes `diff -U0 --no-ext-diff --no-textconv --no-renames HEAD --`. Hunk
  headers only; a pure insertion after line `a` counts as lines `a` to `a + 1`. Whole file: new,
  deleted, untracked, conflicted, binary, more than 64 hunks, no hunks (mode only), no base, or
  past the 4 MiB output limit. Never file contents.
- **Keys:** repository = git common folder, worktree = top folder, both resolved, `/`
  separators, lower case on Windows and macOS. File paths compare case-insensitively there too.
- **Overlap levels:** `none`, `sameArea` (same first folder, no shared file in it), `sameFile`,
  `overlappingLines` (same base commit and ranges within 3 lines). Different bases: file level
  only. Root files have no area.
- **Shared folder:** sessions in one worktree are never compared with each other; each lists the
  others in `sharedWith` and the panel says the changes belong to the folder.
- **Watcher:** recursive on the worktree; inside a git folder only `HEAD`, `index` and
  `logs/HEAD` count (commits, checkouts, staging); a linked worktree's own git folder is watched
  too. Rescan flag, error or a network folder: poll only. A poll that finds unreported changes
  restarts the watcher (silent unwatch after an overflow on Windows). Symlinks are not followed
  (`Config::with_follow_symlinks(false)`). notify 8.2.2 reports the inotify limit (ENOSPC) as
  `ErrorKind::MaxFilesWatch` from `watch` or through the handler (`src/inotify.rs`, read in the
  crate source); both switch the worktree to poll only at once and drop the watcher.
- **Ignored folders:** with every scan the worker asks `git check-ignore` which top-level folders
  are ignored (`target`, `node_modules`); their events are dropped, so a build no longer rescans
  every few seconds. A changed `.gitignore` or a new folder is an event that leads to the next
  scan and refresh. Tracked files below an ignored folder are found by the poll. The recursive
  watch itself still covers those folders (on Linux each costs inotify watches).
- **Timing:** 750 ms quiet, 3 s maximum wait, gap ten times the last rescan's cost (1 to 30 s),
  safety poll 60 s (30 s without a watcher). One worker: at most one git call of the claims at a
  time. A budget across all worktrees: after a scan that took `t`, no scan for `3 t` (at most
  30 s), the most overdue worktree first. The snapshot is rebuilt only when a sync or scan changed
  something.
- **Removal:** `remove_worktree` checks the record, then `ClaimsService::release` closes the
  worktree's watcher and waits for a running git call (scan or resolve, both under one gate)
  before git removes the folder; no watcher, scan or resolve starts there again while a session
  still names it. When the removal fails or keeps the worktree, `ClaimsService::unrelease` gives
  it back and its sessions keep their overlap warnings.
- **Interface:** commands `configure_claims(enabled)` and `claims_snapshot()` (main window only),
  event `claims-changed` with `ClaimsSnapshot { enabled, sessions: SessionClaims[], overlaps:
  Overlap[] }`; each overlap is listed from both sides (`sessionId`, `otherId`), at most 200 per
  session (worst first), with `overlapCount` and `overlapsTruncated` per session. Types mirror in
  `src/claims.ts`.
- **UI:** a warm overlap icon in the session row and, in the collapsed rail, in the badge's top
  left corner (tooltip and screen reader text "Same file as <session>: <path>", file levels
  only); in the Changes tab a warning block with the overlaps in pages of 20 and "Show session",
  a note when overlaps past the limit are not listed, a shared folder note, and a collapsible list
  of changed places with line ranges; paths, areas and git's messages show invisible characters
  as placeholders; a notice in the window and, in the background, a desktop notification under the
  existing notification settings, once per session pair and file per app run.
- **Not built yet:** configurable areas (first folder only), committed but unmerged changes on a
  session branch (only uncommitted changes against HEAD count), stage 2 with `git merge-tree`.

### Step B as built (2026-11-09)

Agent gate, behind the setting "Pause agents before editing a file another session is changing"
(on by default, needs "Detect overlaps between sessions"). Measurements:
[[claude-http-hook-measurement]], [[claude-settings-file-measurement]]. The gate coordinates
the user's own sessions and is no security boundary: edits through the shell, sessions inside
WSL, managed settings that forbid the hook, a body over the limit and every failure of Hangar
let the edit through. The setting's help text says so.

- **Catalog:** `capabilities = { pre_edit_block, context_inject, per_session_config }` per entry,
  all false by default; Claude Code has all three, Codex, Gemini, Copilot and Aider are false
  with a comment pointing here. The gate goes only where `pre_edit_block` and
  `per_session_config` are true, in the one format built so far (Claude Code's `--settings`). A
  user profile with the same id and program inherits the flags.
- **Launch:** `SessionManager::spawn` asks the `LaunchGate` (`gate_service.rs`) on every start of
  an agent session (first start, restart, resume, resume fallback; never update sessions). When
  the gate is on, it appends `--settings <inline JSON>` (http hooks: `PreToolUse` with matcher
  `Edit|Write|MultiEdit|NotebookEdit`, timeout 660 s, and `UserPromptSubmit` for context,
  timeout 10 s) as argv and sets `HANGAR_HOOK_TOKEN` in the child's environment after the profile
  variables; an inherited `HANGAR_HOOK_TOKEN` is removed from every child. A `.cmd` launcher
  (npm's `claude.cmd`) cannot take the inline JSON (Hangar refuses `"` in its arguments,
  `batch.rs`), so it gets `--settings <file>`: the same JSON, without the token, in
  `<local app data>/gate/claude-hooks-<port>[-context].json`, written whole under a temporary
  name and renamed; files of earlier runs are deleted when the endpoint starts, except those whose
  port is still listened on (another Hangar sharing the folder). Measured to work
  with a path containing spaces ([[claude-settings-file-measurement]]). When that path holds a
  character cmd.exe would interpret, or the file cannot be written, the session starts without
  the gate. `SessionInfo.gate` tells the window: `none` (not for this program, gate off, update
  session), `active`, `noEndpoint` (the endpoint did not start), `launcher` (no settings file) or
  `profileSettings` (the start arguments of the profile or the resume already hold `--settings`
  or `--settings=`); the last three show as a quiet muted marker next to the session's title and
  a line in its tooltip. Whether Claude Code merges two `--settings` or keeps only one is not
  measured, so Hangar adds no second one and that session runs without the gate (neither the
  argument nor the token, nothing registered); the user's own settings stay untouched. Sessions
  inside WSL cannot reach the endpoint on the Windows loopback and get no gate (WSL entries have
  no capabilities).
- **Restore order:** the window's restore waits until the first `configure_gate` has settled
  (at most 3 s, `gateSetup` in `src/gate.ts`), so restored Claude Code sessions do not start
  before the backend knows the gate is on.
- **Endpoint:** `hook_http.rs`, a hand-written HTTP/1.1 reader with explicit limits (head 16 KiB,
  64 headers, body 32 MiB, no chunked bodies, no second `Content-Length`, no folded headers, only
  `POST`) on 127.0.0.1 with a random port, started with the first gated launch. One thread per
  connection. Time limits are totals, not per read: a reader sets the socket's read timeout to
  the time left before every read (head 5 s, body 30 s, reading after the answer 2 s, each write
  5 s), so a client sending a byte now and then is cut off. Until the token is known a
  connection holds one of 8 slots; a known session moves to one of 64 (then 503), so slow
  unknown clients cannot take the slots of held requests. The accept thread never reads or
  writes: a connection over the 8 is closed unanswered at once, and Claude Code goes on as with a
  failed hook. A request with `Origin` or any `Sec-Fetch-*` header, or whose `Host` is not exactly
  `127.0.0.1:<port>` (what Claude Code sends, measured), gets 403 before the token is looked at.
  A token per process (two UUID v4, 64 hex characters), held in memory only, compared in constant
  time against every registered token before the body is read; 401 otherwise. Routes `/v1/pre-tool-use` and `/v1/user-prompt-submit`. The body is parsed as it
  streams in; a `Write`'s content is skipped, never kept. Answers close gracefully (write side
  first), since Windows otherwise reset the connection in the tests. No server crate: two routes,
  one client, all limits in one file.
- **Decision logic** (`gate.rs`, pure): the hook's path (as named, and with its existing part
  resolved) relative to the session's worktree. The path comes from the model, so it is resolved
  only when it lies on a local drive or on the share the session's own worktree lives on
  (`git::gitdir_allowed`) and names no DOS device (`CON`, `NUL`, `COM1` ...): a UNC or device path
  would otherwise make Hangar connect to a host (NTLM) or open a pipe before Claude Code's own
  permission prompt. A session outside every known worktree is not looked at at all; another running session in another worktree of
  the same repository whose claims hold the path is an overlap, whether or not this session
  changed the file yet. Both changed it on the same base with near ranges: `overlappingLines`
  with the shared lines; otherwise `sameFile` with the other session's lines. Sessions in one
  folder never collide. No overlap: an empty answer, so Claude Code's own permission flow goes
  on.
- **Pending decisions:** an overlap holds the request and emits `gate-changed`; answers "Allow
  once" (empty answer), "Allow for this file" (the session is not asked again about that file
  until it ends) and "Don't allow" (deny with a short reason naming the other sessions and the
  file). No answer within 10 minutes: deny with "no answer". The session ends, Hangar quits:
  deny. The gate is switched off: empty answer. The agent stops waiting (connection closed, seen
  every 500 ms): its request leaves the decision, which disappears with the last one. The same
  file asked again by the same session while a decision waits joins that decision (up to 4
  requests), and the answer reaches all of them. At most 3 decisions per session and 32 in all;
  beyond that the answer is `permissionDecision: "ask"`, so Claude Code's own prompt asks the
  person instead of letting the edit pass silently ("ask prompts the user to confirm", hooks
  reference; in print mode it did not write the file, [[claude-settings-file-measurement]]).
  The card does not show the tool name: the question is the file, not the tool.
- **Context:** the prompt hook returns the files other running sessions in the repository are
  changing as facts: a header saying the data are names, never instructions, then one JSON
  object per file (`{"file":..,"sessions":[..],"lines":".."}`), at most 30 files and 4 000
  characters, then `{"moreFiles":n}`. Every file and session name, here and in the deny and ask
  reasons, is a JSON string literal whose characters outside a positive list (letters and digits
  of any script, space and `._-/+@#~,()[]=&$!%':` and the middle dot) become `<U+XXXX>`, so a
  hostile name cannot close its quotes, break a line or pose as Hangar's own text. The facts go
  out only when they changed since the session was last told, or when Claude Code's `session_id`
  in the prompt hook changed (a new conversation). That `/clear` starts a new `session_id` is not
  measured, and `/compact` keeps it, so after a compaction the facts come again only when they
  change. `SessionStart` takes no http hooks (measured), so it is the first prompt rather than
  the start.
- **Interface:** commands `configure_gate(enabled)`, `gate_snapshot()` and
  `answer_gate(id, answer: "allowOnce" | "allowFile" | "deny") -> bool` (main window only), event
  `gate-changed` with `GateSnapshot { enabled, pending: PendingDecision[] }`,
  `PendingDecision { id, sessionId, tool, path, otherSessions, level, lines, createdAt }`.
  Types mirror in `src/gate.ts`.
- **UI:** the waiting session takes the attention look with the reason `gate` (sidebar pulse,
  status "needs your decision", chime, taskbar and desktop notification once per decision under
  the existing settings); a card at the top of the waiting session's pane (floating at the top of
  the session area when that session is not on screen) with "Allow once", "Allow for this file",
  "Don't allow", "Show other session" and a count of further waiting decisions. It never takes
  the focus itself; Ctrl+Shift+A brings it there, Escape returns to the terminal without
  answering.
- **Fails open on purpose:** Hangar gone, endpoint refused, Claude Code's own hook timeout, or
  managed settings that forbid the hook (`allowedHttpHookUrls`, `allowManagedHooksOnly`) let the
  edit through; the step A warning remains. A local program running as the same user can read a
  session's environment and with it its token; it can then only ask questions for that session.
- **Not built yet:** the gate for other agents, the gate for Claude Code inside WSL, the "Wait"
  and "Stop" answers of the plan (deny covers both for now). A local program of the same user can
  hold the 8 unknown-client slots and so make new hooks fail open; accepted, since such a program
  can also edit the files itself.

### Step C as built (2026-11-09)

Protocol crate `crates/hangar-team-protocol` (MIT), described in [[team-protocol]]. Not wired
into the app yet; the separate relay will use it as a path dependency.

- **Standalone, no workspace:** its own `Cargo.toml` (with an empty `[workspace]` table) and
  `Cargo.lock`. A workspace at the repository root would move `src-tauri/target` to `target/` and
  change the paths `tauri build`, the release tool, CI and the dev instance rely on; a workspace
  under `src-tauri` would make the relay build Tauri's tree. CI runs its fmt, clippy and tests as
  a separate step.
- **Dependencies:** serde, serde_json (with `raw_value`), thiserror (already in the app), hmac 0.13
  and sha2 0.11 (RustCrypto, MIT/Apache-2.0, both already in the app's lock file through Tauri,
  with their `zeroize` features), hkdf 0.13 (RustCrypto, MIT/Apache-2.0, in the app's lock file),
  zeroize 1 (MIT/Apache-2.0, in the app's tree) and unicode-normalization 0.1 (unicode-rs,
  (MIT OR Apache-2.0) AND Unicode-3.0 for its data tables, a documented license exception like
  `notify`'s CC0: Unicode-3.0 is permissive and only asks that its notice travels with the data,
  and unicode-ident in every Rust build carries it too; its one dependency tinyvec is in the
  app's tree). No async runtime, no I/O, no clock.
- **Content:** `wire` (envelope and all messages of section 4, `deny_unknown_fields` on every
  struct, version checked before the body, limits), `hash` (matching key per epoch, keyed hashes
  of task ids, repositories and folder levels, normalization), `reservation` (state machine and
  the relay's authority, timing from ADR 0005), `overlap` (same area and same file by hash
  equality between devices), `log` (sequence numbers, replay window, resync, op id retries),
  `ids` (opaque identifiers, `Ciphertext`, `Epoch`, `Sealed`).
- **Owner's input during the step:** teams are the website's projects, members its users; ids are
  opaque strings; `hello` carries the website's Ed25519 ticket unchanged and the relay verifies
  it; roles owner, admin, member.
- **Decisions beyond the plan:** repository identity = root commit (no URL normalization); case
  folding per repository for the whole team (default on); `task.rekey` after a key rotation;
  presence and overlaps are not logged; overlapping lines are refined by clients from the
  encrypted detail; one device's sessions are left to step A.
- **Rework after the acceptance and security reviews:** time events of a book operation are
  returned apart from the intent's outcome (`Applied`), so a refused intent no longer swallows a
  warning or an expiry, and every operation applies time to all entries first (a rekey onto an
  expired hold works); release requests time out (10 min), can be withdrawn
  (`task.release_withdraw`) and are followed by a 10 minute cooldown per task and requester;
  owners and admins can force a release (`task.force_release`); presence counts connections;
  `remove_device`; a handover respects the requester's reservation limit; rekeying needs an
  old-epoch hold and a new hash; during a 30 minute grace after a rotation `task.reserve` carries
  the task's old hashes (`prev`) so an offline holder keeps the task; `staleEpoch` error code and
  `check_epoch` for every epoch-bound payload; separate matching and sealing keys per epoch via
  HKDF; NFC, the case rule in the hash input and refusal of format and separator characters;
  only the first folder hash per file (`area`) instead of every level; the encrypted claim detail
  is passed on in `claim.overlap`; overlaps compare member and device; op retries keyed by
  sender, team and op id with a body digest; the version is read before the envelope; bodies are
  parsed straight from raw JSON (duplicate keys refused); a client frame without a team is never
  built; task ids and paths never show in `Debug`. A threat model section is in [[team-protocol]].
- **Additions for the relay (protocol version stays 1, nothing is deployed):** the server side
  of section 3 without cryptography (module `e2e`): `device.keys`, `devices.get`/`devices`, the
  membership log (`memberlog.append`, `memberlog.get`, `memberlog.entries`, logged
  `memberlog.appended`) with a domain-separated SHA-256 chain over index, `prev` and entry, test
  vectors and `MemberChain` for both sides, and `key.rotate` (owner or admin, epoch exactly
  current + 1, `check_rotate`), `key.wrap` and `key.get`; limits `MAX_WRAPPED_KEYS` (512) and
  `MAX_LOG_ENTRIES` (256). `key.rotated` reaches each device with only its own wrapped key, also
  on replay. Also `member.role_changed`, presence `offline` (server only), the op id of `hello`
  on `welcome` and `resync_required`, the deduplication rule (`ClientMessage::deduplicated`),
  `notify` as server-originated only, and the relay's WebSocket reader as the first size check.
  Names follow the relay's draft; field types are the crate's checked types instead of strings.
- **Not built yet:** the relay (step D), the crypto behind `Sealed` (step E), the app side
  (step F).

### Step E as built (2026-11-09)

Crypto crate `crates/hangar-team-crypto` (MIT), described with its decisions and threat model in
[[team-crypto]]. Pure (no I/O, no clock, no keychain); not wired into the app yet, step F part 2
uses it. Needs an external cryptographic review before it protects real data.

- **Standalone like the protocol crate:** own `Cargo.toml` with an empty `[workspace]` table, own
  `Cargo.lock`, path dependency on `hangar-team-protocol`. CI runs fmt, clippy, tests and the
  Python reference as a separate step on all three systems.
- **Modules:** `identity` (Ed25519 and X25519 device keys, versioned keychain format, safety
  numbers per device and per log head), `memberlog` (canonical signed entries bound to team,
  index and predecessor; `LogVerifier` into a `TeamState`), `epoch` (random secret per epoch,
  commitment in the log, HPKE wrapping, history link), `rotation` (`memberlog.append` plus
  `key.rotate`, `key.wrap` for one device), `record` (`Sealed` content: XChaCha20-Poly1305 plus
  the sender's signature, `ReplayGuard`), `recovery` (128-bit code in Crockford base32 with a
  checksum, derives an offline device).
- **Dependencies:** `hpke` 0.15 (default features off), `ed25519-dalek` 3, `chacha20poly1305`
  0.11, `hkdf` 0.13, `sha2` 0.11, `getrandom` 0.4, `zeroize`, `thiserror`, `data-encoding`; all
  MIT, Apache-2.0 or BSD-3-Clause, one RustCrypto and dalek generation.
- **Randomness:** through the `Entropy` trait, fallible; `hpke` gets its ephemeral key material
  through a one-shot generator, so a failing generator is an error and never a panic.
- **Hardened after the security review (2026-11-09):** the relay verifies every
  `memberlog.append` with the crate's `LogVerifier::admit` (so the relay now depends on
  `hangar-team-crypto`) and accepts a rotation only as one `key.rotate` carrying the signed
  rotate entry and the wrapped keys (`admit_rotation`); every device in the log carries a proof
  of possession, and an add device entry the endorsement of one of the member's devices;
  recovery devices only restore their member and sign no records; any member adds and removes
  its own devices; epoch keys bind the rotate entry's log hash, and records carry the sender's
  log head (fork detection) and a counter (`ReplayGuard` with stored high-water marks); X25519
  keys are validated; `TeamState` keeps a key history per holder; the relay keeps up to four
  wrapped keys per device and epoch. Breaking protocol changes for the relay are listed in the
  protocol crate's README changelog (fourth round).
- **Second hardening round (2026-11-09):** the relay passes the ticket role to `admit`,
  `admit_rotation` and the new `admit_device_remove`, and the issuer acts with the lower of log
  role and ticket role (entry 1 only with an owner's ticket); `device.remove` carries its signed
  remove device entry and the relay removes the device only with it; the relay's `key.wrap`
  check moved into the crypto crate (`relay::check_wrap`: sender and target devices of the log,
  one package per sender, target and epoch) and `check_device_remove` was dropped; a member has
  at most 16 devices and a team at most 512; `relay::check_registered_keys` requires the keys a
  holder registered over its own connection; `ReplayGuard` marks are kept per holder and signing
  key; `Member` shows a re-admission (`readmitted_after`, `added_by`, `added_at`).
- **Tests:** 56 unit tests (whole-team flows where relay and client must agree on every entry,
  every log rule and binding, proofs and endorsements, relay-only refusals, ticket role caps,
  device limits, key history, fork and replay detection, record context, size limits, malformed
  input, RFC anchors) and `test_vectors.py`, which reimplements every primitive with the Python standard library and
  recomputes the crate's 16 vectors.
- **Open for step F part 2:** the app's storage duties (secret before `key.rotate`, send counter,
  replay marks, deleting fetched wrapped keys, wiping a removed device's identity); no persisted
  `TeamState` (clients and relay re-verify the log from entry 1); no ownership transfer; no KEM
  key rotation; the external review ([[team-crypto]], questions for the external review).

### Step F as built (part 1, 2026-11-09)

Sign in to a Hangar account and see its projects. Endpoints, token rules and the client's
behavior: [[hangar-account-api]]. Nothing of the team service shows without signing in.

- **Backend:** `account.rs` (pure), `account_api.rs` (reqwest, 15 s, 512 KiB, no redirects),
  `account_service.rs` (device flow state machine `Idle -> WaitingForUser -> SignedIn | Failed`,
  polling with `interval` and `slow_down`, expiry and cancel; refresh token only in the keychain,
  access token in memory, single-flight refresh with rotation; `invalid_grant` signs out with a
  reason). The server address is stored by the backend in `account.json`, because the keychain
  entry is found by it before the window exists. New dependency entry: tokio (`sync`, `time`),
  already in the tree through Tauri and reqwest.
- **Pages:** the confirmation, invite and devices pages open only on the server's own origin; the
  window names the page, never a URL. No revoke endpoint exists, so signing out drops the tokens
  locally and Settings points to `/account/devices`.
- **Window:** Settings, Account (sign in, the code large with copy and "Open browser again", the
  remaining minutes, signed-in name and e-mail with the seats of the own account, sign out,
  server address for self-hosting folded away) and a Team group at the bottom of the sidebar
  while signed in (projects with role, plan, "2 of 3 seats" of the owner's account, pending
  invitations, "Invite people" for owners and admins). One store (`useAccount.ts`) serves both.
- **Tests:** pure rules in `account.rs` (addresses, origins, problems, tokens, polling), the
  service against an in-test mock server on 127.0.0.1 (`tests/account.rs`: whole flow with
  measured poll gaps, decline, expiry, cancel, foreign page, one refresh for two concurrent
  callers, rotation without reuse, `invalid_grant`, problem documents, invite pages, sign-out,
  server switch, relay ticket), Vitest for `src/account.ts`.
- **Not built yet:** creating projects and inviting from inside
  Hangar (the website does it), the Team entry in the collapsed rail, reloading projects on a
  timer.

### Step F as built (part 2a, 2026-11-09)

Team connection: a signed-in person turns on "Share with team" for a project in the sidebar's
Team group; Hangar then keeps one WebSocket to the team relay for it, verifies the membership log
and holds the current epoch's secret. Teammates' status and claims are part 2b. Not run against a
real relay yet; tested against an in-test relay built from both crates.

- **Wiring:** both crates are path dependencies of `src-tauri` (no workspace, which would move
  `src-tauri/target` that the release tool, CI and `tauri build` use); the release tool's lock
  rewrite matches only the `hangar` package. New dependencies: `tokio-tungstenite` 0.29 with the
  handshake feature only (adds `tungstenite`, `sha1`, `rand` 0.9), TCP from tokio (`net`), TLS
  from `tokio-rustls` with `rustls-platform-verifier` (both already in the tree through reqwest),
  `futures-util`. No proxy support for the relay yet.
- **Modules:** `team.rs` (pure: the window's view, reconnect policy, connection clock, stored
  formats), `team_engine.rs` (the protocol without I/O; returns ordered effects), `team_relay.rs`
  (socket, TLS, handshake, 1 MiB message limit), `team_service.rs` (one worker per project,
  keychain off the async threads), commands in `commands.rs`. Types mirror in `src/team.ts`.
- **Device identity per server and project:** `DeviceIdentity` generated on first use, device id
  16 random bytes in hex; stored in the keychain (service `hangar-team`, debug builds
  `hangar-team-dev`) as `device/<key>`, where `<key>` is a hash of server address and project id,
  before the relay sees it. Never in a file or a log.
- **Epoch secrets and the verified log head** live in the keychain as well (`keys/<key>`, the
  newest eight secrets, under 700 of the 1280 UTF-16 units the Windows store allows). Chosen over
  an encrypted local file: the OS already protects the entry, there is no second key and file
  format to get right, and the head's integrity matters as much as the secrets' confidentiality.
  The event log cursor, the send counter, the replay marks and rejected requests (nothing secret)
  are in `<local data>/team/<service>/<key>.json`, the enabled projects in `<config>/team.json`
  under `enabledByService.<service>`: debug and release builds have separate keychain services,
  so they are separate devices and must not share cursor, counter or marks. A missing device entry
  creates a new device; one that is there but does not decode stops with "keychain" and is never
  overwritten. A state file that exists but does not parse stops with `storage` instead of
  starting empty.
- **Send counter:** stored before its record, and on every start raised to at least Unix
  milliseconds times 1024, so a lost or restored state file cannot repeat a counter (it would
  take 1024 records in one millisecond, or a clock set back while the file is lost).
- **One window per team:** a worker holds `<local data>/team/<service>/<key>.lock` (an OS file
  lock) while it runs. A second Hangar window shows `needsAttention { otherInstance }`, retries
  every 5 s and takes over once the first one ends.
- **Order of storage:** the engine returns effects in order and a failed keychain or state file
  write drops everything after it (the keychain stops with "keychain", the state file ends the
  connection), so a rotation's secret is stored before `key.rotate` leaves, and a record counter
  before its record (`reserve_counter`, ready for part 2b). A keys entry that no longer decodes
  stops with "keychain" instead of starting without the verified log head.
- **Connection:** a fresh ticket (`relay_ticket`) for every connection, `hello` with ticket,
  device, protocol range and the stored cursor; `welcome` or `resync_required`; `ticket.renew`
  with a fresh ticket every 5 minutes; a ping every 25 s; nothing heard for 60 s counts as lost;
  a reconnect of its own at 540 s, before the relay's 600 s connection age; the cursor is stored
  after every logged event, so a reconnect replays instead of resyncing. One worker and one
  connection per project; quitting sends close 1000 and waits up to 2 s.
- **Reconnect policy:** 4401 a fresh ticket at once; `err forbidden` on `hello` one immediate
  retry with a fresh ticket, a second one stops (`refused`); 4403 stops with "removed" and keeps
  the device's keys; 4404 asks the website first: only when the project is gone there does it
  stop with "project deleted", delete the team's keychain entries and state file and turn sharing
  off (the website still listing it stops with `refused`, keys kept; no answer backs off and
  retries); 1008 and every other ending back off with equal jitter from 1 s doubling to 60 s.
  One immediate reconnect is available only after a connection settled (its log check finished,
  or it lasted 30 s), not after a mere `welcome`, so a relay that accepts `hello` and keeps
  closing cannot make Hangar spin. Frames are read into a queue of 32; a full queue stops reading
  the socket. A log page with `more` and no entry ends the connection.
- **Bootstrap on every `welcome`:** `device.keys` with the proof, then the whole log through
  `memberlog.get` (paged) into a fresh `LogVerifier`. A refused entry, or a log that no longer
  contains the stored head at its index (fork, rewind, rewrite), stops with "the team's history
  does not match". Then: an empty log and the website role owner creates the team (entry 1 with
  this device) and the first epoch; a device not in the log waits for confirmation and shows its
  safety number (also a member while the owner has not shared yet); a device in the log without
  the epoch's secret takes the wrapped key from `key.rotated`, else `key.get`, through
  `unwrap_any` against the log's commitment; a secret that does not match the commitment (its own
  plan of a rotation another device won) is dropped. An owner or admin device (the lower of log
  and website role) rotates when no epoch exists or the log says a rotation is due, without a
  history link; devices whose key could not be wrapped are handed it with `key.wrap` later.
- **Confirmations:** devices that sent `device.keys` but are not in the log come from
  `devices.get` (every 30 s and after each log sync). Another device of the same person becomes
  an add device entry endorsed by this device; for an owner or admin, a new member's first
  device becomes an add member entry with role member. Only entries the verified state accepts
  are offered. Confirming appends the entry and then wraps the current key to the device
  (`key.wrap`, which the relay answers with `key.wrapped` to that device). Rejecting hides the
  request for good (stored). The person compares the safety numbers on both screens.
- **Removal and trust of relay events:** the relay's events are not signed. `device.removed`
  and `member.removed` only start a log fetch; the keys go when the verified log removed this
  device, or (for `member.removed` naming this member, and 4404) when the website no longer lists
  the project. `member.role_changed` only rereads the project list; the role comes from the
  website. Every wipe also turns sharing off, so the next start does not register a new identity
  unasked. `remove_member` and `remove_device` exist in the service (removal through
  `device.remove` with its signed entry) but are not commands yet.
- **Account changes:** the team service subscribes to the account (`AccountService::subscribe`).
  A sign-out (or a sign-in the server ended) stops the running teams at once with "signed out",
  instead of at the next ticket renewal; a sign-in restarts them; a server change stops and
  forgets the old server's workers and starts the new server's enabled ones.
- **Interface:** commands `team_status(projectId)`, `team_enable(projectId)`,
  `team_disable(projectId)`, `team_pending_confirmations()`, `team_confirm(request)`,
  `team_reject(request)` (main window only); event `team-changed` with `TeamSnapshot { teams:
  { projectId, status }[], pending: PendingConfirmation[] }`; `TeamStatus` is `off`,
  `connecting { retrying }`, `connected { epoch, members, devices }`, `waitingForConfirmation
  { safetyNumber }` (six groups of five digits), `needsAttention { reason }` (`historyMismatch`,
  `signedOut`, `keychain`, `needsUpdate`, `refused`, `storage`, `otherInstance`), `removed`,
  `projectDeleted`; `PendingConfirmation` carries `readmitted` (the log removed this member
  before); errors `{ kind, detail }`.
- **UI:** a "Share with team" switch per project in the Team group with the state below it and,
  while waiting, the safety number in groups; a "Confirm new device" panel that shows one request
  at a time ("1 of N"), with its safety number, a warning for a re-admitted member, Confirm and
  Reject disabled for 1 s whenever the shown request changes, focus on the panel after an answer
  (never on a Confirm button; after the last one on the Team group's heading), and Reject asking inline first. The account API offers no member
  names, so the member shows as its shortened id.
- **Tests:** unit tests for the reconnect policy (close codes, the one retry after a stale
  ticket, backoff and jitter, one immediate reconnect per `hello`), the connection clock (renewal
  every 5 minutes, the age reconnect, pings, silence) and the stored formats; engine tests against
  an in-memory relay (team creation with the secret stored before `key.rotate`, a waiting member,
  an endorsed second device, an added member, a rejected request, rotation after a removal, a
  rewritten history, `hello` errors, resume cursor and gap); an integration test
  (`tests/team.rs`) with real WebSockets against a relay on 127.0.0.1 that uses `EventLog`,
  `LogVerifier::admit`, `admit_rotation`, `admit_device_remove`, `check_wrap` and
  `check_registered_keys`, and the account service against a mock account API: the owner creates
  the team, a second device of the same member joins and gets the key, a second member is added,
  a removal rotates and wipes the removed device, reconnect with resume (no resync), ticket
  renewal, a stale ticket retried once, 4401, 4403, 4404 with and without the website agreeing,
  unreadable keys entry, device entry and state file, relay removal events that wipe nothing, a
  refused keys write that sends no `key.rotate`, a failed state write that ends the connection, a
  rewritten history, sign-out and sign-in, a server change and a second window. Engine tests add
  a partial rotation completed with `key.wrap`, the re-admission mark, the bounded wrapped keys
  of `key.rotated` and the counter floor.
- **Not built yet:** teammates' presence and claims (part 2b), removing devices and members from
  the UI, the "added by" note for current members, granting history on a rotation, comparing log
  safety numbers in the UI, the recovery code, a proxy for the relay. A device removed with
  `device.remove` keeps its keys (4403 follows at once and the log fetch is not answered) until
  the person turns sharing off.

## Still to verify by measurement
- Claude Code http hook: a connection failure and a timeout fail open (measured,
  [[claude-http-hook-measurement]]); still open whether sub-agent tool calls pass `PreToolUse`
  and how the interactive mode behaves while a hook waits.
- Codex: path shape of `apply_patch` input; Gemini: hook merging with
  `GEMINI_CLI_SYSTEM_SETTINGS_PATH`; Copilot: per-session hooks.
- Cost of `git status` and `git diff -U0` on a large repository on Windows: measured, see
  [[claims-rescan-measurement]]. macOS and Linux still open.
- Exact versions of `chacha20poly1305` and the dalek crates before pinning.
