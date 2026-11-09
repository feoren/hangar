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

## Still to verify by measurement
- Claude Code http hook: a connection failure and a timeout fail open (measured,
  [[claude-http-hook-measurement]]); still open whether sub-agent tool calls pass `PreToolUse`
  and how the interactive mode behaves while a hook waits.
- Codex: path shape of `apply_patch` input; Gemini: hook merging with
  `GEMINI_CLI_SYSTEM_SETTINGS_PATH`; Copilot: per-session hooks.
- Cost of `git status` and `git diff -U0` on a large repository on Windows: measured, see
  [[claims-rescan-measurement]]. macOS and Linux still open.
- Exact versions of `chacha20poly1305` and the dalek crates before pinning.
