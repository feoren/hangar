# Measurement: which Claude Code hooks reach an http listener around a permission request

- Date: 2026-11-20
- Platform: Windows 11 Pro 10.0.26300, x64
- Program: Claude Code 2.1.329 (`claude --version`), model `haiku`
- Listener: Python 3.14.4 `ThreadingHTTPServer` on 127.0.0.1 (random port), answering every POST
  with `200` and an empty body. Logged per request: path, body size, top-level keys,
  `hook_event_name`, `notification_type`, `tool_name`, `permission_mode`, the first 7 characters
  of `Authorization`. No other body content was kept.
- Script: `tests/.tmp/hook-measure/measure.py` (not versioned), run as
  `py -3 -I measure.py C:/Users/<user>/.local/bin/claude.exe`.
- Working folder: a fresh, empty folder below `tests/.tmp/hook-measure/work/`, not a git repository.

## Hooks given through `--settings` (inline JSON)

One http hook per event, URL `http://127.0.0.1:<port>/ev/<Event>`, `timeout: 5`,
`headers: {"Authorization": "Bearer $HANGAR_NOTIFY_TOKEN"}`, `allowedEnvVars: ["HANGAR_NOTIFY_TOKEN"]`;
matcher `*` on `PreToolUse`, `PostToolUse`, `PostToolUseFailure` and `PermissionRequest`, none on the
others. Events: `PermissionRequest`, `Notification`, `PreToolUse`, `PostToolUse`,
`PostToolUseFailure`, `PermissionDenied`, `Stop`, `StopFailure`, `UserPromptSubmit`, `SessionEnd`.
`HANGAR_NOTIFY_TOKEN` was set to a marker value in the child's environment.

Prompt (all runs): `Create a file named marker.txt in the current folder containing the single word
marker. Use the Write tool. Do nothing else.`

## Run 1 and 2: without `--permission-mode`

The user settings set `permissions.defaultMode` to `auto`. Both runs (one plain, one with
`--allowedTools Write`) wrote the file without asking. Requests, in order (t in seconds since the
listener started):

```
t=1.12  /ev/UserPromptSubmit  635 B  keys: cwd hook_event_name permission_mode prompt prompt_id session_id transcript_path
t=3.28  /ev/PreToolUse        767 B  tool Write
t=3.34  /ev/PostToolUse      1044 B  tool Write  keys add: duration_ms tool_response tool_use_id
t=4.44  /ev/Stop              696 B  keys: background_tasks cwd effort hook_event_name last_assistant_message permission_mode prompt_id session_crons session_id stop_hook_active transcript_path
t=4.82  /ev/SessionEnd        485 B  keys: cwd hook_event_name prompt_id reason session_id transcript_path
exit 0, file written
```

No `PermissionRequest`: the tool did not need the person's permission.

## Run 3: `--permission-mode default`

```
t=3.87  /ev/UserPromptSubmit   646 B  mode default
t=6.31  /ev/PreToolUse         782 B  tool Write  mode default
t=6.35  /ev/PermissionRequest  833 B  tool Write  mode default  keys: cwd effort hook_event_name permission_mode permission_suggestions prompt_id session_id tool_input tool_name transcript_path
t=7.77  /ev/Stop               841 B  mode default
t=8.17  /ev/SessionEnd         493 B
exit 0, file not written; the model's answer said the write permission was not granted
```

## Run 4: the notice hooks exactly as Hangar writes them

`tests/.tmp/hook-measure/measure_notice.py`: the `--settings` JSON of `gate::claude_notify_settings`
(PermissionRequest and Notification to `/v1/notification/asks`, Notification with the answered
types, UserPromptSubmit, PostToolUse, PostToolUseFailure, Stop and SessionEnd to
`/v1/notification/clear`, matcher `permission_prompt|elicitation_dialog|elicitation_url_dialog` on the
first Notification group), `--permission-mode default`, same prompt. The listener checked the
bearer value.

```
t=1.13  /v1/notification/clear  UserPromptSubmit   auth ok
t=3.49  /v1/notification/asks   PermissionRequest  auth ok
t=4.69  /v1/notification/clear  Stop               auth ok
t=5.09  /v1/notification/clear  SessionEnd         auth ok
exit 0, file not written, stderr empty
```

Claude Code took the settings without a complaint (nothing on stderr) and every hook reached its
route.

## Observations

- Every http hook carried the `Authorization` header with the bearer value from the environment
  (`Bearer ` prefix seen on every request).
- `PermissionRequest` came 0.04 s after `PreToolUse` of the same tool and has no `tool_use_id`.
- The empty 200 answer to `PermissionRequest` made no decision; in print mode, which cannot prompt,
  the call was denied, as the reference says.
- After that denial neither `PostToolUseFailure` nor `PermissionDenied` came; `Stop` did.
- No `Notification` in any of the three print-mode runs.
- `SessionEnd` takes http hooks.

## Not measured

- Interactive mode: whether the permission dialog waits for the `PermissionRequest` hook's answer,
  the `Notification` types `permission_prompt` and `elicitation_dialog` (the reference says they come
  only after about six seconds without typing and only when the person seems away from the
  terminal), what comes after the person answers "No" in the dialog, and `ElicitationResult`.
  These need an interactive session in a terminal and were not run.
