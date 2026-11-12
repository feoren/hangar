---
title: "Team service version 1: research results and build plan"
type: synthesis
tags: [collaboration, teams, agents, server, e2e, git, worktree, hooks, claims]
sources: ["[[claims-rescan-measurement]]", "[[claude-http-hook-measurement]]", "[[relay-interop-measurement]]"]
created: 2026-11-09
updated: 2026-11-12
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
  after every logged event, so a reconnect replays instead of resyncing (since the task
  reservations of part 2b, the first `hello` after an app start sends no cursor and takes the
  relay's snapshot; later reconnects of the same engine resume). One worker and one
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
- **Not built yet:** teammates' presence and claims (part 2b, built since), removing devices and
  members from the UI, the "added by" note for current members, granting history on a rotation, comparing log
  safety numbers in the UI, the recovery code, a proxy for the relay. A device removed with
  `device.remove` keeps its keys (4403 follows at once and the log fetch is not answered) until
  the person turns sharing off.

### Step F as built (part 2b, 2026-11-09)

See your team: the teammates' sessions, the files they change, and their collisions with your own
sessions, with the agent gate of step B pausing on a teammate's file, and task reservations. Run
once against a self-hosted relay ([[relay-interop-measurement]]: the live protocol was
compatible, its account API was not; reservations were not part of that run); otherwise tested
against the in-test relays of `team_engine.rs` and `tests/team.rs`, which pass presence on,
compare claims with the protocol crate's `ClaimBoard` and decide reservations with its
`ReservationBook`.

- **Review items of part 2a:** the device identity is bound to the account of its first
  `welcome` (`StoredDevice` format `v2 <device> <member in hex> <identity>`; `v1` entries are bound
  at their next `welcome`). Another account on the same keychain stops with `needsAttention
  { otherAccount }` before connecting (when the account already knows who is signed in) or at
  `welcome`, and touches no key; signing in with the first account again restarts the team. An
  unreadable `team.json` is an error (`TeamConfig::load`), so turning sharing on or off fails with
  `config` instead of writing over the other projects' and the other build's choices. A worker
  holds its team's turn (an in-process lock) for its whole run; a new worker of the same team
  (off and on again, signed out and in) waits up to 3 s for it before it tries the lock file, so it
  no longer shows `otherInstance` for 5 s. The service now acts on the account's current view in
  its subscription, since the account publishes from several threads and a sign-out's view can
  arrive after the next sign-in's. The regression test for the handover passes with and without
  the turn on the test machine (the old worker exits faster than a sign-in takes), so it guards
  the behavior but did not reproduce the original race.
- **Which repository belongs to a project:** the website knows no repository, so the person links
  them in the Team view: a checkbox per repository folder of a running agent session ("Shared
  repositories", stored in `team.json` under `reposByService.<service>.<server>.<project>` with
  the root commit, the clone's git common folder as `claims::folder_key` normalizes it, and the
  folder name). The root commit alone is not enough: every other clone and every fork of the
  same project shares it, so a link by root alone shared a private fork as soon as its upstream
  was linked. The git common folder is the unit (not the worktree top), so every worktree of a
  linked clone, including the ones Hangar creates for agents, is shared without asking again;
  another clone or fork is its own choice, unticked until the person ticks it. Links from before
  folders were bound (`folder` missing) share nothing; the Team view asks to tick the folders
  again when a running folder of such a repository is offered, and ticking one replaces the old
  link. This is the safer migration: binding them to whatever folders run now could pick a fork
  the person never meant. The person can also remove such a link on its own ("Remove old link",
  `team_link_repo` with `folder: null` and `on: false`). A fork whose root commit equals the
  linked one, checked out at the linked folder path, counts as the linked clone by design: the
  team names a repository by its root commit, and the folder is the person's choice. Nothing of an unlinked folder leaves the device; the device's presence
  still goes out (with no sessions), so teammates see it is connected. `team_link_repo` takes
  only a project id the team protocol accepts (`TeamId::new`), links only to a project with
  sharing on (`unknownProject` otherwise), and only a folder of a running session.
- **Repository identity:** the claims worker asks git once per repository
  `rev-list --max-parents=0 HEAD --` through `Git::run` after the first scan with a base commit and
  keeps the smallest root (`RepoId`); `Git::run` sets `GIT_NO_REPLACE_OBJECTS=1`, so a replace
  ref cannot graft another root onto the history; a failure, a timeout or cut output is remembered as unknown
  while the repository is in use (a huge history is not walked again). The cached root is checked
  with every scan (`git cat-file -e <root>^{commit}`, one cheap git call) and found again when the
  commit is gone, and the cache entry goes when no worktree uses the repository any more, so a
  clone replaced at the same path is never shared under the old one's identity. `WorktreeClaims` carries it with the
  folder name and the truncation flag. Its cost on a very large history is not measured.
- **What is shared** (`team_share.rs`, pure): per linked session the agent's program name, the
  session title, its state (working, waiting, idle from the window's attention logic, sent with
  `team_session_states`, 1 s quiet time), the repository's root commit and the changed paths,
  plus the person's display name as the website keeps it (`GET /me` `name`), empty without one.
  The e-mail address never leaves the device; a teammate without a name shows as its short
  member id. Presence status is a
  sealed `PresenceStatus` record (record id empty), at most `RecordKind::max_plaintext` (about
  12 KB): file lists are halved from the largest session until it fits, the rest counted in
  `moreFiles`. Each session with changes sends `claim.set`: the keyed hashes of repository, area
  and path (case rule fold, paths the protocol refuses are left out and the set marked truncated,
  at most 1000 files) and a sealed `ClaimDetail` (record id: the session key) with name, agent,
  title, base commit, paths and line ranges, cut to fit. Session keys are random per session for
  the engine's lifetime. Nothing is sent again unless its content changed (a digest per session);
  after every `hello` and on a new epoch everything goes again. The record counter is stored once
  before the frames of one batch.
- **What is shown:** `presence.update` and the detail in `claim.overlap` are opened with
  `EpochCipher::open_verified` against the verified log. Both are live state the relay repeats
  after a reconnect, so they bypass the `ReplayGuard` (which would refuse the second delivery).
  High-water marks take its place (`LiveMarks` in `team_engine.rs`): per teammate device the
  newest status counter shown (not per signing key: a device id admitted again continues its
  counter above the old ones thanks to the clock floor, so its old-key status stays old; marks of
  the first format are read as the highest per device), per device and session key the newest
  detail counter; a record below its mark is ignored, an equal one (the same record again)
  passes. The marks are stored in the team's state file (`presenceMarks`, `detailMarks`,
  `detailFloors`) and are never cleared by the relay's unsigned `offline`, a reconnect or a
  restart, so the relay can no longer bring back an older authentic status or detail. They are
  written at once when the last write is 5 s ago, otherwise with the next state file write for
  any reason (a record counter, the cursor), the next ping (25 s), a new connection and the end
  of a connection (turning sharing off, quitting), always all together: the file holds a
  consistent earlier snapshot, never a dropped detail mark without its floor. A crash loses at
  most that span (up to one ping interval), which only lets the relay show again a record that a
  newer one replaced within it. A mark is a pair: the log index that added the signing key (its
  key period), then the counter. A device id admitted again signs with keys of a later period, so
  its records count as newer even when its clock is behind, and its old keys' records as older;
  marks stored before periods were kept (period 0) compare by counter alone. They stay bounded: marks of devices the verified log never listed go after every
  log fetch (a removed device keeps its marks, the log keeps its key history), and a device keeps at most 64
  session marks (beyond that the oldest goes and raises a floor that unknown sessions must
  exceed). Only records of the current epoch or a newer one are opened. No age limit on top: a
  status is sent only when it changes (no periodic re-seal), so a quiet teammate's newest status
  may be hours old and still current; an age window would need a heartbeat re-seal first. The
  relay still decides what reaches the device and when (accepted risk: delayed, never older than
  shown, never forged). Only devices the verified log vouches for appear, at most one entry per
  device in the log: a
  presence or overlap naming another holder is retried once after a log fetch and then dropped,
  and devices the log no longer lists leave the view after every fetch. A record naming a log
  entry or
  an epoch this device does not have yet is kept (at most 64) and retried once after the next log
  fetch. Since the crypto crate checks the sender's signature before the head's hash, only a
  record a listed device signed over a head this device's log does not have can report a fork
  (`OpenError::Fork`); a record anyone could write (wrong hash, no valid signature) is dropped and
  the connection stays. A signed fork in a live record sets only the sending device aside: its
  presence and overlaps go, its live records are ignored until this device's verified log head
  changes (the device may have caught up, or a later entry settles it), and the Team view says
  "Anna #3f9a2c1b… (device 9c41d0e2…) reports a different team history" (`activity.divergent`).
  The earlier stop of the whole team was dropped: one listed device, faulty or shown another log
  by the relay, could stop everyone's sharing with one record, while this device's own log is
  still the one it verified. The fork may be this device's, though: after a restart the relay
  can show it a signed but withheld entry past its stored head, and then every honest teammate
  looks forked. So each such record fetches the log again, and when devices of two distinct
  members report another history, this device is the odd one out and stops with
  `historyMismatch`. Devices of one member never stop it, even when no other device is heard:
  which devices are heard is the relay's word (unsigned `offline`, a new connection), so a rule
  like "every device heard disagrees" would let one person, or the relay, stop the team. Live
  records detect forks only on a best effort basis (a fork before the epoch's rotate entry gives
  another epoch key, so its records fail to decrypt instead of reporting `Fork`); the own log
  sync is the authoritative check. It also stops when its own log fetch contradicts the
  head it verified before (fork, rewind, rewrite). A fork counts only for a record that also
  decrypts with the current epoch's key and names a head from within the epoch (crypto crate),
  so after the rotation its removal made due, an old key of a re-admitted device id cannot get
  the honest device set aside. The device's marks
  stay: they only refuse records older than ones verified against this device's log. Unknown
  fields or versions in a document are refused; names, titles and lists are cut when read. A
  `presence.update` without a status changes only the state (active, idle, away); the sessions
  stay as last signed. `offline` removes the device and its overlaps.
- **Teammate overlaps:** the relay's `sameFile` is refined locally with step A's rule (same base
  commit, ranges within 3 lines: overlapping lines) from the teammate's detail; a detail that
  names no file of ours wins over the relay's word and leaves at most `sameArea`. Without a detail
  the relay's file hash is matched against this device's own claims to name the file. At most 64
  overlaps are kept per own session and device in the log, so invented session keys cannot grow
  the view. An overlap counts only for a session the teammate's newest signed status lists: after
  `claim.clear` the relay could send the ended session's last detail again, or name a session key
  it made up; a new status drops overlaps of sessions it no longer lists. Without any status from
  that device an overlap shows as `sameArea` at most, without a file, so it raises no notice. Each
  overlap names our session, the teammate, agent and title, the level, our spelling of the file
  and the lines. The window shows the worst one in the session's sidebar marker when no local
  overlap is there, a block "Overlaps with your team" in the Changes tab ("Anna (Codex) is changing
  the same lines: src/auth.rs, lines 10 to 15"), and a notice (and, in the background, a desktop
  notification) once per session, teammate device and file per app run. The remembered
  collisions are capped at 2000 and desktop notifications about teammates go out at most once a
  minute, however many collisions arrive. Names in overlaps, notices and the gate card carry the
  short member id from the verified log (`Anna #3f9a2c1b…`), since any member can send any
  display name; the Team view shows the id quietly next to the name.
- **Gate:** `gate::assess_with_team` also pauses an edit of a file a teammate's live claim covers
  (same root commit, paths compared case folded) in a clone linked to that teammate's project
  (`TeamService::teammate_files(root, folder)`): from the teammates' presence, with their lines
  where a detail arrived. `PendingDecision.teammates` names them on the card next to the user's own
  sessions ("Also being changed by Codex · feature, Anna #3f9a2c1b… (Claude Code)"); a teammate alone is
  enough (`otherSessions` may then be empty). The card names the file as this device spells it
  (its own claim, else the edited path), never in the teammate's spelling. The answers and the deny reasons are those of step
  B, with the teammates' names as JSON literals. The prompt hook's context adds one line per
  teammate file (`{"file":..,"teammates":[..],"lines":".."}`) under the same caps; the file is
  named as this device spells it when one of its sessions changes it or it exists in the worktree
  (each part as the folder lists it, looked up only for lines that can still be listed, within
  50 ms for all lines (checked before every folder read and metadata call), only through real
  folders, never through a symlink, junction or other reparse point, not in a worktree that lies
  below one (`mklink /D C:\w \\host\share` could lead to another computer), and not on
  network folders or, on Windows, drives that are not fixed, such as a mapped `Z:`; the lookup runs before the gate's lock is taken), else in the teammate's spelling cut to 200 characters (`literal` cuts further
  and marks control, invisible and reordering characters, also in the names).
- **Interface:** commands `team_session_states(states: {sessionId, state}[])` and
  `team_link_repo(projectId, root, folder or null, on)` (main window only); `team-changed` adds per team
  `activity { linked: {root, folder or null, name}[], peers: PeerView[], overlaps:
  TeammateOverlap[], divergent: {member, device, name}[] }` and `repos: {root, folder, name}[]` (the running sessions' repository
  folders). `PeerView { member, device, name, own, presence, sessions: {key, agent, title, state,
  files, moreFiles}[] }` (`key`: the session key, unique per device, the row's React key),
  `TeammateOverlap
  { sessionId, member, device, otherSession, name, agent, title, level, path, lines, files }`
  (`otherSession`: the teammate's session key, the row's React key, never shown). New attention
  reason `otherAccount`, error kind `unknownRepository`. Types mirror in `src/team.ts`.
- **UI:** below the switch of a connected project, the Team view (`TeamActivityView.tsx`): the
  shared repositories as checkboxes with a line on what the team sees (display name; per
  session agent, title, state, changed files with line numbers), and "In the team": each
  connected device by name (the own other devices as "You (another device)", "away" as text),
  its sessions with agent and title, the state as text and as a shape (dot working, diamond
  waiting, bar idle), and a folded list of changed files. Keyboard: native checkboxes and buttons.
- **Tests:** `team_share.rs` (documents, size limits, refinement, refused paths, repository
  choices, local sessions), engine tests (two members see each other's presence and a refined
  overlap with lines; nothing changed sends nothing; a session's end clears the overlap; the
  frames carry no path, name or root commit; a device bound to another account stops and keeps its
  keys), gate tests (a teammate's claim pauses, other repositories and unclaimed files do not, deny
  names the teammate, context lines), a gate endpoint test with real HTTP, and `tests/team.rs`
  (two members over real WebSockets: presence before and after linking, no claims before linking,
  refined lines, teammate files for the gate, offline after sharing is turned off; two people on
  one keychain; an unreadable `team.json`; sign-out and sign-in three times without
  `otherInstance`). Vitest for the new parsing and texts in `src/team.ts` and the gate's teammates.
- **Task reservations ("I'm working on ...", built since):**
  - **What is shared:** a task is a short title (at most 120 characters); its keyed hash is the
    protocol's `TaskId` (trimmed, NFC, ASCII lower case), so "Fix login" and "fix LOGIN" are one
    task. `task.reserve` carries a sealed reservation note (record id:
    `record::task_record_id`, the hash's 64 lowercase hex digits) with the title as typed, the person's display name and, when the person linked a running
    shared session, its repository's root commit, an optional folder of it (the area, checked
    like a team path) and the session key. A release request carries a sealed note with the
    asking person's name. The relay sees only the hash (`team_tasks.rs`, pure).
  - **First wins:** nothing is applied before the relay's event. `err conflict` names the holder:
    the attempt shows "Anna #3f9a2c1b… was first. Your agent waits." (without a linked session
    only "… was first.") with "Ask to release" and "Cancel". A holder the log does not list yet
    shows the attempt as still reserving; listed after the next log fetch it counts, otherwise the
    attempt ends with a notice. A session can be linked only when the project shares it
    (`TeamActivity.sessions`). When the hold ends by release or expiry (not a handover) the waiting attempt tries
    again by itself. A handover carries no note of the new holder, so the new holder sends its
    `task.reserve` again (activity) and the team reads the title in the new holder's spelling.
  - **Holder's card:** a release request for a task this device holds shows one card at a time
    above the device confirmations, with Release and Keep disabled for 1 s whenever the shown
    request changes, but only when a listed device of the requester signed the request note (and
    the note was not shown before); a request on the relay's word alone shows a quiet
    "Unconfirmed release request" line without buttons. The answer names the requester the card
    showed (`team_answer_release` refuses another), goes out once and hides the card until the
    relay's event (an error brings it back; the same request repeated by the relay keeps it
    answered, and a request that replaces another makes the other's note old). The requester gets a notice (handed over, kept, no
    answer in time, asked too soon or already asked) only when its attempt had asked.
  - **Expiry:** the relay runs a hold only while its device is offline (`Policy::default`); the
    Team view shows "runs out at 14:05 without activity" from `expiresAt`, and the person gets a
    notice on any of its devices at `reservation.expiring`, then at `reservation.expired`.
    Heartbeats go out at most once a minute while a shared session's claims change. After a key
    rotation (and on every new connection) the device reserves its holds again under the current
    hash with the older ones in `prev`; `prev` names only hashes under which a hold of an earlier
    epoch stands (this device's own, or a teammate's who keeps the task while offline), never
    every old hash of the title, which would only tell the relay which hashes of two epochs
    belong together. Expired and force released notices come only when a hold was there.
  - **Restart:** reservations are kept in memory only, the note marks in the state file
    (`noteMarks`, `noteFloors`: per device and note, at most 64 with a floor, like the claim
    details, but the marks of holds and signed requests that still run are never the ones
    dropped); a hold that is missing from the snapshot after a resync counts as ended, and a hold
    whose note never opened gets a mark from the note's header (`record::sealed_header`) when it
    ends (only under a key period the log gives the holder, its counter at most 10 minutes ahead
    of this device's clock, and only while the device has room for another mark: a header is
    not checked, so it never drops a mark or raises the floor); a log answer inside the snapshot
    waits with that check until the snapshot is complete; the first `hello` of an engine sends no cursor, so the relay's snapshot (`task.reserved` per hold, plus `reservation.expiring` and
    `task.release_requested`) restores them; later reconnects of the same engine resume. Attempts
    that waited for someone else are forgotten on a restart; own holds come back with their note.
  - **Gate and context:** `gate::assess_with_tasks` pauses an edit inside the area of a
    teammate's reservation (same root commit, path within the folder, compared as the team
    compares paths) in a clone linked to that project, like a teammate's file
    (`PendingDecision.reserved`, level `sameArea`); the person's own reservations never pause.
    An attempt of this device that a teammate's hold blocks pauses every edit of its linked
    session (inside the attempt's folder when it named one) while the blocking hold is known here
    (a conflict without a hold, or a hold that ended, pauses nothing); the card shows the task as
    reserved. Without a linked session nothing pauses.
    The prompt context lists at most 10 reservations before the files (at most half of the 4000
    characters, the rest counted as `moreTasks`): `{"task":..,"reservedBy":..,"area":..}`,
    `"reservedByUser":true` for the person's own, `"wantedByThisSession":true` for an attempt
    of that session a teammate's hold blocks; every string a `literal()`.
  - **Trust:** see the relay event table in [[team-crypto]] (listed holders only, in order and
    once, waiting for the next log fetch at most 1024 events and fetching for them at most every
    5 s, at most 32 holds per member, notes only from the holder, a note whose title does not hash
    to the task or whose name has control, invisible or reordering characters refused, a note
    shown before or of a hold that ended refused, also after a resync or a restart).
  - **Accepted risks:** release, expiry and handover are not signed by the holder: the relay
    decides first wins and can end a hold or hand it over (a member could ask an honest relay
    for the same, owners and admins can force a release). A broad area (such as `src`) pauses
    the teammates' agents in all of it, but only with a question to their person, and it runs
    out like any hold while its device is offline.
  - **Interface:** commands `team_reserve_task(projectId, title, sessionId or null, area or
    null)`, `team_release_task(projectId, task)` (also cancels an attempt and withdraws its
    request), `team_ask_release(projectId, task)`, `team_answer_release(projectId, task, requester, accept)`
    (main window only; `task` is the hash from the view); `TeamActivity` adds `tasks: TaskView[]`,
    `attempts: AttemptView[]`, `notices: TaskNotice[]`, `sessions` (the linkable session ids);
    `RequestView.confirmed`; error kinds `invalidTask`, `unknownTask`,
    `taskLimit`. Types mirror in `src/team.ts`.
  - **UI:** "Tasks" at the top of a connected project's Team view (`TeamTasksView.tsx`): one
    "Reserve task" button that opens a small form (title, optional session, a folder once a
    session is chosen), the waiting attempts, and every reservation with its holder, since when,
    when it runs out, its folder and one action (Release or Ask to release). Notices show once
    in the window and, in the background, as a desktop notification at most once a minute.
  - **Protocol wishes (not built, nothing invented on the wire):** `task.release_requested` names
    only the requester's member, so the app tries the requester's listed devices to open the note
    (a holder would let it check one); a handover's `task.reserved` carries no note (the new
    holder sends its own).
  - **Tests:** `team_tasks.rs` (notes, request notes, areas), engine tests against the in-memory
    relay with the protocol crate's `ReservationBook` (first wins and the blocked attempt names
    the holder, a request declined then refused by the cooldown then accepted with the title read
    again, an offline holder warned then expired and the waiting attempt winning, an older note,
    a duplicate `seq`, an unlisted holder and a note moved to another task ignored, the first
    `hello` without a cursor, a conflict with an unlisted holder, a note of an ended hold after
    a resync and a restart, notices without a matching hold or request, a card only for a signed
    request answered once, a note naming another task or a reordering name, `prev` only for old
    holds, the throttled log fetch for unknown holders), gate tests (a reserved area pauses, the
    person's own does not, a blocked attempt pauses only its own linked session, context lines as
    bounded literals), and `tests/team.rs` (two people over real WebSockets:
    reserve with an area, the gate's view on the other device, first wins, ask and hand over,
    sharing off and on restores the hold from the snapshot, release). Vitest for parsing, texts
    and the release request list.
- **Not built yet:** shared decisions; a "Wait" answer
  of the gate; the gate for other agents than Claude Code; naming the area of a `sameArea`
  overlap (the relay sends only hashes); removing devices and members from the UI; a hint in the
  Team view when local claims are turned off (then no repository is offered).

### Private and team sessions (0.6.0)

Decided in [[0008-private-and-team-sessions]]. Each session is private or a team session
(`SessionMode`). Only team sessions get the gate of step B (`SessionManager::spawn` asks the
`LaunchGate` only for them) and only they reach the team of part 2b (`team_share::team_labels`
feeds `local_sessions`, which skips a worktree where a private session works too, as its claims
cannot be split per session); local claims of step A cover both. Team is refused for a program
without a hook format and for a profile with a fixed working folder. A team session always runs in a
worktree of its own. The catalog's `capabilities` gained `hook_format` (`"claude_settings"` for
Claude Code, the only one built); the gate needs it next to `pre_edit_block` and
`per_session_config`, and the window lists exactly those programs for team sessions
(`ProfileView.teamCapable`). The check for a profile's own hook configuration moved with it
(`HookFormat::conflicts_with`, for Claude Code `--settings` and `--settings=`). Presence and
shared claims stay agent-independent: they come from git, the program is only a label.

## Still to verify by measurement
- Claude Code http hook: a connection failure and a timeout fail open (measured,
  [[claude-http-hook-measurement]]); still open whether sub-agent tool calls pass `PreToolUse`
  and how the interactive mode behaves while a hook waits.
- Codex: path shape of `apply_patch` input; Gemini: hook merging with
  `GEMINI_CLI_SYSTEM_SETTINGS_PATH`; Copilot: per-session hooks.
- Cost of `git status` and `git diff -U0` on a large repository on Windows: measured, see
  [[claims-rescan-measurement]]. macOS and Linux still open.
- Exact versions of `chacha20poly1305` and the dalek crates before pinning.
