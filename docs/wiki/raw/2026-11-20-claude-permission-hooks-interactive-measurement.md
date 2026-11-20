# Measurement: Claude Code's notice hooks in an interactive session (real ConPTY)

- Date: 2026-11-20
- Platform: Windows 11 Pro 10.0.26300, x64, ConPTY through `portable-pty` 0.9.0
- Program: Claude Code 2.1.329, `--model haiku --permission-mode default`
- Probe: a throwaway Rust program (`tests/.tmp/ptyprobe/`, not versioned) that starts `claude` in a
  120 x 40 PTY with `TERM=xterm-256color`, answers `ESC[6n` with `ESC[1;1R` and `ESC[c` with
  `ESC[?62;22c`, and sends keys at fixed times. No focus reports were sent.
- Listener: `tests/.tmp/ptyprobe/listener.py`, Python 3.14.4 on 127.0.0.1, answers every POST with
  `200` and an empty body, logs route, `hook_event_name`, `notification_type`, `tool_name` and
  whether the bearer value matched. Nothing else of a body is kept.
- Hooks (`--settings`, inline): Hangar's notice hooks (`gate::claude_notify_settings`) plus, for
  this measurement only, `Notification` matcher `idle_prompt` and a `PermissionDenied` hook, both
  to the clear route.
- Prompt (start argument): `Create a file named marker.txt in the current folder containing the
  single word marker. Use the Write tool. Do nothing else.`
- The working folder lies below a folder Claude Code already trusted; no trust dialog came.

## Run A: dialog open 12 s, then Esc, then 15 s

Probe steps (probe clock):

```
t=1.2   prompt submitted: true
t=4.0   permission request seen: true
t=16.0  dialog text on screen: true      ("Do you want to create marker.txt?" with 1. Yes, 2. Yes, and ..., 3. No, "Esc to cancel")
t=16.0  Esc sent (No)
t=31.0  waited 15 s after Esc
t=37.5  exited after /exit: true
```

Listener (its own clock):

```
t=8.12   /v1/notification/clear  UserPromptSubmit                       auth ok
t=10.92  /v1/notification/asks   PermissionRequest  tool Write          auth ok
t=16.92  /v1/notification/asks   Notification       permission_prompt   auth ok
t=38.50  /v1/notification/clear  SessionEnd                             auth ok
```

The screen then showed `Rejected write to marker.txt` and the prompt. The file was not written.

## Run B: dialog open 75 s, then Esc, then 75 s

```
probe:    t=1.0 prompt submitted, t=3.4 permission request seen, t=78.4 Esc sent, t=153.4 waited,
          t=159.9 exited after /exit
listener: t=3.25   clear  UserPromptSubmit
          t=5.66   asks   PermissionRequest  tool Write
          t=11.66  asks   Notification       permission_prompt
          t=156.22 clear  SessionEnd
```

## Observations

- `PermissionRequest` arrived as the dialog appeared; the dialog showed while and after the hook
  was answered (empty 200, no decision).
- `Notification` `permission_prompt` came 6.0 s after `PermissionRequest` in both runs, with no key
  typed and no focus report sent.
- No `idle_prompt` while the dialog stood open for 75 s.
- After "No" (Esc): no hook at all within 15 s (run A) and within 75 s (run B): no `Stop`, no
  `PostToolUseFailure`, no `PermissionDenied`, no `idle_prompt`. The next hook was `SessionEnd`
  after `/exit`.
