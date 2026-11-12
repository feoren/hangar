# Index

Catalog of all wiki pages. Updated on every ingest.

## Overview
- [[overview]]: state of knowledge, open risks and questions

## Decisions
- [[0008-private-and-team-sessions]]: accepted: one switch "Private | Team" per start; a team session has its own worktree, the agent gate and is shared with the team, a private one has neither; team capability comes from the catalog's capabilities and `hook_format` only, presence and claims are agent-independent (sources: 1, updated: 2026-11-12)
- [[0007-detachable-side-panel]]: accepted: the whole right panel (Notes, Changes, Files) detaches into one backend-created `panel` window; the main window stays the source of truth and pushes a validated context, the panel answers through `panel_to_main`; replaces the notes-only window (sources: 0, updated: 2026-11-12)
- [[0006-encrypted-project-notes]]: accepted: shared project notes in `.hangar/notes/`, one encrypted file per note, keys from the team crypto (signed `notes-keys` record) or a solo recovery code, Git conflicts kept as both versions; phase 1 (solo mode) as built (sources: 1, updated: 2026-11-10)
- [[0005-team-collaboration]]: proposed: several people and their agents on one project; requirements from the owner interview, end-to-end encrypted Hangar service, reservations, live file claims, merge queue (sources: 0, updated: 2026-11-08)
- [[0004-notes-per-working-folder]]: notes belong to the working folder, stored as .md in the app data folder, keyed by the normalized path; folder names `general` and `.trash` since 2026-11-08; shared notes in [[0006-encrypted-project-notes]] (sources: 0, updated: 2026-11-10)
- [[0003-program-versions]]: program versions via a `--version` call with timeout and cache; program icons; queries run in an own probe folder (sources: 0, updated: 2026-11-12)
- [[0002-version-scheme]]: versions X.Y.Z-Build (internally X.Y.Z+Build), build number from Git; the code always carries the upcoming version (sources: 2, updated: 2026-11-08)
- [[0001-foundation]]: foundation: Tauri 2, portable-pty, xterm.js, output flow, process tree (sources: 1, updated: 2026-11-07)

## Components
- [[hangar-account-api]]: Hangar account API v1 and the app's client: device flow, rotating refresh tokens with single-flight refresh, problem documents, origin checks for opened pages, keychain storage; the self-hosted relay's differing account API (sources: 1, updated: 2026-11-09)
- [[single-instance]]: tauri-plugin-single-instance 2.5.5: one Hangar per user session in release builds, debug builds exempt; lock per platform (mutex, D-Bus name, socket), release in `cleanup_before_exit`, updater restart and Windows update path (sources: 1, updated: 2026-11-12)
- [[keyring]]: keyring 4.2 for secret profile variables: store crates per platform, error variants that carry raw bytes, build cost (sources: 1, updated: 2026-11-08)

## Platforms

## Agent CLIs
- [[catalog]]: built-in agent catalog: inclusion rules, name collision risks, bundled icons and their precedence, unverified points (sources: 2, updated: 2026-11-12)
- [[resume]]: resume_args per agent with evidence, behavior without a previous conversation, limits of --continue (sources: 2, updated: 2026-11-07)
- [[updates]]: latest and update_hint per agent with evidence, update_hint is executed, OpenCode packages, comparison rules (sources: 2, updated: 2026-11-08)

## Concepts
- [[team-crypto]]: team crypto (step E): device keys and safety numbers, signed membership log bound to team and position, epoch commitments, HPKE wrapping, history links, sealed and signed records, recovery code, threat model (sources: 0, updated: 2026-11-09)
- [[team-protocol]]: team protocol (step C): envelope and message table, keyed hashes of tasks and paths, overlap levels without content, reservation timing, rotation grace, event log and resync, threat model, limits (sources: 0, updated: 2026-11-09)

## Sources
- [[terminal-font-coverage-measurement]]: measurement: Fontsource subsets of Google-hosted mono fonts lack box drawing and block elements, the upstream Source Code Pro, IBM Plex Mono and Victor Mono files have them (sources: 0, updated: 2026-11-12)
- [[single-instance-lock-measurement]]: measurement: tauri-plugin-single-instance 2.5.5 lock tests on Windows 11: second start forwarded while held, lock held during `RunEvent::Exit`, released by `cleanup_before_exit` (updated: 2026-11-11)
- [[note-gitattributes-measurement]]: measurement: git 2.56 on Windows with `* -text -diff -merge`: note bytes stay unconverted under every `core.autocrlf`, conflicts leave `UU`/`UD`/`AA` with readable stages and no markers, rebase swaps stages 2 and 3 (updated: 2026-11-10)
- [[claude-http-hook-measurement]]: measurement: Claude Code 2.1.319 http hooks via `--settings`: request shape, deny reaches the model, timeout and connection failure fail open, SessionStart takes no http hooks, UserPromptSubmit context works (updated: 2026-11-09)
- [[relay-interop-measurement]]: measurement: the app against a self-hosted relay; live protocol compatible (hello, renewals, presence, claims, rotation to epoch 2, 4403, 4404), its account API differs in paths, bodies, client id, tickets and errors (sources: 0, updated: 2026-11-09)
- [[claude-settings-file-measurement]]: measurement: Claude Code 2.1.319 takes `--settings` as a file path (with spaces), sends `Host: 127.0.0.1:<port>`, and "ask" in print mode does not write the file (sources: 1, updated: 2026-11-09)
- [[claims-rescan-measurement]]: measurement: git status and diff -U0 through Git::run take about 150 ms on 50 000 files, 1.3 s with stale stat data; notify 8.2 on Windows unwatches silently after an overflow (updated: 2026-11-09)
- [[terminal-grapheme-widths]]: Unicode 11 counts emoji+VS16 as 1 cell, 15-graphemes as 2 like string-width; measured width table, no emoji fonts in the stack (sources: 0, updated: 2026-11-08)
- [[update-installers]]: research and measurement: detecting npm, winget, uv, brew and native installs from the resolved path, self-updaters, look-alike packages (updated: 2026-11-08)
- [[resume-id-measurement]]: measurement: `claude --resume <unknown uuid>` exits 1 after about 1 s and creates nothing; `--session-id` refuses an id whose transcript exists (updated: 2026-11-08)
- [[subagent-transcripts]]: research and measurement: where Claude Code, Codex, OpenCode and Gemini keep sub-agent transcripts, session id flags, hooks, tailing rules (updated: 2026-11-11)
- [[subagent-activity-measurement]]: measurement: Claude Code 2.1.322 sub-agent tokens (context size of the newest response), tool use count, `toolUseResult` shapes per tool; the live activity line is not stored (updated: 2026-11-11)
- [[context-window-measurement]]: measurement: Claude Code 2.1.322 main transcripts carry the context use but no window size; model id table, 1M evidence, Codex rollouts not linkable (updated: 2026-11-11)
- [[wsl-list-measurement]]: measurement: `wsl.exe --list --quiet` writes UTF-16LE into a pipe, Docker Desktop distribution, keychain round trip through keyring 4.2 (updated: 2026-11-08)
- [[webgl-measurement]]: measurement: xterm.js 6 DOM vs WebGL renderer in Edge 157 (WebView2 engine), no gain, 4x slower on color-heavy output; WebGL stays off (updated: 2026-11-08)
- [[windows-toast-notifications]]: app id rule of tauri-plugin-notification, missing toast tag in tauri-winrt-notification, implicit MTA (sources: 0, updated: 2026-11-08)
- [[code-signing-research]]: code signing: vendors and costs, Tauri signing options, --config, Get-AuthenticodeSignature (updated: 2026-11-08)
- [[cmd-launcher-measurement]]: measurement: .cmd launcher scripts via portable-pty, node from the working folder, injection via `&`, `"`, `%` (updated: 2026-11-08)
- [[update-sources]]: research and measurement: npm, PyPI and GitHub endpoints per agent, OpenCode packages (updated: 2026-11-07)
- [[github-conditional-requests]]: GitHub REST docs and measurement: ETag and If-None-Match, 304 counts unauthenticated, rate limit headers and waits (updated: 2026-11-12)
- [[agent-icons]]: Simple Icons 16.37.0 licenses per icon (CC0, Copilot MIT), brand guidelines, vendor site colors, which agent programs carry an exe icon on Windows (updated: 2026-11-12)
- [[app-execution-alias]]: Store app execution aliases: AppExecLink reparse data after libuv, what std and Get-AuthenticodeSignature do on the empty alias file, start in ConPTY (updated: 2026-11-12)
- [[resume-measurement]]: measurement: claude, codex, opencode, pi, aider with resume_args in an empty folder (updated: 2026-11-07)
- [[version-number-wikipedia]]: Wikipedia: major, minor, revision and build number (updated: 2026-11-07)
- [[agent-clis-research]]: research on 24 agent CLIs: program names, installation, renames (updated: 2026-11-07)
- [[tauri-updater-and-nsis]]: Tauri updater docs and NSIS template: updater signature, latest.json, desktop shortcut (updated: 2026-11-07)
- [[conpty-cursor-query-measurement]]: measurement: ConPTY waits for the reply to ESC[6n at startup (updated: 2026-11-07)

## Syntheses
- [[notes-and-start-usability]]: task-based usability test of the notes panel and of starting a session, findings, accepted redesign direction, and the start target, one-click updates, notes and right panel as built (sources: 0, updated: 2026-11-09)
- [[team-service-v1]]: research answers to the open questions of ADR 0005 (agent hooks, change observation, team crypto, server), the build plan in steps A to G and steps A (local claims), B (agent gate), C (protocol crate), E (team crypto), F part 1 (account sign-in and projects), F part 2a (team connection) and F part 2b (teammates' presence, shared claims, gate on a teammate's file, task reservations) as built (sources: 3, updated: 2026-11-09)
- [[agent-workflow-features]]: specification and contracts of the 0.3.0 feature batch (notifications, search, split view, launcher, git, worktrees, snippets, themes, profiles, secrets, tray) (sources: 1, updated: 2026-11-12)
- [[cmd-launcher]]: .cmd/.bat targets: NoDefaultCurrentDirectoryInExePath, reject special characters instead of escaping them, trade-offs and inheritance, also for version queries (sources: 1, updated: 2026-11-12)
- [[commit-history]]: contract of git_log and git_commit_diff: commits reachable from HEAD page by page with pushed state against the upstream, and one commit's changes in the shape of git_diff (sources: 0, updated: 2026-11-12)
- [[file-viewer]]: contract of open_file_viewer and file_viewer_content: read-only viewer window bound to one file of a session's folder, path rules, content rules (2 MiB, binary, BOM), capability; frontend window, viewer themes and the change path in the tree (sources: 0, updated: 2026-11-12)
- [[notes]]: contract of the note commands (folders, search, trash with restore, export, reveal), the detached side panel window (commands, messages, context, capability), shared project notes (status, setup, unlock, move, conflicts) and the notes UI as built (sources: 1, updated: 2026-11-12)
- [[program-updates]]: contract of check_program_updates, create_update_session and program-update-result, update plan per detected installer, execution rules, cache, 30 min schedule, conditional requests, rate limit pauses, Store apps (sources: 4, updated: 2026-11-12)
- [[session-restore]]: contract: workspace.json, scrollback per session, create_session with resume, resumeFallback event (sources: 1, updated: 2026-11-07)
- [[release-rollout]]: build, sign and publish a release as a GitHub release, create the upcoming version with `--next`; refresh the README screenshots (`pnpm readme:shots`, one image per scene); keys, code signing (vendors, variables, verification) (sources: 2, updated: 2026-11-11)
