---
title: Agent prompt shapes (questions vs. idle input)
type: agent
tags: [agents, attention, queue]
sources: ["[[agent-prompt-strings-reading]]"]
created: 2026-11-14
updated: 2026-11-20
---

# Agent prompt shapes (questions vs. idle input)

What a question or permission dialog of an agent CLI looks like on screen, and whether it rings a
bell. Used by `src/promptGuard.ts` so that a queued message never answers a dialog
([[queued-messages-and-stall-hint]]).

## Bell on a permission prompt

| CLI | Bell by default | Evidence |
| --- | --- | --- |
| Claude Code 2.1.325 | no in Hangar: `preferredNotifChannel` `auto` resolves to no channel outside iTerm2, Apple Terminal, kitty and ghostty; `terminal_bell` must be set by the user | [[agent-prompt-strings-reading]] (code reading, not live) |
| Codex CLI 0.169.0 | not determined; settings point to OSC 9 notifications | [[agent-prompt-strings-reading]] |
| Gemini CLI | not determined | none |

So "asks" from a bell cannot be relied on; the screen has to be read.

For Claude Code from 2.1.268 Hangar does not depend on the screen for permission and MCP input
dialogs: report-only hooks (`PermissionRequest`, `Notification`) set the attention reason
`permission` ([[0016-permission-notices]], [[claude-permission-hooks-measurement]]). The patterns
below stay the fallback for every other program and for a trust dialog.

## Patterns checked by the guard

`looksLikeQuestion` reads the newest 16 non-empty screen lines. A match keeps the message.

| Pattern | Seen in | Evidence level |
| --- | --- | --- |
| "Do you want to ..." | Claude Code dialogs | string in binary ([[agent-prompt-strings-reading]]) |
| "Would you like to ...", "needs your approval" | Codex approvals | string in binary |
| "Yes, and don't ask again", "No, and tell <agent> what to do differently" | Claude Code, Codex options | string in binary |
| "Esc to cancel" | Claude Code dialog footer | string in binary; whether it ever shows beside the idle prompt is not known |
| a numbered row with a pointer in front (`❯ 1.`, `› 1.`, `> 1.`, `● 1.`) plus one more numbered row | choice lists of all three | > [!question] Unverified: pointer glyphs per CLI and platform were not measured; option numbers are rendered apart from the labels in Claude Code |
| "Allow ...?", "Allow once/always/execution", "Apply this change" | Gemini CLI tool confirmation | > [!question] Unverified: Gemini CLI not installed, shape from memory of its UI |
| "(y/n)", "[y/N]", "[Y/n]" | classic terminal prompts (Aider and others) | general convention |

False matches (an agent's answer asking "Would you like to ...", a quoted prompt with a numbered
list) keep the message until the next finished task; that is the safe direction.

## Idle input prompt (positive check)

Not used yet. Candidate markers exist as strings ("? for shortcuts" in Claude Code, "Ask Codex to
do anything" and "context left" in Codex), but footers change with modes and typing, and Codex
appears to rotate placeholders. Without a live measurement of the idle screen per CLI, requiring
one of them would either be wrong or keep every message forever.

> [!question] Unverified
> Measurement needed: screen rows of Claude Code, Codex and Gemini at their idle prompt and in each
> dialog, in Hangar's terminal, to add a positive check per program.
