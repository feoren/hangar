---
title: "Measurement: Claude Code hooks around a permission request"
type: source
tags: [claude-code, hooks, permission, measurement, windows]
sources: []
raw: raw/2026-11-20-claude-permission-hooks-measurement.md, raw/2026-11-20-claude-permission-hooks-interactive-measurement.md
url: https://code.claude.com/docs/en/hooks
retrieved: 2026-11-20
created: 2026-11-20
updated: 2026-11-20
---

# Measurement: Claude Code hooks around a permission request

Four runs of `claude -p` and two interactive runs (Claude Code 2.1.329, Windows 11, `--model haiku`) with an http hook on
ten events, all sent to a local Python listener that answered an empty 200
(`raw/2026-11-20-claude-permission-hooks-measurement.md`).

## Key points
- With the user's default permission mode (`auto`) the `Write` ran without asking: `UserPromptSubmit`,
  `PreToolUse`, `PostToolUse`, `Stop`, `SessionEnd`. No `PermissionRequest`.
- With `--permission-mode default`: `UserPromptSubmit`, `PreToolUse`, then `PermissionRequest`
  0.04 s later (fields `cwd`, `effort`, `hook_event_name`, `permission_mode`,
  `permission_suggestions`, `prompt_id`, `session_id`, `tool_input`, `tool_name`,
  `transcript_path`; no `tool_use_id`), then `Stop` and `SessionEnd`. The empty answer made no
  decision and print mode denied the write.
- After that denial neither `PostToolUseFailure` nor `PermissionDenied` came.
- The bearer token from `allowedEnvVars` reached every hook, `SessionEnd` included.
- No `Notification` in print mode.
- A fourth run with the notice hooks exactly as Hangar writes them (two `Notification` groups with
  alternation matchers included) started without a complaint and sent `UserPromptSubmit`, `Stop`
  and `SessionEnd` to the clear route and `PermissionRequest` to the asks route, each with the
  token.

## Interactive (real ConPTY, two runs)
`raw/2026-11-20-claude-permission-hooks-interactive-measurement.md`: Claude Code 2.1.329 in a
`portable-pty` session, a `Write` that needs permission, the dialog left open (12 s, 75 s), then
Esc ("No").
- `PermissionRequest` arrives as the dialog appears; the empty answer makes no decision and the
  dialog stays.
- `Notification` `permission_prompt` follows 6.0 s later, with no key typed and no focus report.
- No `idle_prompt` while the dialog stands open, even for 75 s.
- After "No": no hook at all for 75 s (no `Stop`, `PostToolUseFailure`, `PermissionDenied` or
  `idle_prompt`); the next one was `SessionEnd` after `/exit`.

## Significance for Hangar
- `PermissionRequest` over http works as a signal and comes right after `PreToolUse`.
- `PostToolUse` ends the wait after an allowed tool; `Stop` after a denied one in print mode.
  The notice hooks of [[0016-permission-notices]] clear on these, on `UserPromptSubmit` and on
  `SessionEnd`, with a time limit as the fallback.

- After a "No" nothing reports the end of the dialog: Hangar clears the signal on the person's
  answering key ([[0016-permission-notices]]).

> [!question] Unverified
> `elicitation_dialog` (needs an MCP server that asks) and a trust dialog at the first start in an
> untrusted folder were not measured.
