# Index

Catalog of all wiki pages. Updated on every ingest.

## Overview
- [[overview]]: state of knowledge, open risks and questions

## Decisions
- [[0004-notes-per-working-folder]]: notes belong to the working folder, stored as .md in the app data folder, keyed by the normalized path; folder names `general` and `.trash` since 2026-11-08 (sources: 0, updated: 2026-11-08)
- [[0003-program-versions]]: program versions via a `--version` call with timeout and cache; program icons (sources: 0, updated: 2026-11-07)
- [[0002-version-scheme]]: versions X.Y.Z-Build (internally X.Y.Z+Build), build number from Git; the code always carries the upcoming version (sources: 2, updated: 2026-11-08)
- [[0001-foundation]]: foundation: Tauri 2, portable-pty, xterm.js, output flow, process tree (sources: 1, updated: 2026-11-07)

## Components

## Platforms

## Agent CLIs
- [[catalog]]: built-in agent catalog: inclusion rules, name collision risks, unverified points (sources: 1, updated: 2026-11-07)
- [[resume]]: resume_args per agent with evidence, behavior without a previous conversation, limits of --continue (sources: 2, updated: 2026-11-07)
- [[updates]]: latest and update_hint per agent with evidence, update_hint is executed, OpenCode packages, comparison rules (sources: 2, updated: 2026-11-08)

## Concepts

## Sources
- [[code-signing-research]]: code signing: vendors and costs, Tauri signing options, --config, Get-AuthenticodeSignature (updated: 2026-11-08)
- [[cmd-launcher-measurement]]: measurement: .cmd launcher scripts via portable-pty, node from the working folder, injection via `&`, `"`, `%` (updated: 2026-11-08)
- [[update-sources]]: research and measurement: npm, PyPI and GitHub endpoints per agent, OpenCode packages (updated: 2026-11-07)
- [[resume-measurement]]: measurement: claude, codex, opencode, pi, aider with resume_args in an empty folder (updated: 2026-11-07)
- [[version-number-wikipedia]]: Wikipedia: major, minor, revision and build number (updated: 2026-11-07)
- [[agent-clis-research]]: research on 24 agent CLIs: program names, installation, renames (updated: 2026-11-07)
- [[tauri-updater-and-nsis]]: Tauri updater docs and NSIS template: updater signature, latest.json, desktop shortcut (updated: 2026-11-07)
- [[conpty-cursor-query-measurement]]: measurement: ConPTY waits for the reply to ESC[6n at startup (updated: 2026-11-07)

## Syntheses
- [[cmd-launcher]]: .cmd/.bat targets: NoDefaultCurrentDirectoryInExePath, reject special characters instead of escaping them, trade-offs and inheritance (sources: 1, updated: 2026-11-08)
- [[notes]]: contract of the note commands (folders, tabs, trash) (sources: 0, updated: 2026-11-07)
- [[program-updates]]: contract of check_program_updates and create_update_session, canRun, execution rules, cache, network rules (sources: 1, updated: 2026-11-08)
- [[session-restore]]: contract: workspace.json, scrollback per session, create_session with resume, resumeFallback event (sources: 1, updated: 2026-11-07)
- [[release-rollout]]: build, sign and publish a release as a GitHub release, create the upcoming version with `--next`; keys, code signing (vendors, variables, verification) (sources: 2, updated: 2026-11-08)
