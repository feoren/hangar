# Claude Code hooks reference, excerpts (retrieved 2026-11-09)

URL: https://code.claude.com/docs/en/hooks
Retrieved 2026-11-09 through a fetch tool that answered questions on the page text; the lines
below are the verbatim quotes it returned. Claude Code version installed on that day: 2.1.319.

## HTTP hook fields
- `url`: "URL to send the POST request to"
- `headers`: "Additional HTTP headers as key-value pairs. Values support environment variable
  interpolation using `$VAR_NAME` or `${VAR_NAME}` syntax. Only variables listed in
  `allowedEnvVars` are resolved"
- `allowedEnvVars`: "List of environment variable names that may be interpolated into header
  values. References to unlisted variables are replaced with empty strings. Required for any env
  var interpolation to work"
- `onFailure`: "What happens to the action when the hook fails: `"continue"`, the default, or
  `"block"`." Requires Claude Code v2.1.295 or later.
- Timeout: "Defaults: 600 for `command`, `http`, and `mcp_tool`; 30 for `prompt`; 60 for `agent`.
  Claude Code lowers the `command`, `http`, and `mcp_tool` default to 30 on `UserPromptSubmit`,
  `PreModelSwitch`, and `PostModelSwitch`, and to 10 on `MessageDisplay`."

## HTTP request and response
- "Claude Code sends the hook's JSON input as the POST request body with `Content-Type:
  application/json`. The response body uses the same JSON output format as command hooks."
- "2xx with an empty body: success, equivalent to exit code 0 with no output"
- "2xx with a JSON object body: parsed using the same JSON output schema as command hooks. A body
  that fails schema validation is a non-blocking error"
- "Non-2xx status: non-blocking error"; "Connection failure: non-blocking error"; "Timeout: the
  hook is canceled, as described under Timeouts"
- On `PreToolUse`, "a timed-out `command`, `http`, or `mcp_tool` hook doesn't block the tool call,
  except for hooks whose failure blocks the action. ... To block the call when a `command` or
  `http` hook times out, set `onFailure: "block"`."

## Events
- SessionStart: "Only `type: "command"` and `type: "mcp_tool"` hooks are supported."
- PreToolUse decisions: "`"allow"` skips the permission prompt"; "`"deny"` prevents the tool
  call." with `permissionDecisionReason` "shown to Claude"; "`"ask"` prompts the user to
  confirm."; "`"defer"` ends the turn so the tool can be resumed later, and requires
  non-interactive mode with the `-p` flag."
- "When multiple PreToolUse hooks return different decisions, precedence is `deny` > `defer` >
  `ask` > `allow`." "Deny and ask rules are still evaluated regardless of what the hook returns."
- "A hook's `additionalContext`, `systemMessage`, and `initialUserMessage` strings, and its plain
  stdout, are capped at 10,000 characters."
- Common input: `agent_id` "Unique identifier for the subagent or in-process teammate the hook
  fires in."

## Settings that limit hooks
- "Hook entries merge across settings levels rather than replacing each other: user, project, and
  local settings add their own hooks without removing managed ones."
- `allowedHttpHookUrls`: "when defined at any settings level, Claude Code runs an HTTP hook handler
  only if its URL matches the merged allowlist."
- `httpHookAllowedEnvVars`: "when defined, Claude Code interpolates only the environment variables
  on that list into hook headers."
- `allowManagedHooksOnly`: "Your user, project, local, and plugin hooks are blocked."
- `disableAllHooks`: "To temporarily disable all hooks without removing them, set
  `"disableAllHooks": true` in your settings file."
