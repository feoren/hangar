# Log

Append-only chronicle. Format: `## [YYYY-MM-DD] <ingest|query|lint|schema> | <title>`

## [2026-11-07] schema | Wiki created
Structure set up following the LLM Wiki pattern (Karpathy): raw/, pages/{sources,components,platforms,agents,concepts,decisions,syntheses}, overview.md, index.md, log.md, CLAUDE.md (schema).

## [2026-11-07] ingest | Foundation and ConPTY measurement
Created ADR pages/decisions/0001-foundation.md. Processed our own measurement raw/2026-11-07-conpty-cursor-query.md as the source pages/sources/conpty-cursor-query-measurement.md. Index updated.

## [2026-11-07] ingest | Tauri updater and NSIS
Read the updater docs and the NSIS template: pages/sources/tauri-updater-and-nsis.md. Created the guide pages/syntheses/release-rollout.md. Index updated.

## [2026-11-07] ingest | Agent CLI research
Filed the research report as raw/2026-11-07-agent-clis-research.md, created the source pages/sources/agent-clis-research.md and pages/agents/catalog.md. Index updated.

## [2026-11-07] ingest | Version scheme
Read the Wikipedia article on version numbers and tauri-bundler (try_add_numeric_build_number). Created ADR pages/decisions/0002-version-scheme.md and the source pages/sources/version-number-wikipedia.md.

## [2026-11-07] query | Agent catalog brought up to date
Aligned pages/agents/catalog.md with the code: detection on every list request, user profiles keep the homepage and install command from the catalog, display "Not installed (n)".

## [2026-11-07] query | ADR 0003 implemented
Set pages/decisions/0003-program-versions.md to accepted and added the implementation, measured values and the risk of the late job assignment. Index updated.

## [2026-11-07] ingest | Resume and restore
Filed our own measurement as raw/2026-11-07-resume-measurement.md (claude, codex, opencode, pi, aider with resume_args in an empty folder). Created the source pages/sources/resume-measurement.md, pages/agents/resume.md and the contract pages/syntheses/session-restore.md; extended pages/agents/catalog.md and the index.

## [2026-11-07] ingest | Notes and program updates
Created ADR pages/decisions/0004-notes-per-working-folder.md (accepted) and the contract pages/syntheses/notes.md. Filed the research raw/2026-11-07-update-sources-research.md and our own measurement raw/2026-11-07-update-sources-measurement.md; created the source pages/sources/update-sources.md, pages/agents/updates.md and the contract pages/syntheses/program-updates.md. Index extended.

## [2026-11-08] ingest | Running program updates
Extended pages/syntheses/program-updates.md with canRun, create_update_session and the execution rules (update_hint only, argv without a shell, shell syntax rejected, version cache cleared after the exit); adjusted pages/agents/updates.md. Index updated.

## [2026-11-08] ingest | Program updates: security rework
pages/syntheses/program-updates.md: the frontend no longer passes a working folder, NoDefaultCurrentDirectoryInExePath for update sessions on Windows, relative program paths rejected, the backend refuses a second concurrent update, and that lock is not a sandbox.

## [2026-11-08] ingest | Code signing for the installer
Filed the research result and our own checks (tauri build --help, config.schema.json and CHANGELOG of @tauri-apps/cli 2.12.4, Get-AuthenticodeSignature) as raw/2026-11-08-code-signing-research.md. Created the source pages/sources/code-signing-research.md; added a signing section (vendors, costs, variables, verification) to pages/syntheses/release-rollout.md; overview.md and index updated.

## [2026-11-08] ingest | .cmd launcher scripts: node in the working folder and BatBadBut
Processed our own measurement raw/2026-11-08-cmd-launcher-measurement.md (Windows 11, ConPTY, portable-pty 0.9.0) as the source pages/sources/cmd-launcher-measurement.md. Created the synthesis pages/syntheses/cmd-launcher.md (criterion .cmd/.bat target, reject instead of escape, inheritance, opting out with an empty env value). Overview and index extended.

## [2026-11-08] query | Rule "the code always carries the upcoming version"
Extended pages/decisions/0002-version-scheme.md with the rule, the sync test (src/versionSync.test.ts), the new checks in tools/release.py (--version equal to the highest changelog version and greater than the last published one) and --next. pages/syntheses/release-rollout.md: added step 5 (create the upcoming version) to the procedure. Index updated.

## [2026-11-08] schema | English wiki, index lines with a colon
Translated the wiki to English: schema, index, log, overview, decisions, components, agents, sources and syntheses; terms follow the root `glossary.md`. Pages and raw sources were renamed to English kebab-case slugs beforehand (for example `0001-grundgeruest.md` to `0001-foundation.md`), with all links adjusted. Index lines now separate name and one-liner with a colon: `- [[name]]: one-liner (sources: n, updated: YYYY-MM-DD)`. Callouts are now `> [!question] Unverified` and `> [!warning] Contradiction`; decisions follow the outline Context, Options, Decision, Consequences, Evidence. Brought up to date along the way: ADR 0004 (folder names `general` and `.trash`).

## [2026-11-08] ingest | Releases on GitHub
pages/syntheses/release-rollout.md: releases are published as GitHub releases (tag vX.Y.Z, draft first, then published with `gh`); the updater polls GitHub first and the own server as a fallback, verified in tauri-plugin-updater 2.14.1 `src/updater.rs` (endpoints in order, first valid manifest wins). Transition step for 0.1.x installations, download button URL and the `origin` remote. Index updated.

## [2026-11-08] schema | Public repository
The repository starts on GitHub with a fresh history. ADR 0002: the build number adds `buildNumberBase` (84, the commit count before the start) from `package.json`, so build numbers keep growing. Pages that referred to local working instructions now refer to `CONTRIBUTING.md` or describe the point directly.

## [2026-11-08] ingest | WSL distribution list and keyring
Processed our own measurement raw/2026-11-08-wsl-list-measurement.md (Windows 11, WSL 2.7.14.0, keyring 4.2.0) as the source pages/sources/wsl-list-measurement.md. Created the component page pages/components/keyring.md. pages/syntheses/agent-workflow-features.md: backend contracts for profiles in Settings, secrets, export, WSL and tray, and the hardened git, worktree and notification rules. Index updated.

## [2026-11-08] ingest | keyring store internals
Read the sources of windows-native-keyring-store 1.1.0, zbus-secret-service-keyring-store 1.0.1 and keyring-core 1.0.1: target names ignore case, `persistence` modifier (Local) applies on write, `get_attributes` tells "stored" without exposing the value (macOS: default implementation reads the value). Measured Local persistence with the ignored keychain test. Updated pages/components/keyring.md and pages/syntheses/agent-workflow-features.md.

## [2026-11-08] query | Session reordering and snap layouts as built
pages/syntheses/agent-workflow-features.md: "As built" notes for items 21 (reorder sessions by pointer drag and Alt+arrow) and 22 (snap layouts, picker, migration of `splitMode`), including why pointer events replace HTML5 drag and drop next to Tauri's file drop (marked unverified). Item 3 points to the replacement.

## [2026-11-08] ingest | Conversation ids and the sub-agent view
Processed our own measurement raw/2026-11-08-resume-id-measurement.md (Windows 11, Claude Code 2.1.319, without a terminal and in ConPTY) and the follow-up check raw/2026-11-08-session-id-in-use-check.md (strings of the 2.1.319 executable) as the source pages/sources/resume-id-measurement.md. pages/agents/resume.md: `session_id_args` and `resume_id_args`, why the fallback reuses the id, why a restart starts a new conversation. pages/syntheses/session-restore.md: `cliSessionId` in the workspace, `create_session` and the `resumeFallback` event (`cliSessionId`, `subAgents`). pages/syntheses/agent-workflow-features.md: item 23 as built (backend contract of `watch_sub_agents` and `unwatch_sub_agents`, polling instead of `notify`, path rules). Index updated.

## [2026-11-08] query | Sub-agent excerpt redaction and conversation id rules
Sub-agent excerpts and descriptions pass a best-effort redaction filter (`src-tauri/src/subagents/redact.rs`)
and can be switched off; locating a transcript backs off from 2 s to 30 s; `cliSessionId` must be unique in
the workspace and a live duplicate starts fresh; profiles that pick a conversation in their own args do not
inherit the id options. Pages changed: [[agent-workflow-features]], [[resume]], [[session-restore]].

## [2026-11-08] query | Acceptance review of one-click updates and the notes backend
Update plans run only a uv installed for the user's own use (standalone installer folders, cargo, or
the package `uv` of winget, Scoop, Homebrew, pipx); a uv bundled in another application's folder
yields `tool-missing` with copy text. The update outcome claims no cause after a successful update
without versions to compare. Trash records that name anything but a plain `.md` file in `.trash/`
are ignored. Pages changed: [[program-updates]], [[notes]], index.

## [2026-11-08] query | Starting a session and one-click updates as built
pages/syntheses/notes-and-start-usability.md: "As built" for findings S1 to S9 (one start target from
the active session's project, folder menu with running projects, favorites and recent folders and a
star per row, worktree decided per start, target shown in sidebar, rail, empty state, Ctrl+Shift+T
hint and quick access) and for one-click program updates (plan, reason and outcome texts, admin
note, second copies, `expectedPlan`, inline question while the program runs). Glossary: start
target, running projects, update plan, second copy, update outcome.

## [2026-11-09] query | Acceptance review of clearer attention states and the chime
Measured in headless Edge: an AudioContext created before the first click or key press in the page
stays suspended, and its pending `resume()` settles only when a later one succeeds, so the queued
chime sounds late, together with the next one. `chimeAudio.ts` therefore plays nothing before
`navigator.userActivation.hasBeenActive` and drops a chime whose resume takes longer than 500 ms.
Contrast at the peak of the breathing tint (default and custom colors, both themes): row text at
least 4.5 : 1, muted text at least 3 : 1; the light theme's "failed" chip text is darkened to
reach 5.5 : 1. Pages changed: [[agent-workflow-features]].

## [2026-11-09] query | Notes redesign and right panel as built
Recorded the 0.3.0 notes rework (quick note, list with scope chips and search, checklists, send
to terminal and terminal selection to note, one right panel with Notes and Changes tabs, overlay
below 900 px, More menu with trash, detached window over localStorage and `notes-command`), the
waiting edge hint in the sidebar and the 3 : 1 edge colors of `attentionEdges`. Pages changed:
[[notes-and-start-usability]], [[notes]].

## [2026-11-09] ingest | Notes window hardening
Recorded the send warning for app text without bracketed paste (`src/appPaste.ts`), the marked
invisible characters in the editor, the `onCloseRequested` close of the notes window (the API
awaits the handler, then calls `destroy()`) and `set_notes_on_top` as a notes window command.
Pages changed: [[notes]].

## [2026-11-09] query | Team service version 1
Filed the desk research on the open questions of ADR 0005: pre-edit hooks per agent CLI, change
observation with git as the oracle and two-stage conflict detection, team key epochs instead of
MLS, a single Rust server with WebSocket and SQLite, and the build plan in steps A to G.
Pages changed: [[team-service-v1]], [[0005-team-collaboration]].

## [2026-11-09] ingest | Claims rescan measurement and step A as built
Measured `git status --porcelain=v2` and `git diff -U0` through `Git::run` on 382 and 50 000
files (about 150 ms per rescan, 1.3 s with stale stat data) and read notify 8.2.2's Windows
completion routine (silent unwatch after an overflow). Built local claims (step A): watcher per
worktree as a trigger, coalesced git rescans, overlap levels, sidebar marker, warning block in
the Changes tab, one-time notice. Pages changed: [[claims-rescan-measurement]],
[[team-service-v1]], [[overview]].

## [2026-11-09] ingest | Step A hardening after review
Refused work trees moved by `core.worktree` (shares, whole drives) before resolving or watching,
stopped following symlinks in the watcher, dropped events from ignored top-level folders, added
a scan budget across worktrees, cached folder keys, capped overlaps per session, gave a released
worktree back when its removal fails, and showed the overlap in the collapsed rail. Read notify
8.2.2's inotify source for the ENOSPC path. Pages changed: [[team-service-v1]].

## [2026-11-09] ingest | Claude Code http hooks and step B as built
Measured Claude Code 2.1.319 http hooks given through `--settings` (request shape, deny reason
reaches the model, timeout and unreachable endpoint fail open, no http hooks on SessionStart,
UserPromptSubmit context) and filed excerpts of the hooks reference. Built the agent gate (step
B): catalog capability flags, local hook endpoint with per-session tokens, pending decisions
answered in the window, facts as prompt context. Pages changed: [[claude-http-hook-measurement]],
[[team-service-v1]].

## [2026-11-09] ingest | Claude Code --settings file and step B hardening
Measured `claude -p --settings <file>` with Claude Code 2.1.319 (file path with spaces works,
`Host` is `127.0.0.1:<port>`, "ask" in print mode does not write the file). Hardened step B after
the security and acceptance reviews: total time limits per request and separate slots for
unknown clients, 403 for browser requests and other hosts, no file system access for UNC and
device paths, names as JSON literals in reasons and facts, a per-session decision limit with
"ask" when full, a settings file for `claude.cmd`, a gate status per session, and a restore that
waits for the first gate setting. Pages changed: [[claude-settings-file-measurement]],
[[team-service-v1]].

## [2026-11-09] ingest | Step B: profiles with their own --settings
A profile or resume that already passes `--settings` now starts without the gate (status
`profileSettings`), since merging two `--settings` is not measured. Pages changed:
[[team-service-v1]].

## [2026-11-09] ingest | Step C protocol crate
Built the protocol crate `crates/hangar-team-protocol` (step C): wire types with strict decoding,
keyed hashes with independent test vectors, reservation and overlap state machines, event log
sequencing. Filed its design as a concept page. Pages changed: [[team-protocol]],
[[team-service-v1]].

## [2026-11-09] ingest | Step C rework after reviews
Reworked the protocol crate after the acceptance and security reviews: time events apart from the
outcome, release request timeout, withdrawal and cooldown, force release, connection counted
presence, device removal, rekey grace with old hashes in `task.reserve`, `staleEpoch`, HKDF key
separation, NFC and format character checks, area-only path hashes, claim detail delivery,
stricter decoding and op retry keys. Added a threat model. Pages changed: [[team-protocol]],
[[team-service-v1]].

## [2026-11-09] ingest | Step C protocol additions for the relay
Extended the protocol crate with the relay's wishes, keeping protocol version 1: end-to-end
groundwork without cryptography (device keys, membership log with a SHA-256 hash chain and test
vectors, key rotation, wrapping and fetching), `member.role_changed`, presence `offline`, and
documented answer, deduplication, `notify` and frame size rules. Pages changed: [[team-protocol]],
[[team-service-v1]].

## [2026-11-09] ingest | Step F part 1: account sign-in and projects
Built the app side of the Hangar account: device flow sign-in with polling rules, the refresh
token in the OS keychain, single-flight refresh with rotation, problem documents, origin checks
for opened pages, Settings section Account and the sidebar's Team group. Filed the API and the
client's behavior. Pages changed: [[hangar-account-api]] (new), [[team-service-v1]].

## [2026-11-09] ingest | Step F part 1 rework after reviews
Filed the website's revoke endpoint (RFC 7009), the 30 s refresh grace and the new seat counting.
The client now holds a cross-process lock and rereads the keychain before a refresh, retries a
lost refresh answer once, keeps a separate keychain entry in debug builds, revokes on sign-out,
lists other servers' stored sign-ins, bypasses proxies for loopback servers, sends normalized
pages to the window and checks relay addresses. Pages changed: [[hangar-account-api]].

## [2026-11-09] ingest | Step F part 1 follow-ups: per-build sign-in lists and lock coverage
`account.json` keeps the list of stored sign-ins per keychain service and is reread under the
process lock before each write. Storing a confirmed sign-in and ending the session after a second
401 now hold the process lock as well. The `signedOutLocally` notice no longer claims the server
was unreachable. Pages changed: [[hangar-account-api]].

## [2026-11-09] ingest | Team crypto (step E)
Built the pure crate `crates/hangar-team-crypto`: device keys and safety numbers, a signed
membership log bound to team and position, epoch commitments with HPKE wrapping, history links,
sealed and signed records and the recovery code, with RFC anchored tests and an independent
Python reference for the vectors. Needs an external cryptographic review. Pages changed:
[[team-crypto]] (new), [[team-service-v1]].

## [2026-11-09] ingest | Team crypto and protocol hardened after the security review
The relay now validates every membership log entry with the crypto crate's verifier and takes a
rotation only as one `key.rotate` with its signed entry. Devices carry a proof of possession and
need the endorsement of one of their member's devices; recovery devices only restore their
member; members manage their own devices. Epoch keys bind the rotate entry's log hash, records
carry the sender's log head and a counter, X25519 keys are validated, a key history verifies
records of removed devices, and the relay keeps up to four wrapped keys per device and epoch.
Twenty questions for the external review are filed. Pages changed: [[team-crypto]],
[[team-protocol]], [[team-service-v1]].

## [2026-11-09] ingest | Team crypto and protocol, second hardening round
The relay now caps an issuer's rights at the role of its verified ticket (entry 1 only with an
owner's ticket), takes a device removal only as `device.remove` with its signed log entry, and
checks `key.wrap` against the verified log in the crypto crate's new `relay` module (one package
per sender, target and epoch). A member has at most 16 devices and a team at most 512. The relay
requires that a device's keys in the log are the keys that device registered itself, replay
marks are kept per holder and signing key, and the state shows a re-admitted member. Pages
changed: [[team-crypto]], [[team-protocol]], [[team-service-v1]].

## [2026-11-09] ingest | Team service step F part 2a as built (team connection)
Hangar connects a shared project to the team relay: device identity, epoch secrets and the
verified log head in the keychain, the event log cursor and replay marks in a state file, a
WebSocket with ticket renewal, the relay's close codes and jittered backoff, team creation, the
first epoch, confirmation of new devices and members by safety number, key wrapping and rotation
after a removal, and a "history does not match" state for a forked or rewritten log. Tested
against an in-test relay built from both crates. Pages changed: [[team-service-v1]].

## [2026-11-09] ingest | Team connection hardened after security and acceptance review
Relay events no longer wipe keys on their own: removals need the verified log or the website;
4404 asks the website first; every wipe turns sharing off. Team state and `team.json` are kept per
keychain service; a lock file keeps a team to one window; a sign-out stops the teams at once and a sign-in restarts them; a server change stops the old server's workers; the backoff resets only after a
settled connection; the reader queue is bounded; the send counter has a clock floor; unreadable
device entries and state files stop instead of starting over. The confirmation panel shows one
request at a time with an arming delay, an inline question before rejecting and a re-admission
warning. Pages changed: [[team-service-v1]], [[team-crypto]].

## [2026-11-09] ingest | Team service step F part 2b as built (see your team)
Teammates' presence and claims: repositories are linked to a shared project in the Team view;
presence and claim details travel sealed and are opened against the verified log; the relay's
"same file" is refined to overlapping lines locally; teammate overlaps show in the sidebar marker,
the Changes tab and a one-time notice; the agent gate pauses on a teammate's file and the context
names teammates' files. Review items of part 2a: the device identity is bound to its account, an
unreadable `team.json` is an error, a new worker waits for the old one's turn. Reservations are
not built yet. Pages changed: [[team-service-v1]].

## [2026-11-09] ingest | Team service step F part 2b: review fixes
A fork is reported only for a record whose signature verifies (crypto crate, check order);
high-water marks for teammates' status and claim details survive offline, reconnects and
restarts; only records of the current epoch or newer open; peers and overlaps are bounded and
desktop notices about teammates throttled; repositories are linked per clone folder (root-only
links share nothing until a folder is ticked again); the e-mail address no longer leaves the
device; names carry the short member id; git ignores replace refs. Pages changed:
[[team-service-v1]], [[team-crypto]].

## [2026-11-09] ingest | Team service step F part 2b: security follow-up
A signed fork in a teammate's live record now sets only that device aside (named in the Team
view) until the verified log moves on; the team stops only when this device's own log
contradicts its verified head. Presence marks are kept per device (not per signing key) and
for removed devices; overlaps count only for sessions the teammate's newest signed status lists;
a status-less presence keeps the sessions; marks are written at most every 5 s; the root commit
cache is checked per scan; teammate paths in the agent's context use this device's spelling;
old root-only links can be removed. Pages changed: [[team-service-v1]], [[team-crypto]].

## [2026-11-09] ingest | Team service step F part 2b: last security review
Every divergent teammate record now fetches the log again, and divergence reported by devices of
two distinct members stops the team, since the fork is then this device's; devices of one member
never do, as which devices are heard is the relay's word. Fork detection from live records is
documented as best effort. `open_verified` reports a fork only after the epoch start check and decryption, so an
old key of a re-admitted device id cannot get the honest device set aside. Live marks compare by
key period, then counter, and go along with every state file write. The teammate path lookup
for the agent's context is bounded to 50 ms (checked before every file system call), follows no
reparse point, skips worktrees below one and non-fixed drives, and runs outside the gate's lock. Pages changed: [[team-service-v1]], [[team-crypto]].

## [2026-11-09] ingest | Relay interop run and task reservations
Filed the interop run against a self-hosted relay (live protocol compatible; its account API
differs from the website API, reported to the relay's maintainers) and recorded the mismatch in
the account API page. Task reservations as built: sealed notes, first wins with a waiting
attempt, release requests with the holder's card, expiry warnings, reservations in the gate and
the agent's context, trust rows for the reservation events. Pages changed:
[[relay-interop-measurement]], [[hangar-account-api]], [[team-service-v1]], [[team-crypto]],
[[overview]].

## [2026-11-09] ingest | Task reservations: review fixes
A blocked attempt pauses its own linked session; only shared sessions can be linked; the note
record id is fixed in the crypto crate (`task_record_id`); a conflict with an unlisted holder
waits for the next log fetch. Note marks are stored and survive the end of a hold, resyncs and
restarts; notes must hash to their task and carry a plain name; the holder's card needs a signed
request and answers it once; notices need a matching hold or request; `prev` names only old
holds; unknown holders fetch the log at most every 5 s. Accepted risks recorded. Pages changed:
[[team-service-v1]], [[team-crypto]].

## [2026-11-09] ingest | Task reservations: last low findings
Note marks of running holds and signed requests are never dropped to stay within bounds; a
blocked attempt pauses its session only while the blocking hold is known; a repeated release
request keeps its answer and a replaced one makes the old note stale; holds missing from a
resync's snapshot (checked only once the snapshot is complete) and holds whose note never
opened leave a mark when they end (new `record::sealed_header` in the crypto crate; such a mark
needs a key period of the sender, counts at most 10 minutes ahead of the clock and never drops
another mark or raises the floor); conflicts with unknown holders fetch the log at
most every 5 s. Pages changed: [[team-service-v1]], [[team-crypto]].

## [2026-11-10] query | README screenshots as a release step
The README screenshots are now a tracked tool, `tools/readme-shots/` (`pnpm readme:shots`): the
real frontend with a mocked backend and a fictional scene, rendered in headless Edge, Chrome or
Chromium over the DevTools protocol, written losslessly re-encoded to `docs/images/`. The version
in the header comes from `src-tauri/tauri.conf.json`, so a run after `tools/release.py` shows the
released `X.Y.Z-Build`. Added as step 3 of the release steps. Pages changed: [[release-rollout]].

## [2026-11-10] query | Release checksums
`tools/release.py` now writes `SHA256SUMS.txt` next to the installers in `release/<version>/`:
the SHA-256 of `Hangar_X.Y.Z-Build_x64-setup.exe` and `Hangar-Setup.exe` in `sha256sum` format
with LF line endings. It is attached to the GitHub release before `latest.json`, which stays last,
and appears in the printed upload order for `--base-url`. Pages changed: [[release-rollout]].

## [2026-11-10] query | Pasting screenshots
Screenshots paste as a saved PNG's path; why `arboard` is read directly and where the files live.
Pages changed: [[agent-workflow-features]].

## [2026-11-10] query | Bounded clipboard image decoding
arboard decodes the Windows clipboard's PNG without allocation limits, so screenshots are now read
through `clipboard-win` with the size checked in the header first; files are owner only on Unix
and one paste runs at a time. Pages changed: [[agent-workflow-features]].

## [2026-11-10] query | ADR 0006: encrypted project notes
Proposed design for notes shared through the repository: `.hangar/notes/` with `vault.json`, one
`<note id>.note` per note (XChaCha20-Poly1305, per-file HKDF key, vault, note id and key id as
associated data, padded plaintext, nothing sensitive outside), a notes key ring sealed as a signed
team record `notes-keys` and re-sealed on every rotation (new key after removals), a solo mode
with a notes recovery code, main worktree rule, lossless automatic conflict resolution, threat
model, non-goals, three phases and open questions for the owner. No code changed. Pages changed:
[[0006-encrypted-project-notes]] (new), [[0004-notes-per-working-folder]].

## [2026-11-10] query | ADR 0006 accepted
The owner chose `.hangar/notes/` and a mandatory notes recovery code in solo mode; the remaining
open questions follow the proposal (owners and admins turn shared notes on, new members read
earlier notes, no automatic commits). Status set to accepted. Pages changed:
[[0006-encrypted-project-notes]].

## [2026-11-10] update | State looks per session state
Working, done, asks and pause each get their own color, pulse strength and pulse duration; the
custom "working" chip label keeps 4.5 : 1. Pages changed: [[agent-workflow-features]].

## [2026-11-10] ingest | Git with `* -text -diff -merge` for note files
Measured on Windows (git 2.56.0.windows.4, `core.autocrlf` true, false and input): note bytes
stay unconverted, `git diff` treats them as binary, a conflict leaves `UU` (modify/delete `UD`,
add/add `AA`) with stages readable through `git cat-file blob :N:<path>` and no conflict markers;
a rebase swaps stages 2 and 3. The app's conflict test confirms the resolution end to end. macOS
and Linux remain unmeasured. Pages changed: [[note-gitattributes-measurement]] (new),
[[0006-encrypted-project-notes]].

## [2026-11-10] query | Shared project notes, phase 1
Phase 1 of ADR 0006 as built: crypto module `notes` with test vectors, `project_notes.rs` (solo
mode, keychain service `hangar-notes`, code slot, main worktree rule, local trash, automatic
conflict resolution), commands `shared_notes_*` and `move_note`, the Shared and "Only on this
computer" groups and the setup dialog. Pages changed: [[notes]], [[0006-encrypted-project-notes]].

## [2026-11-10] query | Shared notes, security rework
After the security review: `vault.json` conflicts resolve only between two readable vaults whose
key rings this computer holds; stages are read as the XY code promises and `DD` is cleared; the
keychain entry is never overwritten when unreadable or newer; links and junctions for
`.hangar/notes` are refused; the main worktree must share the git common directory; the recovery
code is never saved inside a Git work tree, copies as a secret and leaves the clipboard after a
minute; a prepared setup expires after 15 minutes or when its dialog closes; scans have a byte
budget and remember unreadable files. The key ring size limit of the Windows credential store is
filed as a phase 2 blocker. Pages changed: [[0006-encrypted-project-notes]], [[notes]].

## [2026-11-11] query | Shared notes, last review items
The recovery code is also refused in any folder with a `.git` entry on the way up, even when git
reports no repository (`safe.directory`); the note metadata cache is keyed by file stamp, vault
id and the ring's key ids; search shares the scan's byte budget. Pages changed:
[[0006-encrypted-project-notes]].

## [2026-11-11] query | Sub-agent view shows one sub-agent's activity
The sub-agent view now shows one sub-agent at a time beside the terminal, as a console-like log
streamed incrementally by the watch (`focus_sub_agent`, log events with task, text, tool calls
and short results, all masked); finished sub-agents can be removed from the tree. Pages changed:
[[agent-workflow-features]].

## [2026-11-11] query | Sub-agent view security rework
The mask now strips terminal sequences before it masks, covers command line secrets (flags,
`sshpass -p`, `mysql -pX`, `curl -u`), more names, `Authorization` schemes, SecureString and
more token prefixes; transcripts open without following links; focus switches collapse and
are debounced. Pages changed: [[agent-workflow-features]].

## [2026-11-11] query | Sub-agent mask: linear runtime and more forms
A skipped loose value is passed over as a whole, so `key:` repeated no longer takes
quadratic time; glued `docker login -pX` and `curl -uX`, raw `Authorization:` keys, Unicode
spaces, escaped quotes and shell quoting are covered; FIFOs open without blocking. Known limits
(lookalike names, intermediate links, hard links) are documented. Pages changed:
[[agent-workflow-features]].

## [2026-11-11] query | One Hangar per user session
Read tauri-plugin-single-instance 2.5.5 (Windows, Linux, macOS implementations), the restart and
`cleanup_before_exit` order of tauri 2.12.5 and the Windows install path of tauri-plugin-updater
2.14.1 from the Cargo registry. Release builds register the plugin first; debug builds are exempt.
Measured the plugin's own lock tests on Windows 11 (3 passed). Pages changed: [[single-instance]] (new),
[[single-instance-lock-measurement]] (new).

## [2026-11-11] query | README screenshots per scene
`tools/readme-shots/` now renders five scenes in dark and light (overview, sub-agent log, Team
window, notes with shared and local groups, Settings appearance) with stable file names, a fixed
clock, UTC and animations frozen on a fixed frame; `--scene` picks some. Edge may relaunch itself
with exit code 0 and keep running, so the driver waits for the DevTools port instead of failing
and always sends `Browser.close`. Pages changed: [[release-rollout]].

## [2026-11-11] ingest | Sub-agent activity, tokens and tool results
Measured with Claude Code 2.1.322 on about 160 sub-agent transcripts (key names and numbers only):
the token count Claude Code shows is the context size of the newest assistant response's `usage`,
the tool use count is the number of `tool_use` blocks, each tool result's record carries a structured
`toolUseResult` (line counts, patches, file counts), and the live activity line is stored nowhere.
Pages changed: [[subagent-activity-measurement]] (new), [[subagent-transcripts]].

## [2026-11-11] ingest | Context use and window size of a session
Measured with Claude Code 2.1.322 on the 40 newest main transcripts (key names and numbers only):
the context use is in the newest `assistant` record's `usage`, the window size is stored nowhere
Hangar can read, and contexts above 600K show that Opus 5.5 and Sonnet 5 have 1M without a beta.
Hangar maps model ids to windows after the docs' model table. Codex rollouts cannot be linked to a
session yet. Pages changed: [[context-window-measurement]] (new).

## [2026-11-12] ingest | Program update checks every 30 minutes
GitHub REST docs (best practices, rate limits) and a measurement: an unauthenticated `304` still
counts against the 60 requests per hour, PyPI answers `304` too, npm `/latest` sends no `ETag`.
Hangar now checks every 30 min and on returning to the window after that long, keeps results for
25 min and failures for 15 min, re-reads the installed versions on each periodic check, sends
stored entity tags and pauses a source on `403`/`429` rate limits. Pages changed:
[[program-updates]], [[github-conditional-requests]] (new).

## [2026-11-12] ingest | App execution aliases of Store apps
libuv's AppExecLink reader and a measurement with the Store PowerShell 7.7.0: the alias is an
empty reparse-point file that starts fine but cannot be opened; the third string of its reparse
data names the package executable. Hangar now reads icons and file stamps from that executable.
Pages changed: [[app-execution-alias]] (new).

## [2026-11-12] query | Update plan for Store apps
A program found as an execution alias, or below `Program Files\WindowsApps`, is now a Store
install: no winget command and no administrator rights, reason `store-managed` ("The Microsoft
Store keeps this program up to date."). Checked before the Program Files rule that used to call
the package folder a machine-wide setup. Pages changed: [[program-updates]].

## [2026-11-12] query | Version queries in their own folder
Security review: the version query ran in the temporary folder without
`NoDefaultCurrentDirectoryInExePath`, so an npm launcher could start a `node.cmd` planted there.
Sessions and version queries now share one environment rule (`src-tauri/src/child_env.rs`) and the
queries run in `version-probe` in the app data folder. Pages changed: [[0003-program-versions]],
[[cmd-launcher]].

## [2026-11-12] query | Stricter execution alias targets
Security review: alias targets were only checked for an `X:\` prefix. They now need plain
components (no `.`, `..` or device names, after Microsoft's naming rules), a fixed local drive,
and for a Store install a `WindowsApps` folder. Pages changed: [[app-execution-alias]].

## [2026-11-12] query | What an update check sends
Security review: the stored entity tag goes back to the source as `If-None-Match`, so "nothing
that identifies the user" no longer held. The contract now names the tag and why a server could
recognize a client by it. Pages changed: [[program-updates]].

## [2026-11-12] ingest | Bundled agent icons
Simple Icons 16.37.0 data, disclaimer and license, the Primer Octicons license, three brand
guideline pages, vendor site colors and an icon count of the agent executables on Windows. Every
catalog agent now has a bundled tile: 10 marks from Simple Icons, 12 monograms drawn for Hangar;
"Automatic" prefers it over the exe icon. Pages changed: [[agent-icons]] (new), [[catalog]].

## [2026-11-12] query | File viewer window contract
Backend for a read-only file viewer: `open_file_viewer` (main window only) opens a window
`file-viewer-<n>` bound to one file of a session's folder with the file tree's path rules;
`file_viewer_content` reads exactly that file for the calling window. Pages changed:
[[file-viewer]] (new).

## [2026-11-12] query | File viewer window frontend
Frontend of the file viewer: opening from the Files tab, the lazily loaded read-only CodeMirror
view, seven viewer themes with dark and light palettes, settings sync through the `storage` event
and the "working" color along the path to changed files. Pages changed: [[file-viewer]].

## [2026-11-12] query | Commit history contract
Backend for showing committed changes in the Changes tab: `git_log` (a page of commits from HEAD,
newest first, each marked pushed when the upstream has it) and `git_commit_diff` (one commit's
changes against its first parent or the empty tree, in the shape of `git_diff` plus the header).
Pages changed: [[commit-history]] (new).

## [2026-11-12] ingest | Terminal font coverage measurement
Measured box drawing, block element, braille and symbol coverage of candidate monospace fonts:
Fontsource subsets of Google-hosted fonts lack U+2500..U+259F, the upstream Source Code Pro, IBM
Plex Mono and Victor Mono files cover them, so those three are bundled from upstream. Recorded the
0.6.0 terminal look (14 schemes with contrast floors, renamed keys, more system fonts). Pages
changed: [[terminal-font-coverage-measurement]] (new), [[agent-workflow-features]].

## [2026-11-12] query | Detachable side panel
Decided and built the detachable right panel: one backend-created `panel` window replaces the
notes-only window and shows Notes, Changes and Files. The main window pushes a validated context
(`panel-context`) and commands with `emitTo`; the panel answers through `panel_to_main`
(`sendToTerminal`, `notesChanged`, `showMain`, `showSession`, `ready`) and may open file viewers.
Size and place are remembered and checked against the monitors. Pages changed:
[[0007-detachable-side-panel]] (new), [[notes]], [[file-viewer]], [[single-instance]].

## [2026-11-12] decision | Private and team sessions
The owner replaced the per-program worktree button with a switch "Private | Team". A team session
has its own worktree, the agent gate and is shared with the team; a private one has none of it.
Team capability comes from the catalog only (`hook_format` next to the capabilities), presence
and claims stay agent-independent. Pages changed: [[0008-private-and-team-sessions]] (new),
[[team-service-v1]], [[agent-workflow-features]].

## [2026-11-12] decision | Private and team sessions, review fixes
A worktree shared by a private and a team session is not shared with the team; Team is refused
for programs without a hook format and profiles with a fixed folder; old worktree sessions of
such programs restore private; sessions without a worktree lose the gate after the upgrade.
Pages changed: [[0008-private-and-team-sessions]], [[team-service-v1]].

## [2026-11-12] ingest | Per-session edit gates for Codex, Gemini, Copilot and Qwen Code
Desk research (no hook run against a live model; local `codex --version`, `--help`, `features list`
only) on pre-edit blocking, context injection and per-session installation. Findings: Qwen Code and
Gemini CLI take a settings file through an env var (`QWEN_CODE_SYSTEM_SETTINGS_PATH`,
`GEMINI_CLI_SYSTEM_SETTINGS_PATH`) and concatenate hooks with the user hooks; Copilot CLI takes
`--plugin-dir` and cannot use a loopback http hook for `preToolUse`; Codex takes `-c` hooks but needs
`--dangerously-bypass-hook-trust`, passes the whole patch text, and reads project hooks of a linked
worktree from the root checkout. Open measurements listed in the page. Pages changed:
[[agent-hooks-per-session-research]] (new).

## [2026-11-12] ingest | Per-session edit gates for the remaining agent CLIs
Desk research (no agent installed or run) on the 17 catalog agents not covered before, against vendor
docs, GitHub sources and registry metadata. Pi, Oh My Pi, OpenCode and Droid are feasible; Cline, Amp,
Crush and Vibe only after a measurement; Aider, Goose, Kimi, Antigravity, Cursor, Kiro, ForgeCode, Kode
and Codebuff are not possible today. Pages changed: [[agent-hooks-remaining-agents-research]] (new),
index.
## [2026-11-12] ingest | Agent gate adapters for Qwen Code, Gemini CLI, Copilot CLI and Codex CLI
Built the four per-session hook recipes of the research page and turned the catalog flags on.
Measured on Windows 11 (no model call): a GUI-subsystem program works as a hook command through
cmd.exe, PowerShell and a direct start, PowerShell loses its exit code; Codex 0.167.2 registers a
`-c hooks.PreToolUse` value as an untrusted `sessionFlags` hook, and cmd.exe eats `^` through npm's
`codex.cmd`. Gemini CLI, Copilot CLI and Qwen Code were not installed and stay unmeasured. Pages
changed: [[gate-hook-helper-measurement]] (new), [[agent-gate-adapters]] (new),
[[agent-hooks-per-session-research]], [[0008-private-and-team-sessions]].

## [2026-11-12] ingest | Gate helper through PowerShell: console build and UTF-8
Acceptance review measurement: PowerShell turns the console build's exit code 2 into 1 and passes
UTF-8 stdin and stdout unchanged. The Gemini deny now leaves the helper as JSON with exit 0; a
Codex `-c`/`--config` that sets `hooks` or `hooks.PreToolUse` now counts as the profile's own
hook settings. Pages changed: [[gate-hook-helper-measurement]], [[agent-gate-adapters]],
[[team-service-v1]].

## [2026-11-12] ingest | Codex live run and gate rework
Owner's decision "measure first". One approved live Codex 0.167.2 measurement (three runs, probe
as hook command, throwaway repository deleted): the `-c` hook fires on `apply_patch`,
`HANGAR_GATE` reaches the hook, the patch carries relative paths, and only a JSON deny with exit 0
blocks the write (exit 2 plus JSON fails open). The helper answers Codex that way now. Codex is
offered from 0.167.2; Gemini CLI, Copilot CLI and Qwen Code keep their recipes with
`measured = false`. New: minimum version per format from the catalog, no team offer where an
administrator's system settings would be replaced, gate variables stripped by a child Hangar,
path limit, stdin deadline, fresh private session folders, `(`, `)` and `@` refused for cmd.exe.
Pages changed: [[codex-gate-live-measurement]] (new), [[agent-gate-adapters]],
[[agent-hooks-per-session-research]], [[0008-private-and-team-sessions]].
## [2026-11-12] query | Activity calendar git contract
Documented the `git_activity` command (own commits of the last year for the activity calendar in
the empty window): argv, limits, deduplication and what is skipped. Pages changed:
[[agent-workflow-features]].

## [2026-11-12] query | File viewer: preview, images, diff against HEAD, live refresh; tree filter and drag
Filed the contract of the new viewer commands (`file_viewer_stamp`, `file_viewer_diff`,
`file_viewer_open_link`), the image and Markdown rules, the live refresh by polling, the tree's
filter with its bounded path index (`list_session_paths`) and dragging tree rows onto a terminal.
Pages changed: [[file-viewer]], index.

## [2026-11-12] ingest | Gate adapters for Pi, Oh My Pi, OpenCode and Droid
Built the four recipes from [[agent-hooks-remaining-agents-research]]: an in-process extension
for Pi and Oh My Pi (`-e`), an OpenCode plugin in a per-session `OPENCODE_CONFIG_DIR`, and a Droid
`--settings` file running `hangar gate-hook droid pre-tool-use`. The extension and plugin ask the
endpoint with `node:http` (Node's `fetch` sends `sec-fetch-mode`, which the endpoint refuses) and
fail open. Approved live measurements: OpenCode 1.19.5 (three turns, free model) and Pi 1.5.2
(two runs ended at expired sign-ins, the third used a live Pi with a scripted model); both are
offered from those versions. Oh My Pi and Droid are not installed and stay `measured = false`.
Pages changed: [[opencode-gate-live-measurement]] (new), [[pi-gate-live-measurement]] (new),
[[agent-gate-adapters]], [[agent-hooks-remaining-agents-research]], index.

## [2026-11-12] ingest | OpenCode gate through OPENCODE_CONFIG_CONTENT; review fixes
Security review follow-up. Two approved live OpenCode 1.19.5 turns: the gate plugin named as a
`file://` URL in `OPENCODE_CONFIG_CONTENT` loads without a configuration folder or npm install, a
deny still keeps the write from happening, and a temporary plugin in the user's
`~/.config/opencode/plugins` kept loading next to it. The recipe now uses that variable
(`GATE_ENV_NAMES`, conflict with a profile's plain or keychain `OPENCODE_CONFIG_CONTENT`, not
offered where Hangar's environment sets it). Conflicts now also look at keychain variable names.
The in-process plugins look tools up as own keys only, read patch text only from the patch tools'
named fields, take paths as given, and drop `@` and expand `~` only for Pi.
Pages changed: [[opencode-gate-live-measurement]], [[agent-gate-adapters]],
[[agent-hooks-remaining-agents-research]], index.

## [2026-11-12] ingest | Local dictation research
Compared whisper.cpp (whisper-rs), Vosk, sherpa-onnx, Windows and macOS speech APIs, the Web Speech
API and microphone capture (cpal versus getUserMedia). Recommendation: whisper-rs with small q5_1 and
base q5_1, cpal in the backend, push-to-talk, consent-based model download. Latency and German quality
are unmeasured. Pages changed: [[local-dictation-research]] (new), index.
## [2026-11-12] query | Git actions in the Changes tab
Documented `git_stage`, `git_unstage`, `git_discard` and `git_commit` (session ID, literal
pathspecs on stdin, path rules, filter refusal, hooks not bypassed) and the line comments to the
agent. Pages changed: [[commit-history]], index.

## [2026-11-12] query | Git actions hardened after review
Discard refuses paths through or holding links (junctions included), stage, unstage and discard
run without repository hooks, deadlines scale with the path count (commit 600 seconds) and name
the index lock, commit messages refuse control characters, "stage changed and commit" stages
tracked files only, git's environment drops a calling agent session's markers, and diff line
comments have a keyboard path. Pages changed: [[commit-history]].
## [2026-11-12] query | Session resources and push to the phone
Filed the contracts of two 0.6.0 features: `session_resources` (CPU share of the machine and
resident memory per session process tree, read from the Job Object on Windows and `/proc` on
Linux, outlier above 90 % CPU for 30 s or 4 GiB, polled while the window is visible) and the push
commands (`push_settings`, `push_save`, `push_test`, `push_session`; ntfy or Pushover, secrets in
the keychain service `hangar-push`, https only, no redirects, 30 s debounce per session). Linux
sampling and the services' formats are marked unverified.
Pages changed: [[session-resources-and-phone-push]] (new), index.

## [2026-11-12] query | Session resources and push: review rework
Outliers now count core equivalents (0.9 cores for 30 s), answers are cached for 1 s and the
trees are read outside the session lock; processes no longer in the job are skipped. Pushes are
built in the backend from program and running number (session names only by opt-in), the ntfy
token is bound to its origin, open topics need 16 characters, and there are global (10 a minute)
and test (5 s) limits. `Cache: no` checked against the ntfy publishing docs.
Pages changed: [[session-resources-and-phone-push]].
## [2026-11-12] query | Gate rules and team-capability reasons
Filed the design of two gate additions: remembered answers ("always" or "never" for a file or
folder of one project, applied in `gate_service.rs` before the window is asked, deny wins, stored
tolerantly in `gate-rules.json`, a log of automatic answers) and `TeamBlock`, the reason a program
cannot be a team session, shown as one collapsed line under Team for fixable reasons only.
Pages changed: [[gate-rules]] (new), [[agent-gate-adapters]], index.

## [2026-11-12] ingest | System bar and session rows without tooltip
Filed the system bar contract (`system_usage`: machine CPU from GetSystemTimes or /proc/stat,
memory from GlobalMemoryStatusEx or /proc/meminfo, GPU as the busiest engine of the PDH counter
GPU Engine Utilization Percentage, one shared poll every 1.5 s, same setting as the session
resources) and the own measurement (about 2 ms per reading, Get-Counter cross-check). Session rows
lost their tooltip; overlap, gate, exit code, outlier, resources and dev server moved into the peek.
Pages changed: [[session-resources-and-phone-push]], [[system-usage-measurement]] (new), index.

## [2026-11-12] query | System bar layout and peek for sessions on screen
The system bar's gauges became tiles with the sparkline as background; measured headless at 200,
272 and 560 px in all 12 languages without overflow. The peek now also shows for the active and
pane-placed sessions (without terminal lines) and names the program and version; the context
meter and the marker dot in a session row lost their tooltips.
Pages changed: [[session-resources-and-phone-push]].

## [2026-11-13] ingest | Token usage in Claude Code transcripts, costs and handover
Measured the usage fields of a Claude Code 2.1.322 transcript (keys and counts only: 1345
assistant records, 771 ids, identical usage per id) and filed the price table's source. Filed the
contracts of the cost overview (per session and local day, ledger, price table, budget notice) and
of the session handover; Codex and the other agents show no costs.
Pages changed: [[token-usage-measurement]] (new), [[costs-and-handover]] (new), index.

## [2026-11-13] query | Agent arena
Filed the design of the agent arena: worktrees `hangar/arena-<id>-<agent>` from one commit read
once, the task pasted with Enter once a session is quiet, diff stats against the base commit with
new files counted, the test command split into argv (no shell, PATH only, time limit, process tree
ended), taking the winner over through `git apply` plus copied new files with refusal on files the
project folder changed, and forced removal only for confirmed arena worktrees. No cost column yet.
Pages changed: [[agent-arena]] (new), index.

## [2026-11-13] ingest | Notes panel split into Private and Shared
The notes panel's two groups (Shared, Only on this computer) became a segmented switch
Private | Shared with counts above the list, shown only where the project can have shared notes.
New notes and quick notes go to the chosen side, captured terminal output always to the private
notes; the choice is remembered in `localStorage`.
Pages changed: [[notes]], [[0006-encrypted-project-notes]].

## [2026-11-13] query | Account details and profile picture
The window could stay without the signed-in person's name for a whole run: `/me` was loaded once
and `account_status` retried it only when it read the keychain itself. Documented the background
load with retries, the view revision, and the profile picture contract (`avatar_url`,
`avatar_etag`, `GET /me/avatar`) with the backend's download rules and cache.
Pages changed: [[hangar-account-api]].

## [2026-11-13] query | GPU per session and memory amounts in the system bar
Session rows got three lines (state, duration, folder and branch; then CPU, RAM, GPU, run time
and context meter). The GPU numbers per session tree come from a PDH reading shared with the
system bar (gpu.rs: engine utilization, process and adapter dedicated memory), Windows only. The
system bar adds RAM and GPU memory amounts from 340 px and in the tooltip.
Pages changed: [[session-resources-and-phone-push]], index.

## [2026-11-13] query | Session row third line and system bar amounts at narrow widths
Measured the session row's third line and the system bar in Edge at 305 to 505 px; the line now
gives way in a fixed order (token count, bar, names to icons, whole parts wrap away) and the
system bar shows RAM and GPU amounts from 305 px.
Pages changed: [[session-resources-and-phone-push]]; raw: 2026-11-13-session-row-layout-measurement.md.

## [2026-11-13] ingest | HANGAR.md and the upstream check of worktree sessions
Filed the own measurement of the instruction and first prompt options of Claude Code 2.1.323,
Codex CLI 0.169.0 and Aider 0.86.2 (help texts, Codex binary strings; Gemini CLI not installed)
and the contract of HANGAR.md (catalog fields `instruction_args`, `prompt_args`, `[start_prompts]`,
length limit, cmd.exe launchers) and of the first prompt of a fresh worktree session.
Pages changed: [[agent-instruction-flags-measurement]] (new), [[agent-instructions]] (new), index.

## [2026-11-13] ingest | HANGAR.md: launchers, command line length, secrets
Filed the npm shim chain of Codex 0.169.0 (`codex.cmd` runs `node codex.js`, which resolves and
spawns the platform package's `codex.exe` with its own environment) and Claude Code 2.1.323
accepting `--append-system-prompt=<value>`. The text is never handed to a cmd.exe launcher and
Settings names such programs; the length check covers the fully quoted command line with the gate
options; the template and the Settings help warn against secrets.
Pages changed: [[agent-instruction-flags-measurement]], [[agent-instructions]].

## [2026-11-13] ingest | Terminal links
Read xterm.js 6.0.0's Linkifier and RenderService (line cache, re-query on viewport render,
reply map reset with late callbacks) and measured Windows dropping trailing dots and spaces
from file names. Filed the terminal links behavior and the allowlist decision for opening
clicked files.
Pages changed: [[xterm-linkifier]] (new), [[windows-path-normalization-measurement]] (new),
[[terminal-links]] (new), index.
## [2026-11-13] query | Files tab inserts paths per session program
Right click and drag from the Files tab now type the path the way the target session's program reads it: an @ mention for agents, quoted native paths for PowerShell and cmd, POSIX paths for Git Bash (`/c/...`) and WSL (`/mnt/c/...`); relative inside the session's folder, absolute outside, so any pane takes a drop. Paths with control characters insert nothing.
Pages changed: [[file-viewer]].
## [2026-11-13] ingest | Own names for the color schemes
Filed ADR 0009: the terminal color schemes and file viewer themes no longer carry third-party
product names (Harbor, Meridian, Studio, Anvil, Sundial, Velvet, Ember, Nocturne, Fjord, Ochre,
Neon, Heather); every 0.6.0 key migrates. Read the license files of all palette origins (MIT,
Apache-2.0 for Darcula and Tokyo Night, MIT/X11 for gruvbox) and the Monokai Pro license, which
forbids redistribution; Ember is now drawn from the classic 2006 Monokai values. Origins and
licenses in THIRD_PARTY_NOTICES.md. A trademark registry search for the new names is still open.
Pages changed: [[0009-own-color-scheme-names]] (new), [[file-viewer]], [[agent-workflow-features]], index.

## [2026-11-13] query | Session peek without CPU and RAM
The session row's third line shows CPU, RAM and GPU, so the peek no longer repeats them; it
keeps the outlier note. `resourceText` and the texts `session.resources` and
`session.resourcesMemory` are gone.
Pages changed: [[session-resources-and-phone-push]].

## [2026-11-13] query | Changes list opens collapsed
The changes list in the right panel (and a commit's files in the history) no longer opens the
last file with a diff by itself; every file starts closed and keeps the user's choice.
Pages changed: [[commit-history]].

## [2026-11-14] lint | Mask a personal path in a raw source
A history scan for secrets and private data found the local user name in the npm folder of `raw/2026-11-07-update-sources-measurement.md`. Masked it as `%APPDATA%\npm` under the masking rule and translated a leftover German note; the measurement itself is unchanged.

## [2026-11-14] ingest | Updater signing outside the build
`tools/release.py` no longer hands the updater key to `pnpm tauri build`: the build runs with `bundle.createUpdaterArtifacts: false`, and `tauri signer sign --app-version` signs the installer afterwards as the only process with the key. Measured with Tauri CLI 2.12.4 and a throwaway key that the signer writes `<file>.sig` with the same trusted comment as the build (`timestamp`, `file`, `version`) and accepts an empty password variable without asking.
Pages changed: [[release-rollout]], [[tauri-updater-and-nsis]].

## [2026-11-14] lint | Audit fixes reflected in the pages
Terminal path links now refuse links to anything but local fixed drives in every segment and no longer open SVG or XML files directly; relay tickets with plain `ws` are accepted only from a loopback server; `worktree add` runs without repository hooks and filters.
Pages changed: [[terminal-links]], [[hangar-account-api]], [[agent-workflow-features]].

## [2026-11-14] ingest | Transcript fields for the session report
Measured (key names and counts only) which Claude Code main transcript fields carry prompts, slash commands, tool calls, sub-agent runs, times and the branch, and that Codex 0.169.0 still has no flag that fixes the thread id. Filed the contract of `export_report` ("Export as report", Claude Code only).
Pages changed: [[session-report-transcripts]] (new), [[agent-workflow-features]].

## [2026-11-14] query | Queued messages and the stall hint
Filed how a queued message decides when to go out (the tracker's new `endedBy`, armed and disarmed messages, no send after a restore) and how the stall hint tells progress from a ticking spinner. Noted why the texts live beside `workspace.json` instead of in it (`deny_unknown_fields`).
Pages changed: [[queued-messages-and-stall-hint]].

## [2026-11-14] ingest | Agent prompt strings and the queue's question guard
Read short phrases from the installed Claude Code 2.1.325 and Codex 0.169.0 executables (nothing run) and how Claude Code picks its notification channel: `auto` rings no bell in Hangar's terminal, so a permission dialog ends quietly. The queue now checks the screen for a question before sending.
Pages changed: [[agent-prompt-strings-reading]], [[agent-prompt-shapes]], [[queued-messages-and-stall-hint]].

## [2026-11-14] ingest | Worktree cleanup
A dialog lists the worktrees Hangar created (its own layout only, found below the worktrees folder and in the records of the repositories Hangar knows) with state, size and last change, and removes the safe ones after the backend checked the state again; a branch goes only when merged into the default branch, deleted against the checked commit. Measured with git 2.56 that `worktree remove` on a folder deleted by hand takes away only that record, so the removal after closing a session no longer runs `worktree prune`. Causes of an entry that cannot be checked are codes the window words itself; worktrees created in this run count as in use until their session starts; ignored files keep a worktree on purpose.
Pages changed: [[agent-workflow-features]], [[worktree-remove-missing-measurement]].

## [2026-11-14] schema | Command guard decided and built
ADR 0010: the agent gate asks before a team session's agent runs a shell command that matches a rule of `command-guard.toml` (written with commented defaults on first use). Claude Code's matcher gains `Bash|PowerShell`, the Pi and OpenCode extensions send `bash` calls; Codex and private sessions stay uncovered with the reasons. Remembered answers are gate rules with scope `command`; no answer within 10 minutes denies. Shell tool input shapes are not measured live and are marked so.
Pages changed: [[0010-command-guard]], [[gate-rules]], [[agent-gate-adapters]].

## [2026-11-14] ingest | Shell tool input shapes; command guard reworked after review
Read the installed agents for their shell tool inputs (Claude Code transcripts, Pi 1.5.2 `bash.js` and `powershell.js`, OpenCode 1.19.5 binary strings) and filed them as a source. ADR 0010 reworked after the security and acceptance review: one lexer per dialect, every matching rule counts, built-in rules for commands too complex, read from stdin, too long or naming Hangar's own settings, a full queue denies, `cwd` problems no longer fail open, OpenCode's `workdir`, translated wording of Hangar's own rules, the residual risk stated.
Pages changed: [[shell-tool-input-shapes]], [[0010-command-guard]], [[agent-gate-adapters]].

## [2026-11-14] schema | Command guard: second security review
ADR 0010 updated: brace expansion capped at 8 braces and the hook threads given an 8 MiB stack (a deep expansion overflowed the stack), `hangar-config` narrowed to the config folder, `<local data>/gate` and the two settings files (Hangar's own worktrees in the data folder are ordinary projects), redirection targets checked, wildcards and 8.3 names against the protected names, shell options with values, PowerShell arguments in parentheses, `wsl` without `-e`, `start` and `Start-Process` unwrapped, empty special parameters, `--version` and `--help` no stdin scripts, a bad `cwd` dropped for edit tools too.
Pages changed: [[0010-command-guard]].

## [2026-11-14] schema | Command guard: third security review
ADR 0010 refined: a wildcard needs five letters of its own outside a plain extension to stand for one of Hangar's names, bracket classes count as wildcards, a wildcard right below the app settings folders counts, `start` unwrapping counts toward the depth limit, a single plain string in PowerShell parentheses is read as that string.
Pages changed: [[0010-command-guard]].

## [2026-11-14] ingest | Agent commands and compacting from the context meter
Read the slash command tables of the installed Claude Code 2.1.325 and Codex CLI 0.169.0
executables and Gemini CLI's command reference (not installed). The catalog gets `agent_commands`
for the three; profiles can add their own. The session menu, quick access and a compact button in
the row actions (from 80 % context use, hidden while the agent works) send them like "Send to
several". Binary strings also show that Codex refuses such commands during a turn and Claude
Code queues them; the compact button therefore also hides after a bell or during a gate decision.
Pages changed: [[agent-commands-measurement]] (new), [[agent-commands]] (new), [[catalog]], index.

## [2026-11-14] ingest | Power detection for the power saving mode
Saved Microsoft's `SYSTEM_POWER_STATUS` reference and the kernel's `sysfs-class-power` ABI to raw, with one own reading of a Windows 11 desktop (AC 1, battery flag 128). The power saving mode maps them to `noBattery`, `ac`, `battery`, `saver` or `unknown`; laptops on battery and Linux are not measured yet.
Pages changed: [[power-status-sources]], [[power-detection]].

## [2026-11-14] ingest | Project overview in the home view
New synthesis for the project overview: how folders become projects, the new `git_repo_places` command and why the existing commands did not fit, what a card shows and when git is asked. Measured with git 2.56.0.windows.4 that a linked worktree's `.git` file names an absolute git folder and its `commondir` holds `../..`.
Pages changed: [[project-overview]], [[git-worktree-commondir-measurement]].

## [2026-11-14] query | Work log and daily review
Decision on the work log behind the daily review: what is stored per session (key, title, program, folder, repository, worktree flag, times), UTC day files, 90 days, size limits, never terminal content or prompts; how the review computes run and active time as unions and which git calls collect the day's commits and files.
Pages changed: [[0011-work-log-and-daily-review]].

## [2026-11-14] query | Agent commands use the queue's question guard
After merging the queued message and the agent commands, commands also check the screen for a question or choice list (`asksQuestion` on top of `looksLikeQuestion`): the menu and quick access hide them, every send checks again, and the compact button's click into a question sends nothing and says why.
Pages changed: [[agent-commands]].

## [2026-11-14] query | Stall hint on battery
The stall hint now follows the power saving mode: `pollInterval("stall")` gives 10 s on AC and 30 s on battery, and `StallMonitor.setSampleInterval` restarts its timer without resetting the time since the last progress. It keeps its own timer on the injected clock instead of `PollTimer`.
Pages changed: [[power-detection]], [[queued-messages-and-stall-hint]].

## [2026-11-15] query | Frontend security fixes from the audit
OSC 8 links show their real target on hover; a click whose visible text names another host or looks like a path shows the address with an Open button first (`osc8Decision`). Ctrl+V and Shift+Insert ask before text that would run line by line or hides characters (setting `warnMultilinePaste`, on by default). Broadcasts, queued messages and agent commands skip a program without bracketed paste when the text has a line break or invisible characters. Dropped paths with control characters are left out, Aider's `(Y)es/(N)o` prompt counts as a question ([UNVERIFIED]), and link trimming is a single pass.
Pages changed: [[notes]], [[queued-messages-and-stall-hint]], [[terminal-links]].

## [2026-11-15] update | Git hooks off by default, capped reads, more redaction
Every git call now passes `core.hooksPath` below the git program file and `hook.<event>.enabled=false`
for every event (config-defined hooks of git 2.54+), so no repository hook runs
(shared notes staged conflicts with `git add` and could run `post-index-change`); only the commit
of the Changes panel opts in. Files below `.git`, notes and the file viewer read regular files
only, capped, without following a final link or blocking on a FIFO. The work log no longer
follows a `.git` file to another share, and the daily review asks a repository on a share only for
sessions on that share. The excerpt filter masks `.netrc` passwords, XML secret elements,
multi-word `password:` values and Slack and Discord webhook URLs. Worktree cleanup compares
resolved paths too.
Pages changed: [[agent-workflow-features]], [[commit-history]], [[0011-work-log-and-daily-review]], [[file-viewer]].

## [2026-11-15] query | Third team security review: membership, admission, key change
An owner's or admin's device reads the website's member list (`GET /api/v1/projects/{id}/members`, not offered by the website yet) and removes members it no longer names from the log, which makes the next epoch leave their devices out; roles follow the website on the owner's device. A newcomer shares nothing until the person confirms the admitting device's and the log's safety numbers (stored with the head, `StoredKeys` v2), and the log's owner must be the website's `owner_id`. A member device seals nothing while a rotation is due. Relay-triggered website checks at most every 30 s, overlaps capped per teammate device with details only under a signed status, teammate texts without control or reordering characters, plain http only to literal loopback addresses in release builds, the ntfy address in the keychain, and the first `welcome` must name the signed-in account. `account-changed` and `team-changed` are payload-free signals now (Tauri also delivers `emit_to` events to any-target listeners in other webviews); the main window reads `account_status` and the new `team_snapshot`.
Pages changed: [[team-crypto]], [[team-service-v1]], [[session-resources-and-phone-push]], [[hangar-account-api]].

## [2026-11-15] query | Hardening of process launch, command surface and update chain
Filed the security review's findings and their fixes: no program lookup in the working folder for every catalog agent, Unix close escalates to SIGKILL and never signals a reaped child, bounded session input (Busy, 1 MiB, frontend pieces and retry) and resize, update sessions always in the home folder, editor variables stripped, UNC and device folders refused, relative program paths refused, NUL in env values refused, package names start alphanumeric, git reads by session id, payload-free gate-changed, no listening in file viewers, CSP additions, requireSignedVersion, release key-id check and the Tauri CLI through node. Job Object assignment after spawn documented as accepted.
Pages changed: [[process-launch-hardening]], [[cmd-launcher]], [[program-updates]], [[commit-history]], [[0001-foundation]], [[release-rollout]].

## [2026-11-15] ingest | Stalled session input and folder errors in the user's language
A write that stalls for 30 s now throws `InputStalled`, still ends an abandoned bracketed paste, shows a translated terminal notice and keeps "Send to several" from sending Enter. `ManagerError` serializes `RemoteCwd` and `BadCwd` with a stable `[code]` prefix that the window translates.
Pages changed: [[process-launch-hardening]].

## [2026-11-15] decision | Command guard hardening after an audit
The command guard reads POSIX here-documents and PowerShell here-strings, brace expansion in every word, `$IFS` and default values, cmd substring expansions, abbreviated long options, `.`/`source` and `/dev/stdin` scripts, PowerShell `-Path:` values and pipelines into `Remove-Item`, more wrappers, `find -exec` and `trap`, and keeps the working folder before a `cd`. Claude Code's hook matcher is `*`; any tool with a command that is no edit is checked. An unreadable edit path asks instead of failing open; links into Hangar's folder are resolved; the extensions block a shell command they cannot ask about; silent connections cannot crowd out hooks, a burst of parallel hooks is never cut, and a session over its 32 connections is denied unread instead of left unanswered. A quote or `)` in a here-document inside `$(...)` no longer ends the substitution.
Pages changed: [[0010-command-guard]].

## [2026-11-15] lint | Payload-free team and account signals after the security merge
The process launch review still listed `team-changed` and `account-changed` as carrying a payload, and the account and team interface notes still named them as carrying their views; since the team security review both are payload-free signals and the main window reads `account_status` and `team_snapshot`.
Pages changed: [[process-launch-hardening]], [[hangar-account-api]], [[team-service-v1]].

## [2026-11-15] decision | Release tool: programs only from PATH, real signature verification
`tools/release.py` runs git, node, cargo and PowerShell only as absolute `.exe` programs from `PATH` outside the repository and the working folder, sets `NoDefaultCurrentDirectoryInExePath=1`, checks the installed Tauri CLI version against `pnpm-lock.yaml` and gives the signer a reduced environment without Node settings. After signing, `src-tauri/examples/verify_update_sig.rs` verifies the signature with the same `minisign-verify` call as `tauri-plugin-updater` 2.14.1, including the signed version; the key id is printed in minisign's order. The main window's updater permissions are narrowed to `allow-check` and `allow-download-and-install`; CI checks out without persisted credentials, builds with `--locked` and takes the pnpm version from `packageManager` only (pnpm/action-setup README at the pinned commit). Residual risk documented: `node_modules` is not integrity-checked.
Pages changed: [[release-rollout]], [[tauri-updater-and-nsis]].

## [2026-11-15] ingest | Second security round for links, paste and session input
OSC 8 link text is now judged as the whole word around the link (also through wrapped rows), word by word without punctuation, after NFKC and with ideographic dots; invisible characters, bare file names of another target and addresses with a user name ask or are refused. The file viewer's Markdown links get the same check. The paste question is asked again when bracketed paste went off meanwhile (residual mode lag documented), blank text sends no lone Enter, a stall drops the writes queued behind it, and dropped paths with bidi controls or line and paragraph separators are left out (zero-width joiners stay allowed for Persian, Indic and emoji file names).
Pages changed: [[terminal-links]], [[notes]], [[queued-messages-and-stall-hint]], [[process-launch-hardening]], [[agent-workflow-features]].

## [2026-11-15] update | Second security round on teams, accounts and crypto
Membership log capped at 8192 entries with 64 places kept for removing a member and rotating; a due rotation that only removes devices added after the epoch began is deferred to at most one per 10 minutes and holds no sharing; refused rotations are bounded per connection; a website promotion to admin waits for the owner's confirmation while demotions follow at once; the admission confirmation is bound to the numbers shown and armed with a delay; no binding before `/me` is known; no trust without a website owner for devices not admitted before; reports of other devices no longer stop the team; the `v1` keys migration needs an epoch secret; text cleaning drops invisible characters and line separators. Recovery derivation without team salt accepted with reasoning; relay rate limit filed as a server requirement.
Pages changed: [[team-crypto]].

## [2026-11-15] update | A full team log stays visible and names its way out
A log that reached its reserved places no longer goes unnoticed: every device reports it without stopping, requests that no longer fit stay listed as such, and owners and admins are told the way out that exists (remove the member on the website, a new project for new devices). A website without a project owner now stops new devices with its own reason, `NoWebsiteOwner`.
Pages changed: [[team-crypto]].

## [2026-11-15] update | Second security review of process launch and the command surface
The program lookup in the working folder is now decided on the resolved program (only shells keep it, also for user profiles that start an agent); the app's end sends SIGKILL synchronously on Unix; Linux session signals respect the reaping; working folders, git folders and shared notes are placed without a lookup leaving the machine (links included; a mapped network drive named directly stays allowed); the security switches and queued messages moved from `localStorage` into a main-window-only backend store; `claims-changed` and `program-update-result` became payload-free signals; the panel window reaches only the session the main window handed it; folders and Hangar's files open through ShellExecute; placeholders are filled in one pass; JetBrains askpass helpers are removed from the children's environment. The file viewer's InPrivate mode was investigated (Tauri 2.12.5, wry 0.57.1) and documented, not built.
Pages changed: [[process-launch-hardening]], [[program-updates]], [[queued-messages-and-stall-hint]], [[team-service-v1]], [[0007-detachable-side-panel]].

## [2026-11-15] decision | Repository config guard, worktree checkout, redaction and report fixes after an audit
Before every git call Hangar now reads the repository's config files and all their include targets itself (any `includeIf` condition, `git config --file --no-includes` started outside every repository, since git opens the repository's config with includes at startup even for `--file`, measured with git 2.56), refuses includes, `core.excludesFile`, `core.attributesFile`, `core.worktree`, `mailmap.file`, `diff.orderFile`, alternates and linked entries that lead to another share, and switches off the union of filter drivers and config hook names found; cached per git folder by size, modification time and link state. A worktree is added with `--no-checkout` and checked out with `reset --hard` inside it, so an `onbranch:hangar/**` smudge filter cannot run. No caller names its own `core.hooksPath` any more. Redaction covers escaped and URL-encoded JSON, qualifiers after the secret word, fullwidth separators, YAML block scalars, commas inside values, `ConvertTo-SecureString` switches first and URL passwords with `/`, and keeps numbers, booleans and calls. Reports escape `![` and `](` outside code. Project notes read files only as regular files on the open handle; a placeholder's second open goes through `ReOpenFile`, only for content tags.
Pages changed: [[agent-workflow-features]].

## [2026-11-15] decision | Command guard fails closed after a second audit
The command guard asks instead of guessing where it does not model a construct (here-document delimiters with `$`, backticks or CR, open `${`, brace expansions and nesting past their caps), reads `#` comments and `case` patterns inside `$(...)` and arithmetic contexts as bash 5.3 does, ends PowerShell words at script block braces, reads PowerShell parameters with every dash and `:value`, wrapper options as getopt does, cmd `if` conditions and `for` sets, every change of folder, Windows path parts of dots and spaces, SQL behind comments and `psql -f -`, git's value options before the subcommand in code for every git rule, `$_` in a piped `ForEach-Object` block as the listed items (only a listing call's paths count for a piped delete). No built-in rule can be remembered; an unreadable hook request of a known session is denied instead of refused; a connection whose head is read cannot be closed for a newcomer.
Pages changed: [[0010-command-guard]].

## [2026-11-15] update | Stage, unstage and discard create no hooks folder
The commands no longer create the empty `git-no-hooks` folder below the local data folder or take a path to it; git switches hooks off for them like for every other call, so a file left in that folder no longer blocks staging. The `.no-hooks` constant of the worktree folder is gone too; a dot name stays no valid slug.
Pages changed: [[commit-history]], [[agent-workflow-features]].

## [2026-11-15] update | Release script: key password prompt, build input check, verifier built first
`tools/release.py` asks for the updater key password at the console and refuses an empty one unless `--allow-unencrypted-key` is passed, and refuses to run while `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` is set in the environment. It aborts when an ignored Tauri config override, Cargo config, toolchain file or `.npmrc` would change the build; git runs without fsmonitor, hooks, filters and updater key variables; the signature verifier is built before `tauri build`, and the installer SHA-256 noted after signing must match the copies under `release/`. Measured with Tauri CLI 2.12.4 and throwaway keys: a key generated with an empty password is still scrypt-encrypted, and re-encrypting it with a new password keeps the key id. The page now gives a stdlib-only re-encryption script (getpass, decrypted key only in memory, writes `<key>.new`), verified on a throwaway key: wrong current password writes nothing, the new key signs with the new password only and keeps its key id. `tauri signer sign` never prompts; without the password variable it assumes an empty password. The page also recommends releasing from a fresh `git clone --no-local`.
Pages changed: [[release-rollout]].

## [2026-11-15] update | Third security round: link decision inverted, hidden cells, gate arm delay
`osc8Decision` (terminal OSC 8 links and the file viewer's Markdown links) no longer lists signs that may surround a host; it searches every host-like run in each word and opens at once only when there is none or each is the target's site, with a named port and scheme matching. Combining marks are dropped and UTS 39 look-alikes of the dot and slash are read as such; an internationalized target host always asks. The visible word now follows a hard line break that looks like a wrap and treats invisible or same-color cells as gaps; printed addresses end where the visible text ends and show their target on hover. The viewer judges a link with the text touching it in its block. Gate cards ignore answer clicks for 600 ms after they appear. An OSC 52 write counts as the program's selection only while the program tracks the mouse.
Pages changed: [[terminal-links]], [[gate-rules]].

## [2026-11-15] update | Third security round on team membership freshness
A new member always joins the log as a member; a higher website role becomes the owner's own role change confirmation. Every admitted device reads the website's member list: when the log still names a member the list left out for 10 minutes, a device that cannot remove members stops sealing with its own notice, and without a list nothing changes. A deferred rotation no owner or admin makes stops sealing on other devices after 10 minutes. A teammate's signed newer log head the relay does not hand over shows as held back history. Paged member and project lists count as no answer. The last log place is kept for the rotation a removal makes due (crate rule, relay and clients agree), and only people the website lists are offered for admission. The 256 entry bound of rejected confirmations is documented as accepted.
Pages changed: [[team-crypto]].

## [2026-11-15] update | Third security review of the command surface
The panel and file viewer windows hold no `core:window:*` permission any more (the window plugin's commands take any window's label); the panel closes and ends itself through `close_panel_window` and `destroy_panel_window`, which act on the calling window only. `PanelScope` keeps the handed session's working folder: from the panel, shared notes are reachable only for that folder and text only for that session. The paste warning, automatic updates, HANGAR.md, the snippets and the workspace templates moved from `localStorage` into the main-window-only backend store, migrated once and stored even when empty. `answer_gate` and `command_guard_info` run off the main thread. Git and the file manager no longer inherit session markers, the gate's token or an editor's git variables (`child_env::scrub`). Folders open through the shell's `Folder` class. Bounds for session titles, note reorders and the daily review's window. The file viewer's InPrivate mode was evaluated again and not built.
Pages changed: [[process-launch-hardening]], [[0007-detachable-side-panel]], [[notes]], [[file-viewer]].

## [2026-11-15] decision | Repository config guard and redaction after a third audit
The guard now takes every folder git's `is_git_directory` would accept on the way up (a `HEAD` entry, then `objects` and `refs` of any kind in the folder `commondir` names; Windows ignores `X_OK`), reads `.git` files and `commondir` up to 1 MiB whole with only trailing line breaks removed, as git does, and refuses one it cannot read or check instead of skipping it (also in `check_git_links`); alternates files are read to depth 5 inclusive, as git reads them, with at most 256 entries; every file it opens is looked up by a walk that follows only links to local fixed drives, so a folder linked to a share in the middle of a path is refused. Redaction covers whole `.env` values, `=>`, `:=`, `<-`, subscripts, `SetEnvironmentVariable`, `setx`, fish `set -x`, Dockerfile `ENV`, name-and-token lines, YAML values on the next line, `.NET` `<add key value>` entries, more Slack and AWS prefixes, Teams webhooks, spaced key blocks, PuTTY private lines and `Your API key is <token>`.
Pages changed: [[agent-workflow-features]].

## [2026-11-15] decision | Command guard reads as the shells do after a third audit
Claude Code's `PreToolUse` hook now carries `"onFailure": "block"` for an installed Claude Code 2.1.295 or later (older or unknown versions keep the earlier settings), so a hook that cannot connect, times out or gets an error blocks the tool call; with the gate off the endpoint answers every tool hook "no decision" instead of an error. The guard reads multi-word inline scripts joined as the shell joins them, PowerShell assignments, `return`, `throw`, the call operator, backtick escapes, literal subexpressions and comma lists, cmd blocks, carets, `start` titles and glued switches, more wrappers, positional parameters of inline scripts, substitutions in bash builtins that evaluate subscripts, wildcard option words, `.git` wildcards and short names, glued SQL values and database shell escapes, git configuration naming a program; new default rules `git-push-delete`, `dropdb`, `mysqladmin-drop`. The Pi and OpenCode extensions block a shell command when their own handler fails and check any tool with a `command`. The audit's 925 inputs (350 of them passed before) and 393 everyday commands are test data under `src-tauri/tests/data/`.
Pages changed: [[0010-command-guard]].

## [2026-11-15] decision | Team removals in a full log, overdue scope and release binding after a fourth review
The first half of the membership log's reserved places takes remove device entries as well, so a stolen device can be removed when the log is full, while the last 32 places stay for removing members and changing the key; a refused removal reads `logFull`. Only a removal of a device's own member's devices makes a deferred key change overdue on that device, so one member's churn no longer pauses everybody's sharing. A member who left a log at its last place shows the new waiting reason `logFull` with the new project way out. An admission without numbers cannot be confirmed. Hangar removes the variables of a calling agent session, its gate and an editor from its own environment at startup, so programs Windows' shell opens for it inherit none of them. The release script hashes the installer right after the Authenticode check, requires that hash around the updater signer and verifies the uploaded copy against the signature text `latest.json` gets.
Pages changed: [[team-crypto]], [[release-rollout]], [[process-launch-hardening]].

## [2026-11-15] decision | Repository config guard and redaction after a fourth audit
The guard now also checks the back-links of linked worktrees: git 2.56 opens the folder each `worktrees/<id>/gitdir` of the common git folder names on `worktree list`, `add`, `remove` and on a commit (an SMB contact was measured), so a back-link to another share or device refuses the call before git runs; relative targets resolve against `worktrees/<id>`, more than 256 entries refuse the call, and the `worktrees` folder and each `gitdir` file are stamped for the cache. Every git call gets `-c submodule.recurse=false`, so a repository setting cannot take a restore into submodules whose filters were never read. The project notes check the main worktree's path text before touching it. Redaction covers `.pgpass` lines, XML entries named by a `key` or `name` attribute, config commands that set a secret key, Markdown table rows, fallback literals of secret variables, the password flags of `redis-cli`, `ldap*`, `smbclient`, `net use`, `openssl` and `twine`, `echo ... | ... --password-stdin`, and the prefixes `pypi-AgE`, `whsec_`, `AGE-SECRET-KEY-1`, `glsa_`, `HRKU-` and Telegram bot tokens.
Pages changed: [[agent-workflow-features]].

## [2026-11-15] update | Fourth security round on link judgment and main window state
Terminal and viewer links are judged by their own text and the word around them; a click opens at once only when neither misleads. Digits glued to a host's last label (also superscript, circled and subscript ones) no longer hide it. File endings that are delegated TLDs (`java`, `md`, `py`, `rs`, `sh`, `zip`) make a host unless the name is in clear file form and the target's file. The dot look-alikes come from `confusables.txt` (prototypes U+002E, U+00B7, U+0323, U+0971), mapped before NFKC, letters and marks only between ASCII letters or digits; modifier letters between them count as a dot. `cellHidden` resolves default and palette colors through the active scheme, inverse and dim, and needs both a low contrast and close channels. The discard question in the Changes panel keeps the session and folder it was asked for, names the folder and closes on a switch. Start mode, last program, folders, session restore and the notice settings moved into the main-window-only store (`main-prefs.json`), migrated once; the panel's window prefs are read strictly.
Pages changed: [[terminal-links]], [[process-launch-hardening]].

## [2026-11-15] decision | Command guard after a fourth audit
The guard now strips a cmd `@` in front of any command (also after `do`/`if`/`else`) and reads a literal `for` set as placing the loop variable (`for %d in (.git) do rd /s /q "%d"`, recursive `/r` roots too); reads `find -exec {}` as `<start>/.git` as well when the entries are not limited to a non-`.git` name; unwraps program launchers and container exec (`docker|podman|kubectl exec`, `docker run|compose`, `su -c`, `runuser --`, `npx`, `pnpm|yarn exec|dlx`, `uv|poetry|pipenv run`, `conda run`, `mise exec`, `direnv exec`, `dotenv`, `cross-env`, `op|doppler|infisical run`, `aws-vault exec`, `nix-shell --run`, `nix develop -c`, `systemd-run`, `xvfb-run`, `strace`, `gsudo`, `gtimeout`); asks about writes into the folder that holds Hangar's settings folder (the parent or an app-data root) when another argument names the identifier or a settings file, and always for extractors and mirror tools pointed there; protects Claude Code's `.claude/settings.json` and `settings.local.json` (project and home) like Hangar's own settings, in commands and edits; reads a leading `*` as able to reach `.git` under `dotglob`. False alarms removed: `git push --dry-run --force`/`-nf` (a `none` option, lifted only on a definite match that is no option's value and not negated by `--no-dry-run`), and literal `for`/`foreach`/`ForEach-Object` loops over in-project folder names. A failed Claude Code version probe is no longer cached as `None`, so the gate's fail-closed `onFailure` step is retried. New ASK and PASS rows under `src-tauri/tests/data/command_guard_audit.txt`.
Pages changed: [[0010-command-guard]].

## [2026-11-15] decision | Command guard rework after the fourth audit review
Hardened the fourth audit, fail-closed. A `for`/`foreach` loop variable is placed in its body only when the whole command writes it exactly once (its own literal header) and runs no inline script; a second write anywhere (another loop, `d=`/`+=`, `export`, `read`, `printf -v`, `mapfile`, `getopts`, PowerShell `$d =`, `$d++`, `Set-Variable`) leaves it unplaced, which `command_split` tracks by recording every write in the raw words (also after `do`/`then`, PowerShell names ignoring case, scope and braces, list assignments, bare words naming a variable to a cmdlet; a dot-sourced script or `declare -n` binds nothing). Claude Code hook settings are recognized through an NTFS data stream, a Windows 8.3 short name, a wildcard, and a split source/`.claude` folder (`cp settings.json .claude/`); any touch asks, reads included, because the content is not visible. Extractors into the config parent now also match an old-style `tar xf`, a glued `-C<dir>`/`-o<dir>`/`--directory=<dir>`, `xcopy`, a wildcard beside the parent, and the working folder being the parent. Launchers are read at every offset past their global options (`docker -H`/`--context`/`container`, compose `-f`/`-p`, `kubectl -n`, `uv --directory`, `pnpm -C`, `poetry -C`, `gsudo -u`, `strace -o`, `su --command=`/`-c<glued>`), so an unknown option cannot hide the inner command; a pipe into a container shell is a stdin script, while a lone interactive shell is not. A failed version probe is kept in a 60 s negative cache (`version::NEGATIVE_TTL`) instead of being dropped, so a hanging `--version` does not block every profile list while the gate's fail-closed step still returns after the window. New rows under `src-tauri/tests/data/command_guard_audit.txt`.
Pages changed: [[0010-command-guard]].

## [2026-11-15] update | Session rows built by the user instead of fourteen fixed styles
Settings > Appearance > "Session row" is a row builder now: four lines with a left and a right part, ten elements (title, state, program, signs, folder, branch, load, run time, context, tokens), a form per element where it makes sense, card frame and compact density. The layout is parsed strictly, planned once per layout, and stored as `rowLayout` with the other appearance settings; a stored `rowStyle` becomes its preset once. Accordion, counters and columns map to their closest layouts (no chevron, no counts on the group header, no column heading). Groups, tree guides and sub-agent rows are unchanged.
Pages changed: [[session-row-layout]].

## [2026-11-15] update | Row builder acceptance measurements
Tree guide geometry of all presets compared against main in Edge 157 headless (identical), editor drag, keyboard and focus driven over the DevTools protocol; arrow buttons at an edge are now `aria-disabled`.
Pages changed: [[session-row-layout]].

## [2026-11-15] update | Costs and handover merged with the security rounds and the row builder
`cost-usage-changed` became a signal without payload; the main window reads `cost_report`. The
handover reads the last request from its own tracker in the terminal (the last prompt bar is
gone), the changed files by session id, and its menu entry shows only while another installed
agent can take over. The cost line stays in the peek, after the row builder's load and context.
Pages changed: [[costs-and-handover]].

## [2026-11-15] update | Costs and handover security review
The daily budget and the day its notice last showed moved from localStorage into the protected
`main-prefs.json`, validated by the backend and migrated once. The price file and the ledger are
read with a byte cap instead of a size check before the read. A handover whose new agent never
turned on bracketed paste is no longer pasted; a notice offers to copy it.
Pages changed: [[costs-and-handover]].

## [2026-11-15] update | Agent arena on the hardened worktree code
After merging the security rounds the arena follows the worktree rules of main: worktrees are added with `--no-checkout` and checked out inside, discarding names no `core.hooksPath` of its own and takes a hand-deleted folder's record with `worktree remove` instead of `worktree prune`, `arena_create` refuses share and device paths and marks its worktrees as pending for the cleanup, and arena sessions get no upstream check. Taking over now requires the project folder to be the top of the repository that lists the contestant's worktree.
Pages changed: [[agent-arena]].

## [2026-11-15] ingest | Dictation measurement
Filed the whisper-rs build and speed measurement of 2026-11-12/13 as a source page: libclang and CMake requirements, the cmake crate dropping MSVC optimization flags and the `CMAKE_PROJECT_INCLUDE` fix, the AVX2 baseline, speed, RAM and hand-scored word errors of base and small q5_1.
Pages changed: [[dictation-measurement]] (new), [[local-dictation-research]] (index line), index.

## [2026-11-15] update | Local dictation as built
Hold Ctrl+Shift+Space to dictate into the session active on key down. Documented the states, commands and events, the model download (pinned revision, size, SHA-256, `.part` then rename, Hugging Face hosts only), the audio path and privacy rules, the key handling, the build and CI needs (CMake, C++ compiler, libclang, ALSA headers on Linux) and the platform notes (`NSMicrophoneUsageDescription`, entitlement and IME behavior unverified).
Pages changed: [[local-dictation]] (new), [[overview]], index.

## [2026-11-15] update | Dictation acceptance fixes
A quick tap no longer reads as a blocked microphone (no buffer yet is judged only after 500 ms), each transcript ends with one space so consecutive holds form a sentence, and the page records why the dictation choices stay in localStorage. `release.py` no longer refuses the tracked `.cargo/config.toml` as a hidden build input.
Pages changed: [[local-dictation]], [[release-rollout]].

## [2026-11-15] update | Dictation security and acceptance review
The dictation state and text go out as payload-free signals; the main window reads `dictation_status` and takes each transcript once through the new `dictation_take_text` (queue of 8). The level stays a number payload, with the reason on the page. Every dictation command checks the main window itself. Whether the key is on moved to the backend's `MainPrefs` with a one-time migration from localStorage; the model hash is checked once per run before the first load; a recording without stop ends after the cap plus 5 s and the window stops a stale one when it loads; the status names whether a microphone is there; repeated Space after an early Shift release no longer reaches the program; macOS gets the audio-input entitlement; licenses of whisper.cpp, ggml, whisper-rs and cpal listed.
Pages changed: [[local-dictation]].

## [2026-11-15] ingest | Work tree timeline
Filed the contract of the timeline in the Changes tab: snapshots live in a side repository per
repository below the app data folder (`GIT_OBJECT_DIRECTORY` plus the user's objects as an
alternate), so nothing is written into the user's git folder (measured in `nothing_is_written_into_the_users_repository`).
The key is bound to the session in the backend, steps above 50 MiB per file or 200 MiB together
are skipped, going back is refused while a session in the same work tree works and checks links,
filters, sparse entries, folders and ignored files in the way before the undo point is saved;
snapshots go after a successful close, on quit without restore, and at start for keys not in the
workspace.
Pages changed: [[work-tree-timeline]] (new), index.

## [2026-11-15] update | Agent arena: take-over and discard hardened
Paths into a git folder (any case, trailing dots or spaces, HFS+ ignorables, NTFS short names) and Windows device names are refused, new files are checked against the resolved git folder, links and submodules in the patch and links on the way are refused, new files are opened without following a link and created new, they go in before the patch and are taken back if it fails, the user confirms the exact file list (files that can run code marked), and discarding deletes the folder with `remove_dir_all` before git takes its record. Measured: Win32 trailing dot normalization, `git worktree remove --force` leaving a junction behind, `git apply -R` not restoring bytes under autocrlf.
Pages changed: [[agent-arena]].

## [2026-11-15] update | 0.8.0 integration of costs, handover, arena, dictation and timeline
Merged the four features into one branch. The session start takes one prepared worktree for the
handover and the arena (only the arena's fresh one is removed when its session fails), the
protected main window preferences carry the daily budget, its warned day and the dictation key,
and the terminal keeps the user's submitted lines in one `SubmittedLines` record that feeds both
the handover's last request and the timeline's last prompt.
Pages changed: [[costs-and-handover]].

## [2026-11-15] ingest | Timeline security rework
Measured the git behavior the jump back now relies on (`checkout-index --prefix` into a staging
folder, assume-unchanged entries, `gc` with an alternate, `File::try_lock` per handle) and filed it
as [[timeline-restore-measurement]]. The contract now describes staging plus installing each file
with a canonical check right before the write, the last look before writing (working states read
again, tree built again), symbolic link entries refused, assume-unchanged bits cleared, blob sizes
checked again, the cross-process lock for the clean-up, `gc`, the 2 GiB cap, side repositories of
missing drives kept, coded refusals, the view offered only where snapshots are taken, the prompt
kept only from the waiting state, and the residual risks.
Pages changed: [[work-tree-timeline]], [[timeline-restore-measurement]] (new), index.

## [2026-11-15] update | Timeline step prompt and prune target
The step's prompt is now the last line submitted while the agent waits for a task, fixed when the
working phase begins, so `/clear` or `/model` before the real prompt no longer names the step.
The undo point of a jump back no longer prunes the jump's target.
Pages changed: [[work-tree-timeline]].

## [2026-11-15] update | 0.8.0 integration of the timeline restore hardening
Merged the timeline's restore hardening and step prompt into the integration. The terminal's one
prompt line tracker now feeds two records: `SubmittedLines` for the handover's last request (agent
sessions only) and the timeline's `StepPrompt` (fixed when the work starts). The timeline's
`splitBackendError` with its detail line is the one kept; the arena reads its code from it.
Pages changed: [[costs-and-handover]].

## [2026-11-15] update | Prompt mouse editing: suggestion stays, selection deletes
A click in an agent's prompt no longer moves into Claude Code's faint suggestion (a Right arrow took it); the cursor stops at the end of the typed text and at the first character after the prompt mark. Backspace or Delete with a selection inside the typed text of the cursor's line deletes it (arrows to its end, one Backspace per character); elsewhere the keys behave as before. Read the suggestion's rendering from the Claude Code 2.1.325 binary.
Pages changed: [[prompt-mouse-editing]], [[claude-code-suggestion-reading]].

## [2026-11-15] update | Seven screenshot scenes per release
The README screenshots now come from seven scenes: the overview plus six for the release's most visible features; for 0.8.0 `home`, `queue`, `guard`, `review`, `worktrees` and `row-builder` replace `subagent`, `files`, `team`, `notes` and `settings`.
Pages changed: [[release-rollout]].

## [2026-11-15] update | 0.9.0 integration with the last prompt bar
Merged main (0.8.0 released, upcoming 0.9.0, last prompt bar back) into the integration. The
wave's features move to 0.9.0 in the changelog. The bar no longer keeps a tracker of its own: the
terminal's one prompt line tracker feeds the handover's `SubmittedLines`, the timeline's
`StepPrompt` and the bar, so deleting a selection in the prompt reaches all three alike.
Pages changed: [[costs-and-handover]].

## [2026-11-15] ingest | Sub-agent library and figures
Read `claude --help` of Claude Code 2.1.325 (`--agent <agent>`) and searched the binary for a name
rule (none for agents). Specified and built the sub-agent library: listing global and project
definitions, adding from text, figures (bundled, own images, clipboard) and starting Claude Code as
a definition; duplicates of one name in global and project folders are one row, project first.
Pages changed: [[sub-agent-library]] (new), [[claude-agent-flag-measurement]] (new), index.

## [2026-11-15] ingest | Sub-agent library after review
Read the Claude Code 2.1.325 binary for agent frontmatter fields (hooks and MCP servers apply to
the main-thread agent, untrusted folders skip them, a widening permissionMode is ignored). The
figure became a session's look, the figure row gave way to a start menu, project definitions are
marked and never wear a global figure, new files never overwrite and pick a free name, and the
figure store reads with limits.
Pages changed: [[sub-agent-library]], [[claude-agent-frontmatter-reading]] (new), index.

## [2026-11-15] update | Prompt editing across wrapped rows, resizable settings dialog
Click-to-move and deleting a selection in an agent's prompt now work across the rows xterm wrapped
the cursor's line onto itself (`isWrapped`), with the blank cell a wrapped wide character leaves
dropped from the counts. Prompts the agent wraps or breaks itself (Claude Code's own wrapping,
Shift+Enter) stay single line. A ConPTY recording of Claude Code 2.1.325 confirms that it ends every
prompt row with a real line break, so its prompts are not joined yet. The settings dialog can be resized by its edges and corners, grows to both sides,
is clamped to the window, snaps to device pixels and remembers its size.
Pages changed: [[prompt-mouse-editing]], [[resizable-dialogs]] (new), [[claude-prompt-wrap-measurement]]
(new, raw `2026-11-15-claude-prompt-wrap-measurement.md`), index.

## [2026-11-15] update | Prompt editing in Claude Code's own wrapped input block
Measured Right, Backspace, two blanks at a wrap, a row filled to the edge, a word split inside the word,
a wide character at the boundary, Shift+Enter and the right margin at 50 and 80 columns with Claude Code
2.1.325 through ConPTY (screen rebuilt with an emulator, never Enter). Click-to-move and deleting a
selection now treat Claude Code's input block (mark, indented rows, rules above and below) as one line,
counting a boundary as one character where the screen is certain and doing nothing where a word split,
a wide character or the cursor makes two readings possible. Hidden second blank at a wrap is the
documented residual risk. Gemini CLI is not installed and was not measured.
Pages changed: [[prompt-mouse-editing]], [[claude-prompt-wrap-measurement]].

## [2026-11-16] ingest | Crew column specification
Specified the crew column (sub-agents as still figures with speech bubbles beside the terminal, plinth
color per state, strip fallback, throttle, shared watch store, privacy). Removed the head motion of
figures from the plan. Pages changed: [[crew-column]], index.

## [2026-11-16] update | Sub-agent figures judged per session folder
The crew column and the sub-agent tree showed the neutral initial for every session as soon as
any project overrode a global definition of that name (Hangar's own `.claude/agents` did). They
now pass the session's folder to `runningFigure`, which follows `defsFor` like the session rows.
Pages changed: [[sub-agent-library]], [[crew-column]].

## [2026-11-16] update | Crew plinth carries the name
The plinth became a low round pedestal and shows the sub-agent's name instead of the state word;
the state shows by color alone on screen and stays as a word in the tooltip and accessible name.
Pages changed: [[crew-column]].

## [2026-11-16] update | Show the crew again from the session menu
A hidden crew could only come back through Settings. The session menu and quick access now
offer "Show crew" and "Hide crew" for sessions with sub-agents. Pages changed: [[crew-column]].

## [2026-11-16] update | Row preset "Hangar"
Added the owner's own session row from Hangar 0.8.0 as the first row preset, `hangar`, read from
that app's stored `rowLayout` and checked against its window. Pages changed: [[session-row-layout]].

## [2026-11-16] ingest | Sub-agent models and Codex CLI agent files
Read the sub-agent documentation of Claude Code and Codex CLI, `codex --help` and strings of the
2.1.325 and 0.169.0 binaries (model field, `CLAUDE_CODE_SUBAGENT_MODEL`, `[agents]` defaults,
`-c` overrides, effort names). Added the model per definition, a default model per program for
every session Hangar starts, Codex CLI definitions in the library and copies of the user's
definitions in Codex CLI's folder (marker, backups of replaced files). Pages changed:
[[sub-agent-models-research]] (new), [[sub-agent-library]], index.

## [2026-11-16] update | Sub-agent copies after review
Project folders: only untracked files of a definition's name are replaced (git `ls-files`), never
by a marker's source, and a linked `.codex` skips the folder; the folders come from the backend
(sessions' working folders, stored start places). Backups are named after the definition, a copy
edited by hand keeps naming the original, runs take one lock and report issues with a stable kind
that Settings shows. A marker names only a backup of its own definition, a removal reads the
marker again, and a model change never writes through a linked project agents folder. Pages
changed: [[sub-agent-library]].

## [2026-11-16] update | Seven screenshot scenes for 0.9.0
The README screenshots for 0.9.0 are the overview plus `crew`, `sub-agents`, `timeline`, `arena`,
`costs` and `dictation`; they replace `home`, `queue`, `guard`, `review`, `worktrees` and
`row-builder`. Pages changed: [[release-rollout]].

## [2026-11-17] update | ADR 0012 cross-project messages
Proposed how a session sends an instruction to the session of a partner project in another folder:
partner pairs, sending by hand or through `hangar-send`, delivery as bracketed paste after the
person's approval. Pages changed: [[0012-cross-project-messages]], index.

## [2026-11-17] update | ADR 0012 accepted
The owner accepted the cross-project messages decision; the next step is measuring per catalog
CLI that a pasted message with a header line arrives as one prompt. Pages changed:
[[0012-cross-project-messages]], index.
