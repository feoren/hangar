---
title: "Measurement: resume options of the agent CLIs in an empty folder"
type: source
tags: [agents, resume, measurement, windows, conpty]
sources: []
raw: raw/2026-11-07-resume-measurement.md
url:
retrieved: 2026-11-07
created: 2026-11-07
updated: 2026-11-07
---

# Measurement: resume options of the agent CLIs in an empty folder

Own measurement on Windows 11 (build 26300) with ConPTY through the real `SessionManager`: claude
2.1.319, codex 0.156.1, opencode 1.15.7, pi 0.79.9 and aider 0.86.2, each with the `resume_args` from the
catalog in a fresh empty folder, without input.

## Key points
- No measured agent exited within 8 to 20 s in an empty folder; none reported an error.
- claude and codex first show a trust prompt in an unknown folder and wait.
  codex prints "Resuming session…" before that.
- aider asks in a folder without a Git repository whether to create one, and waits.
- pi with `--continue` and no previous session simply starts a new one.
- opencode starts a full-screen TUI and queries several terminal capabilities; without a terminal
  emulator, the screen stays blank.
- The help texts confirm the options: `claude -c/--continue`, `codex resume --last`,
  `opencode -c/--continue`, `pi -c/--continue`, `aider --restore-chat-history`.

## Significance for Hangar
- The fallback to a normal launch (nonzero exit within 5 s) is not needed for these cases and
  was not triggered; it is a safeguard for CLIs that exit with an error when there is no previous
  session. Whether claude or codex exit like that in an already trusted folder without a previous
  session has not been measured.
- See [[resume]].
