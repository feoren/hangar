---
title: "Research: per-session edit hooks in OpenCode 2.x, Cursor CLI, Cline CLI and Gemini CLI"
type: source
tags: [hooks, gate, team, opencode, cursor, cline, gemini, research]
sources: []
raw: raw/2026-11-22-agent-hooks-cursor-cline-gemini-opencode2-research.md
url: https://cursor.com/docs/agent/hooks
retrieved: 2026-11-22
created: 2026-11-22
updated: 2026-11-22
---

# Research: per-session edit hooks in OpenCode 2.x, Cursor CLI, Cline CLI and Gemini CLI

Code and docs research for [[agent-gate-adapters]]: which of four more agent CLIs can hold a
file edit for one session without touching the user's settings. Record:
`raw/2026-11-22-agent-hooks-cursor-cline-gemini-opencode2-research.md`. Nothing here was run live
unless [[opencode2-gate-live-measurement]] says so.

## Key points

| Agent | Per-session block | Main obstacle |
| --- | --- | --- |
| OpenCode 2.x | V2 plugin folder in `plugins` of `OPENCODE_CONFIG_CONTENT`, with `--standalone` | measured since: [[opencode2-gate-live-measurement]] |
| Cursor CLI 2026.11.19 | `--plugin-dir <dir>` with a `preToolUse` hook, deny JSON with exit 0 | local plugins load only when signed in; Windows hook shell bug under Git Bash (forum); not measured |
| Cline CLI 3.0.75 | none clean: `--hooks-dir` is a no-op; `--config <dir>` would hide the user's own agents, skills, rules and plugins | a cancel ends the whole run |
| Gemini CLI 0.70.0 | none clean: system settings need an administrator owner since 0.60.0; workspace settings write into the worktree and `~/.gemini`; `GEMINI_CLI_HOME` signs OAuth users out | Hangar's recipe never worked with an offered version |

> [!question] Unverified
> Cursor: whether `--plugin-dir` hooks ask for trust and which shell runs them when Hangar starts
> the agent; absolute or relative `file_path`. Cline: hook behaviour on Windows. All from code
> and docs, not run.

## Significance for Hangar

- OpenCode 2.x: built and measured ([[opencode2-gate-live-measurement]]).
- Cursor CLI: built and measured ([[cursor-gate-live-measurement]]).
- Cline CLI and Gemini CLI stay off team sessions: each per-session route changes the user's own
  setup (hidden agents and rules, files in the worktree or `~/.gemini`, a sign-out).
