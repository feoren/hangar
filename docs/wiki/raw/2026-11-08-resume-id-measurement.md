# Measurement: `claude --resume <unknown uuid>` (2026-11-08)

- Platform: Windows 11 Pro 10.0.26300, x64; Claude Code 2.1.319 (`claude --version`).
- Folder: a fresh, empty, never trusted folder under `<repo>/tests/.tmp/resume-probe`, deleted afterwards.
- Environment: the markers of the calling agent session removed (`CLAUDECODE`, `CLAUDE_CODE_*`
  identifiers, as in `INHERITED_SESSION_MARKERS`), no other change. No input was sent apart from
  the reply `ESC[1;1R` to `ESC[6n`. The UUIDs were random v4 values that never named a conversation.

## Run 1: without a terminal (stdin from /dev/null, stdout and stderr to files)

Command: `timeout 25 claude --resume 94bb5aa6-ab43-4350-911d-34ee0d619f80 < /dev/null`

- Exit code 1 after 0.97 s.
- stdout: empty. stderr, first line: `No conversation found with session ID: 94bb5aa6-ab43-4350-911d-34ee0d619f80`

## Run 2: in a PTY (ConPTY through Hangar's `SessionManager`, 120x30)

Profile `claude` with args `["--resume", "2a354880-3238-4689-ba88-c90261cacd06"]`, one-off test
program (not kept in the repo).

- Exit code 1 after 1.03 s.
- Output (escape sequences shortened): `ESC[6n`, mode settings, window title `claude`, then in
  red `No conversation found with session ID: 2a354880-3238-4689-ba88-c90261cacd06`, then the
  modes are reset.
- No folder trust prompt was shown: the id check runs before it.

## Follow-up check

- No project folder for the probe folder appeared under `%USERPROFILE%\.claude\projects\`
  (directory listing by name only).
