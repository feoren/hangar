---
title: "Measurement: Cursor CLI 2026.11.19 live run with a gate plugin"
type: source
tags: [hooks, gate, cursor, plugin, windows, measurement, live]
sources: []
raw: raw/2026-11-22-cursor-gate-live-measurement.md
url:
retrieved: 2026-11-22
created: 2026-11-22
updated: 2026-11-22
---

# Measurement: Cursor CLI 2026.11.19 live run with a gate plugin

Six live turns on Windows 11 with a signed-in Cursor CLI 2026.11.19, headless and interactive in
a ConPTY, through `cursor-agent.cmd` as Hangar starts it, ending with Hangar's own helper against
a probe endpoint. Follows the route found in
[[agent-hooks-cursor-cline-gemini-opencode2-research]]. Record:
`raw/2026-11-22-cursor-gate-live-measurement.md`.

## Key points

- **`--plugin-dir <folder>` loads a local plugin** (`.cursor-plugin/plugin.json`,
  `hooks/hooks.json`) for one run; its `preToolUse` hook runs before each tool. Local plugins
  need a signed-in Cursor CLI.
- **Hook shell:** the command ran in its PowerShell form (`& '<exe>' ...`) when started through
  `cursor-agent.cmd`.
- **Input:** `tool_name` and `tool_input`; file changes come as `Write` (also edits, with the whole
  new content) and `Delete`, each with an absolute `file_path`; shell calls as `Shell` with
  `command` and `cwd`. No top-level `cwd`, but `workspace_roots`.
- **Matcher `Write|Delete`** keeps reads and shell calls away from the hook.
- **Deny:** `{"permission":"deny","user_message":..,"agent_message":..}` with exit 0 rejects the
  tool; the agent sees the reason and a note not to work around it.
- **Interactive:** only Cursor's usual workspace trust question comes first; no prompt for the
  plugin. `HANGAR_GATE` arrives in the hook.
- **End to end:** `hangar gate-hook cursor pre-tool-use` sent `Write` and `Delete` with the
  workspace root as `cwd` and the right token; the endpoint's deny kept both files unchanged.

## Significance for Hangar

- Cursor CLI is team-capable from 2026.11.19 (`hook_format = "cursor_plugin"`, measured).
- Signed out, Cursor ignores `--plugin-dir`, and the gate would silently not load
  [inference from the research, the signed-out case was not run].

> [!question] Unverified
> The hook shell when Git Bash is an ancestor (forum: a silent deny), the shell on macOS and
> Linux, Cursor's own "ask" answer, and whether a team admin's setting that turns local plugins
> off is visible to Hangar.
