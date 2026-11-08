---
title: "Measurement: claude --resume with an unknown conversation id"
type: source
tags: [agents, claude-code, resume, measurement, windows, conpty]
sources: []
raw: raw/2026-11-08-resume-id-measurement.md
url:
retrieved: 2026-11-08
created: 2026-11-08
updated: 2026-11-08
---

# Measurement: claude --resume with an unknown conversation id

Own measurement on Windows 11 (build 26300) with Claude Code 2.1.319, once without a terminal and
once in ConPTY through the real `SessionManager`, in a fresh, untrusted, empty folder.

## Key points
- `claude --resume <uuid>` for an id without a stored conversation exits with **code 1 after about
  1 s** and prints `No conversation found with session ID: <uuid>` (stderr without a terminal,
  red text in the PTY).
- The check runs **before** the folder trust prompt, so no input is needed.
- Nothing was created under `~/.claude/projects/` for the folder, so the id stays free for a fresh
  `--session-id <uuid>`.

## Significance for Hangar
- The resume fallback ([[session-restore]]) triggers reliably: exit within the 5 s window, nonzero
  code. Hangar then starts `claude --session-id <same uuid>` ([[resume]]).

## Follow-up: an id that already names a conversation
Read from the strings of the 2.1.319 executable (raw/2026-11-08-session-id-in-use-check.md, no
conversation started or read): `--session-id <uuid>` is refused with `Error: Session ID <uuid> is
already in use.` when `<uuid>.jsonl` exists (most likely in the project folder of the working
folder), unless combined with `--fork-session` and `--resume <same uuid>`.

Consequence for Hangar: when `--resume <id>` fails although the conversation exists (corrupt or
unreadable transcript), the fallback `--session-id <same id>` ends with that error and the
session shows its exit; a restart then starts fresh under a new id. When the conversation is
unknown (the measured and usual case: deleted or cleaned up after `cleanupPeriodDays`), the
fallback works.

> [!question] Unverified
> Behavior when the conversation exists but cannot be loaded (corrupt file): not measured, it needs
> a real conversation, that is a login and a model call. Which folder `gee()` returns.
