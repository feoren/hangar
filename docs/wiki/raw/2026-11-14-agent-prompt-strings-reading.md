# Agent prompt strings: reading of installed binaries

Date: 2026-11-14. Platform: Windows 11 x64. Method: `grep -a` for short phrases in the installed
executables (counts and a few hundred bytes of context); nothing was run, no session started.
Only short phrases are recorded here, not code.

## Claude Code 2.1.325 (`claude --version`), native executable under the user's local bin folder

Phrase counts (`grep -ac`):

| Phrase | Count |
| --- | --- |
| `Do you want to proceed?` | 6 (plus "Do you want to allow this connection?", "Do you want to allow Claude to fetch this content?") |
| `Yes, and don't ask again` | 9 |
| `No, and tell Claude what to do differently` | 3 |
| `Esc to cancel` | 10 |
| `? for shortcuts` | 5 |
| `esc to interrupt` | 4 |
| `1. Yes` | 0 (option numbers are rendered separately from the labels) |

Notification channel (context of `preferredNotifChannel`):

- Allowed values: `auto`, `iterm2`, `terminal_bell`, `iterm2_with_bell`, `kitty`, `ghostty`,
  `notifications_disabled`. Default in the fresh global config: `preferredNotifChannel: "auto"`.
- Notification kinds include `permission_prompt`, `idle_prompt`, `elicitation_dialog`.
- `auto` resolves by terminal: `Apple_Terminal` gives `terminal_bell` (only under a further check),
  `iTerm.app` gives `iterm2`, `kitty` and `ghostty` their own; every other terminal gives
  `no_method_available`, which sends nothing. Only `terminal_bell` and `iterm2_with_bell` write a
  bell.
- Conclusion (reading, not a live measurement): in Hangar's terminal (none of the named ones) a
  permission prompt rings no bell by default; only `preferredNotifChannel: "terminal_bell"` in the
  user's Claude config makes it ring.

## Codex CLI 0.169.0 (npm package `@openai/codex`, `codex.exe` of the win32-x64 package)

| Phrase | Count |
| --- | --- |
| `Would you like to run the following command?` | 1 |
| `Would you like to make the following edits?` | 1 |
| `Would you like to grant these permissions?` | present (context) |
| `needs your approval` | present (context) |
| `Yes, and don't ask again` | 1 (several endings: "for these files", "for commands that start with") |
| `No, and tell Codex what to do differently` | 1 |
| `Ask Codex to do anything` | 1 (next to `Ask a follow-up question`, likely a placeholder list) |
| `context left` | 1 |
| `notification_method`, `notification_condition`, `osc9`, `approval-requested` | present |

No phrase showed whether a bell is written on an approval by default; the settings names point to
OSC 9 notifications. Not determined.

## Gemini CLI

Not installed on this machine; nothing read.
