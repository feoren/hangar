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
