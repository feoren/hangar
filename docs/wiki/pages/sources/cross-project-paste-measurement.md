---
title: "Measurement: pasted message with a header line, and loopback from agent sandboxes"
type: source
tags: [measurement, live, bracketed-paste, pty, conpty, windows, claude-code, codex, pi, aider, sandbox]
sources: []
raw: raw/2026-11-17-cross-project-paste-measurement.md
url:
retrieved: 2026-11-17
created: 2026-11-17
updated: 2026-11-17
---

# Measurement: pasted message with a header line, and loopback from agent sandboxes

Live runs on Windows 11 for [[0012-cross-project-messages]], ordered by the owner: Claude Code
2.1.326, Codex CLI 0.169.0, Pi 1.5.2 and Aider 0.86.2, each started interactively in a ConPTY
(120x40, `portable-pty` 0.9) in a throwaway git folder that was deleted afterwards. OpenCode (broken
npm install) and Gemini CLI (not installed) were not measured. Record:
`raw/2026-11-17-cross-project-paste-measurement.md`.

## Key points

- **One prompt per message.** A bracketed paste of `[Message from project "probe"]`, a `\r` and a
  second line, followed 300 ms later by `\r` as its own write, arrived as exactly one user message
  with both lines in all four transcripts. Claude Code and Codex answered it.
- **The line break inside the paste does not submit.** Without the separate Enter, all four only
  filled their input; no transcript entry was written.
- **Bracketed paste comes on early.** `ESC[?2004h` appeared within 0.5 to 1.6 s for all four.
- **But also during the trust dialog.** In a folder they had not seen, Claude Code and Codex turned
  bracketed paste on while their "trust this folder" dialog was open. Pasted text plus Enter
  answered that dialog (Claude Code took it as "No" and exited with code 1). "Bracketed paste on
  and output quiet" is therefore no proof that the agent's prompt is shown.
- **ConPTY waits for a cursor position answer.** The terminal must answer `ESC[6n`; without it
  Claude Code printed nothing for 60 s. xterm.js answers it.
- **Loopback from Codex's sandbox works.** `codex exec --sandbox workspace-write` reached a listener
  on 127.0.0.1 with `curl.exe`, without `network_access=true`. A bare `curl` failed, because Codex
  ran it through PowerShell, where `curl` is the alias of `Invoke-WebRequest`.
- **Claude Code asks first.** `claude -p` reached the listener under the owner's `auto` permission
  mode; with `--permission-mode default` the Bash call needed an approval and nothing was sent.

## Significance for Hangar

- Delivery as bracketed paste plus a separate Enter, as ADR 0012 decided, holds for the four
  measured CLIs.
- Hangar must not press Enter on its own for an agent's message: the trust dialog of a first start
  looks ready by the same signals. The person's "Paste" step of ADR 0012 covers this.
- `hangar-send` must be an executable with its own name (`hangar-send.exe` on Windows), never
  something a shell alias can shadow. Under Claude Code's default permissions each call needs an
  approval or an allow rule.
- Not measured: Codex's and Claude Code's sandboxes on Linux and macOS, Claude Code's optional OS
  sandbox, and whether Pi and Aider run commands without a sandbox of their own.
