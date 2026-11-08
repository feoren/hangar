# Index

Catalog of all wiki pages. Updated on every ingest.

## Overview
- [[overview]]: state of knowledge, open risks and questions

## Decisions
- [[0005-team-collaboration]]: proposed: several people and their agents on one project; requirements from the owner interview, end-to-end encrypted Hangar service, reservations, live file claims, merge queue (sources: 0, updated: 2026-11-08)
- [[0004-notes-per-working-folder]]: notes belong to the working folder, stored as .md in the app data folder, keyed by the normalized path; folder names `general` and `.trash` since 2026-11-08 (sources: 0, updated: 2026-11-08)
- [[0003-program-versions]]: program versions via a `--version` call with timeout and cache; program icons (sources: 0, updated: 2026-11-07)
- [[0002-version-scheme]]: versions X.Y.Z-Build (internally X.Y.Z+Build), build number from Git; the code always carries the upcoming version (sources: 2, updated: 2026-11-08)
- [[0001-foundation]]: foundation: Tauri 2, portable-pty, xterm.js, output flow, process tree (sources: 1, updated: 2026-11-07)

## Components
- [[keyring]]: keyring 4.2 for secret profile variables: store crates per platform, error variants that carry raw bytes, build cost (sources: 1, updated: 2026-11-08)

## Platforms

## Agent CLIs
- [[catalog]]: built-in agent catalog: inclusion rules, name collision risks, unverified points (sources: 1, updated: 2026-11-07)
- [[resume]]: resume_args per agent with evidence, behavior without a previous conversation, limits of --continue (sources: 2, updated: 2026-11-07)
- [[updates]]: latest and update_hint per agent with evidence, update_hint is executed, OpenCode packages, comparison rules (sources: 2, updated: 2026-11-08)

## Concepts

## Sources
- [[subagent-transcripts]]: research and measurement: where Claude Code, Codex, OpenCode and Gemini keep sub-agent transcripts, session id flags, hooks, tailing rules (updated: 2026-11-08)
- [[wsl-list-measurement]]: measurement: `wsl.exe --list --quiet` writes UTF-16LE into a pipe, Docker Desktop distribution, keychain round trip through keyring 4.2 (updated: 2026-11-08)
- [[webgl-measurement]]: measurement: xterm.js 6 DOM vs WebGL renderer in Edge 157 (WebView2 engine), no gain, 4x slower on color-heavy output; WebGL stays off (updated: 2026-11-08)
- [[windows-toast-notifications]]: app id rule of tauri-plugin-notification, missing toast tag in tauri-winrt-notification, implicit MTA (sources: 0, updated: 2026-11-08)
- [[code-signing-research]]: code signing: vendors and costs, Tauri signing options, --config, Get-AuthenticodeSignature (updated: 2026-11-08)
- [[cmd-launcher-measurement]]: measurement: .cmd launcher scripts via portable-pty, node from the working folder, injection via `&`, `"`, `%` (updated: 2026-11-08)
- [[update-sources]]: research and measurement: npm, PyPI and GitHub endpoints per agent, OpenCode packages (updated: 2026-11-07)
- [[resume-measurement]]: measurement: claude, codex, opencode, pi, aider with resume_args in an empty folder (updated: 2026-11-07)
- [[version-number-wikipedia]]: Wikipedia: major, minor, revision and build number (updated: 2026-11-07)
- [[agent-clis-research]]: research on 24 agent CLIs: program names, installation, renames (updated: 2026-11-07)
- [[tauri-updater-and-nsis]]: Tauri updater docs and NSIS template: updater signature, latest.json, desktop shortcut (updated: 2026-11-07)
- [[conpty-cursor-query-measurement]]: measurement: ConPTY waits for the reply to ESC[6n at startup (updated: 2026-11-07)

## Syntheses
- [[agent-workflow-features]]: specification and contracts of the 0.3.0 feature batch (notifications, search, split view, launcher, git, worktrees, snippets, themes, profiles, secrets, tray) (sources: 0, updated: 2026-11-08)
- [[cmd-launcher]]: .cmd/.bat targets: NoDefaultCurrentDirectoryInExePath, reject special characters instead of escaping them, trade-offs and inheritance (sources: 1, updated: 2026-11-08)
- [[notes]]: contract of the note commands (folders, tabs, trash) (sources: 0, updated: 2026-11-07)
- [[program-updates]]: contract of check_program_updates and create_update_session, canRun, execution rules, cache, network rules (sources: 1, updated: 2026-11-08)
- [[session-restore]]: contract: workspace.json, scrollback per session, create_session with resume, resumeFallback event (sources: 1, updated: 2026-11-07)
- [[release-rollout]]: build, sign and publish a release as a GitHub release, create the upcoming version with `--next`; keys, code signing (vendors, variables, verification) (sources: 2, updated: 2026-11-08)
