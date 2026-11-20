---
title: "Reference: Claude Code Notification and PermissionRequest hooks"
type: source
tags: [claude-code, hooks, notification, permission, http]
sources: []
raw: raw/2026-11-20-claude-notification-hooks-reference.md
url: https://code.claude.com/docs/en/hooks
retrieved: 2026-11-20
created: 2026-11-20
updated: 2026-11-20
---

# Reference: Claude Code `Notification` and `PermissionRequest` hooks

Verbatim excerpts of the hooks reference and four CHANGELOG lines, retrieved 2026-11-20
(`raw/2026-11-20-claude-notification-hooks-reference.md`). Read for ADR 0016
([[0016-permission-notices]]).

## Key points
- **Notification types** (the matcher matches the type): `permission_prompt`, `idle_prompt`,
  `auth_success`, `auth_storage_failure`, `elicitation_dialog`, `elicitation_url_dialog`,
  `elicitation_complete`, `elicitation_response`, `agent_needs_input`, `agent_completed`, three
  `quota_auto_resume_*` and `plugin_notification`. The input carries `message`, an optional `title`
  and `notification_type` besides the common fields.
- **Timing:** `permission_prompt`, `elicitation_dialog` and `elicitation_url_dialog` come after about
  six seconds without typing, and in terminal sessions only "when you appear to be away from the
  terminal"; `idle_prompt` about 60 seconds after Claude finished. The reference points to
  `PermissionRequest` "to run a hook immediately when Claude asks for permission".
- **PermissionRequest** runs "only when Claude Code is about to ask you for permission, or when it
  would otherwise auto-deny a call that can't prompt"; not for a sandboxed command's network
  request (that one is a `permission_prompt`). It matches on the tool name.
- **Hook types per event:** `PermissionRequest` takes `command`, `http`, `mcp_tool` and `prompt`;
  `Notification`, `ElicitationResult`, `SessionEnd` and `StopFailure` take `command`, `http` and
  `mcp_tool`; `PostToolUse`, `PostToolUseFailure`, `Stop`, `UserPromptSubmit` and `PermissionDenied`
  take all five. `SessionStart` takes no http hooks.
- **HTTP answers:** a 2xx with an empty body is "equivalent to exit code 0 with no output"; a
  non-2xx, a connection failure, a body that is no JSON object and a timeout are non-blocking
  errors unless the hook sets `onFailure: "block"`.
- **ElicitationResult** runs after the person answers an MCP elicitation.
- **CHANGELOG:** `PermissionRequest` added in 2.0.45; MCP elicitation support added (version not in
  the excerpt); "Fixed PermissionRequest hooks not firing in `--print` mode"; "Fixed Notification
  hooks not firing for permission prompts when running under Claude Desktop or VS Code".

## Significance for Hangar
- `PermissionRequest` is the immediate signal for a permission dialog; `permission_prompt` is late
  and may not come at all while the person sits at the terminal, so it is only a second source.
- An http hook that answers an empty 200 makes no decision, and any failure of such a hook is
  non-blocking: report-only hooks cannot hold up or decide anything.
- No `async` for http hooks (the reference keeps it to command hooks); a short `timeout` bounds the
  wait instead.

> [!question] Unverified
> The version that introduced each `Notification` type, and whether a dialog waits for the
> `PermissionRequest` hook's answer in interactive mode, are not in the excerpts.
