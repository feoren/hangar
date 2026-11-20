# Excerpts: Claude Code hooks reference (Notification, PermissionRequest, hook types per event)

Retrieved 2026-11-20 from https://code.claude.com/docs/en/hooks.md (Markdown form of the hooks reference) and
https://raw.githubusercontent.com/anthropics/claude-code/main/CHANGELOG.md. Verbatim excerpts, line ranges of the
retrieved copy noted; one dash in a CHANGELOG line replaced by a colon (project rule), nothing else changed.

## Matcher values (reference line 299)

| `Notification` | notification type | `permission_prompt`, `idle_prompt`, `auth_success`, `auth_storage_failure`, `elicitation_dialog`, `elicitation_url_dialog`, `elicitation_complete`, `elicitation_response`, `agent_needs_input`, `agent_completed`, `quota_auto_resume_fired`, `quota_auto_resume_stale`, `quota_auto_resume_disabled`, `plugin_notification` |

## HTTP hook responses (lines 1074-1084)

HTTP hooks use HTTP status codes and response bodies instead of exit codes and stdout. To block the action when the request fails or returns a non-2xx status, set [`onFailure: "block"`](#block-the-action-when-a-hook-fails). To block a tool call or deny a permission, return a 2xx response with a JSON body containing the appropriate [decision fields](#decision-control).

After a [non-blocking error](#exit-code-output), the action goes ahead unless the hook is one [whose failure blocks the action](#block-the-action-when-a-hook-fails). Except on events with their own failure contract in the [per-event table](#exit-code-2-behavior-per-event), such as `WorktreeCreate`, you get these results for each response or failure:

* **2xx with an empty body**: success, equivalent to exit code 0 with no output
* **2xx with a JSON object body**: parsed using the same [JSON output](#json-output) schema as command hooks. A body that fails schema validation is a non-blocking error
* **2xx with any other body, such as plain text**: non-blocking error, handled the same as a non-2xx status. Claude Code doesn't add the text to Claude's context
* **Non-2xx status**: non-blocking error
* **Connection failure**: non-blocking error
* **Timeout**: the hook is canceled, as described under [Timeouts](#timeouts)


## PermissionRequest (lines 2162-2181)

### PermissionRequest

Runs when Claude Code is about to ask you for permission to use a tool. In sessions that can't show a prompt, such as background subagents in [non-interactive mode](https://code.claude.com/docs/en/headless.md), Claude Code still runs these hooks, and if no hook returns a decision, it denies the tool call. For a call that reaches a `--permission-prompt-tool` or the Agent SDK's [`canUseTool` callback](https://code.claude.com/docs/en/agent-sdk/permissions), the hooks run alongside your host, and whichever decides first applies.
Use [PermissionRequest decision control](#permissionrequest-decision-control) to allow or deny on behalf of the user.

Use this event when you need a signal the moment Claude asks for permission to use a tool. Claude Code runs a [Notification](#notification) hook with the `permission_prompt` type only after the prompt has waited about six seconds.

Claude Code doesn't run PermissionRequest hooks for a sandboxed command's [network request](https://code.claude.com/docs/en/sandboxing.md#network-isolation). To get a signal for that prompt, use the `permission_prompt` notification type.

Matches on tool name, same values as PreToolUse.

#### PermissionRequest input

PermissionRequest hooks receive `tool_name` and `tool_input` fields like PreToolUse hooks, but without `tool_use_id`. For an MCP tool, they also receive the [`mcp_server`](#pretooluse-input) object. An optional `permission_suggestions` array contains the [permission updates](#permission-update-entries) Claude Code suggests for this request, such as adding an allow rule or changing the permission mode.

The `permission_suggestions` array isn't an exact list of the options you see, because each permission dialog builds its own options. Some dialogs, such as the one for file edits, don't read the array at all and derive their options from the request itself. A dialog that does read it can still withhold an option whose suggestion stays in the array, for example when [`allowManagedPermissionRulesOnly`](https://code.claude.com/docs/en/settings-reference#allowmanagedpermissionrulesonly) hides rule-saving options. It can also offer options that have no suggestion entry, such as [**Yes, and switch to auto mode**](https://code.claude.com/docs/en/permission-modes.md#switch-permission-modes), which changes the permission mode directly rather than through a permission update.

PreToolUse hooks run before every tool call, whether or not it needs permission. PermissionRequest hooks run only when Claude Code is about to ask you for permission, or when it would otherwise auto-deny a call that can't prompt. Neither event fires for [`EndConversation`](https://code.claude.com/docs/en/tools-reference#endconversation-tool-behavior).

```json theme={null}

## Notification (lines 2564-2660)

### Notification

Runs when Claude Code sends notifications. Matches on notification type. Omit the matcher to run hooks for all notification types.

You receive these hook events even with desktop notifications turned off: the `preferredNotifChannel` setting, including `notifications_disabled`, changes only how you're alerted, not whether your hook runs.

| Matcher | When it fires |
| :- | :- |
| `permission_prompt` | Claude needs you to approve a tool use or a sandboxed command's [network request](https://code.claude.com/docs/en/sandboxing.md#network-isolation), and the prompt has waited about six seconds |
| `idle_prompt` | Claude finished responding about 60 seconds ago and you haven't typed since |
| `auth_success` | Authentication completes |
| `auth_storage_failure` | Your sign-in completes, but Claude Code can't save the new credentials to its [credential storage](https://code.claude.com/docs/en/authentication.md#credential-management). Fires in place of `auth_success` |
| `elicitation_dialog` | An MCP server opens an elicitation form and you haven't typed for about six seconds |
| `elicitation_url_dialog` | An MCP server asks you to open a browser URL and you haven't typed for about six seconds |
| `elicitation_complete` | An MCP server reports that a [URL-mode elicitation](#elicitation-input) is complete |
| `elicitation_response` | An MCP elicitation response is sent back to the server |
| `agent_needs_input` | A background session starts waiting on your input while [agent view](https://code.claude.com/docs/en/agent-view.md) is open in a terminal. Also fires when a terminal session shows you an [agent team teammate's terminal setup question](https://code.claude.com/docs/en/agent-teams.md#choose-a-display-mode) or auto mode's notice about [classifier request charges](https://code.claude.com/docs/en/auto-mode-classifier-billing.md) and you haven't typed for about six seconds |
| `agent_completed` | A background session finishes or fails. Fires only while [agent view](https://code.claude.com/docs/en/agent-view.md) is open in a terminal |
| `quota_auto_resume_fired` | Claude Code continues your task after a claude.ai usage limit paused it: at the reset, or sooner when something you do in Claude Code during the wait, such as adding usage credits, upgrading your plan, or switching models, makes usage available again, with the [model-setting exception](https://code.claude.com/docs/en/interactive-mode.md#wait-for-a-usage-limit-to-reset) |
| `quota_auto_resume_stale` | A claude.ai usage limit reset while your computer slept for more than about 30 minutes. Claude Code waits for you to press `Enter` instead of continuing. After a shorter sleep it continues and fires `quota_auto_resume_fired` instead |
| `quota_auto_resume_disabled` | Claude Code ends its wait for a claude.ai usage limit without continuing your task: [`autoContinueAtUsageLimit`](https://code.claude.com/docs/en/settings-reference#autocontinueatusagelimit) turned off or the reset moved more than 24 hours away during a wait Claude Code started on its own, the continued task kept hitting the limit, or the continuation was blocked before it reached the model. Doesn't fire when you press `Esc` or `Ctrl+C`, or pick **Don't continue automatically** |
| `plugin_notification` | A [mod](https://code.claude.com/docs/en/plugins/mods/api.md#show-something-without-starting-a-turn) raises a notification with `$.ui.notify`. The hook's `title` is the mod's name unless the mod set one |

The `quota_auto_resume_fired`, `quota_auto_resume_stale`, and `quota_auto_resume_disabled` types require Claude Code v2.1.234 or later. The `auth_storage_failure` type requires v2.1.288 or later.

In terminal sessions, `permission_prompt` for a sandboxed command's network request requires Claude Code v2.1.246 or later.

`agent_needs_input` for a teammate's terminal setup question requires Claude Code v2.1.248 or later.

> [!NOTE]
>
> The `permission_prompt`, `idle_prompt`, `elicitation_dialog`, and `elicitation_url_dialog` types share their timing with desktop notifications, so in terminal sessions you only see them when you appear to be away from the terminal:
>
> * Expect `permission_prompt` once you haven't typed for about six seconds. The timer starts when the permission prompt appears, and each keystroke defers it. To run a hook immediately when Claude asks for permission to use a tool, use [PermissionRequest](#permissionrequest) instead.
> * Expect `idle_prompt` about 60 seconds after Claude finishes responding, and only if you haven't typed since and no background agent, such as a background [subagent](https://code.claude.com/docs/en/sub-agents.md), is still running. Claude Code doesn't send `idle_prompt` while it waits for a claude.ai usage limit to reset. When the wait ends on its own, one of the `quota_auto_resume_*` types fires instead.
> * Expect `elicitation_dialog` for an elicitation form, or `elicitation_url_dialog` for a browser URL request, once you haven't typed for about six seconds. Both share the same six-second gate as `permission_prompt`: the timer starts when the dialog appears, and each keystroke defers it.
>
> A permission request or elicitation that arrives while another dialog is on screen keeps the same six-second gate, timed from when the request arrives. Its notification can reach you while the request still waits behind the open dialog.

Claude Code times `permission_prompt` differently in sessions where it sends permission requests to the Agent SDK's [`canUseTool` callback](https://code.claude.com/docs/en/agent-sdk/user-input), which is how Claude Desktop and the VS Code extension host Claude Code:

* Expect `permission_prompt` about six seconds after Claude asks for permission. Claude Code doesn't defer it while you type.
* If you or a [PermissionRequest](#permissionrequest) hook answer sooner, Claude Code doesn't run `permission_prompt`.
* Set [`CLAUDE_CODE_DISABLE_PERMISSION_PROMPT_NOTIFY_HOOKS`](https://code.claude.com/docs/en/env-vars) to `1` to turn `permission_prompt` off in these sessions.

Before v2.1.233, `permission_prompt` didn't fire in these sessions.

Use separate matchers to run different handlers depending on the notification type. This configuration triggers a permission-specific alert script when Claude needs permission approval and a different notification when Claude has been idle:

```json theme={null}
{
  "hooks": {
    "Notification": [
      {
        "matcher": "permission_prompt",
        "hooks": [
          {
            "type": "command",
            "command": "/path/to/permission-alert.sh"
          }
        ]
      },
      {
        "matcher": "idle_prompt",
        "hooks": [
          {
            "type": "command",
            "command": "/path/to/idle-notification.sh"
          }
        ]
      }
    ]
  }
}
```

#### Notification input

In addition to the [common input fields](#common-input-fields), Notification hooks receive `message` with the notification text, an optional `title`, and `notification_type` indicating which type fired.

```json theme={null}
{
  "session_id": "abc123",
  "transcript_path": "/Users/.../.claude/projects/.../00893aaf-19fa-41d2-8238-13269b9b3ca0.jsonl",
  "cwd": "/Users/...",
  "hook_event_name": "Notification",
  "message": "Claude needs your permission",
  "notification_type": "permission_prompt"
}
```

Notification hooks can't block or modify notifications. Claude Code discards their `systemMessage` and `continue` fields but still emits [`terminalSequence`](#emit-terminal-notifications), which is what the desktop notification example relies on. Notification hooks are intended for side effects such as forwarding the notification to an external service.

### SubagentStart

Runs when Claude spawns a subagent with the Agent tool, when Claude [resumes a subagent](https://code.claude.com/docs/en/sub-agents.md#resume-subagents), and each time an in-process [agent team](https://code.claude.com/docs/en/agent-teams.md) teammate handles a new message. Supports matchers to filter by agent type name. For built-in agents, this is the agent name like `general-purpose`, `Explore`, or `Plan`. For [custom subagents](https://code.claude.com/docs/en/sub-agents.md), this is the `name` field from the agent's frontmatter, not the filename.


## ElicitationResult (lines 3910-3920)

### ElicitationResult

Runs after a user responds to an MCP elicitation. Hooks can observe, modify, or block the response before it is sent back to the MCP server.

When an [Elicitation](#elicitation) hook answers a request, Claude Code sends that answer to the server without running ElicitationResult hooks.

The matcher field matches against the MCP server name.

#### ElicitationResult input

In addition to the [common input fields](#common-input-fields), ElicitationResult hooks receive `mcp_server_name`, `action`, and optional `mode`, `elicitation_id`, and `content` fields.

## Hook types per event (lines 3988-4031)

## Prompt-based hooks

In addition to command, HTTP, and MCP tool hooks, Claude Code supports prompt-based hooks (`type: "prompt"`) that use an LLM to evaluate whether to allow or block an action, and agent hooks (`type: "agent"`) that spawn an agentic verifier with tool access. Not all events support every hook type.

Events that support all five hook types (`command`, `http`, `mcp_tool`, `prompt`, and `agent`):

* `PermissionDenied`
* `PostToolBatch`
* `PostToolUse`
* `PostToolUseFailure`
* `PreToolUse`
* `Stop`
* `SubagentStop`
* `TaskCompleted`
* `TaskCreated`
* `TeammateIdle`
* `UserPromptExpansion`
* `UserPromptSubmit`

`PermissionRequest` supports `command`, `http`, `mcp_tool`, and `prompt` hooks but not `agent` hooks. If you configure an agent hook on this event, Claude Code skips it and the permission flow proceeds unchanged. To allow or deny from a hook, return the [decision object](#permissionrequest-decision-control) from a command or HTTP hook.

Events that support `command`, `http`, and `mcp_tool` hooks but not `prompt` or `agent`:

* `ConfigChange`
* `CwdChanged`
* `DirectoryAdded`
* `Elicitation`
* `ElicitationResult`
* `FileChanged`
* `InstructionsLoaded`
* `MessageDisplay`
* `Notification`
* `PostCompact`
* `PostModelSwitch`
* `PreCompact`
* `PreModelSwitch`
* `SessionEnd`
* `StopFailure`
* `SubagentStart`
* `WorktreeCreate`
* `WorktreeRemove`

`Setup` supports only `command` hooks. `SessionStart` supports only `command` and `mcp_tool` hooks, and [MCP tool hook fields](#mcp-tool-hook-fields) describes when its `mcp_tool` hooks run.


## CHANGELOG lines

- (line 4984) Fixed PermissionRequest hooks not firing in `--print` mode
- (line 5993) Fixed Notification hooks not firing for permission prompts when running under Claude Desktop or VS Code
- (line 10563) Added `PermissionRequest` hook to automatically approve or deny tool permission requests with custom logic
- (line 9226) Added MCP elicitation support: MCP servers can now request structured input mid-task via an interactive dialog (form fields or browser URL)
