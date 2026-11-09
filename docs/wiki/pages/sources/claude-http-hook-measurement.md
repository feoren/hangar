---
title: "Measurement: Claude Code http hooks through --settings, plus the hooks reference"
type: source
tags: [claude-code, hooks, gate, measurement, windows]
sources: []
raw: raw/2026-11-09-claude-http-hook-measurement.md
url: https://code.claude.com/docs/en/hooks
retrieved: 2026-11-09
created: 2026-11-09
updated: 2026-11-09
---

# Measurement: Claude Code http hooks through `--settings`

Two runs of `claude -p` (Claude Code 2.1.319, Windows 11, `--model haiku`,
`--permission-mode acceptEdits`) in a throwaway git repository with a local Python listener, and
excerpts of the hooks reference (`raw/2026-11-09-claude-hooks-reference.md`).

## Key points
- **Request:** a `PreToolUse` http hook gets a POST with `Content-Type: application/json`, a plain
  `Content-Length` body (no chunked encoding, `Connection: keep-alive`, user agent axios 1.15.2)
  holding `session_id` (equal to `--session-id`), `transcript_path`, `cwd`, `prompt_id`,
  `permission_mode`, `effort`, `hook_event_name`, `tool_name`, `tool_input` and `tool_use_id`.
  `tool_input.file_path` is absolute, with backslashes on Windows; a `Write` carries the whole new
  `content`.
- **Token:** `"headers": {"Authorization": "Bearer $HANGAR_HOOK_TOKEN"}` with
  `"allowedEnvVars": ["HANGAR_HOOK_TOKEN"]` sent the value from the environment of `claude`.
- **Decisions:** an empty 200 body let the edit continue through the normal permission flow; a
  `permissionDecision: "deny"` blocked it and the model quoted the `permissionDecisionReason`
  verbatim and did not retry.
- **Fail open:** a hook that timed out (`timeout: 5`, listener sleeping 30 s) and a hook to a port
  without a listener both let the write through. The reference adds `onFailure: "block"` (2.1.295
  or later) to turn that around.
- **SessionStart takes no http hooks:** the listener never saw one; the reference says only
  `command` and `mcp_tool` hooks run there. A `UserPromptSubmit` http hook's `additionalContext`
  reached the model.
- **Merging:** a project `.claude/settings.json` command hook still ran next to the `--settings`
  hooks. The user settings file had no hooks of its own, so merging with user hooks rests on the
  reference ("Hook entries merge across settings levels").
- **Limits from settings:** `allowedHttpHookUrls`, `httpHookAllowedEnvVars`,
  `allowManagedHooksOnly` and `disableAllHooks` can switch such hooks off or blank the token.

## Significance for Hangar
- The agent gate ([[team-service-v1]], step B) gives each Claude Code session its hooks through
  `--settings <inline JSON>` and its token through the PTY child's environment; nothing is
  written to the user's settings files.
- Hangar answers before Claude Code's own hook timeout (10 against 11 minutes), because a timed
  out hook lets the edit through.
- Facts for the agent go in through `UserPromptSubmit`, not `SessionStart`.
- The gate fails open by design (Hangar gone, endpoint refused, managed settings forbid the hook);
  the step A watcher stays the second line.

> [!question] Unverified
> Interactive mode while a hook holds the request (Esc during the wait), sub-agent tool calls
> (the reference says hooks there carry `agent_id`), a user settings file with hooks of its own,
> and `--settings` with a file path instead of inline JSON were not measured.
