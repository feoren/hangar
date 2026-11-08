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
