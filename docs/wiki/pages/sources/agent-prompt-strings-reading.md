---
title: Agent prompt strings (binary reading)
type: source
tags: [agents, attention, queue]
sources: []
raw: raw/2026-11-14-agent-prompt-strings-reading.md
url: ""
retrieved: 2026-11-14
created: 2026-11-14
updated: 2026-11-14
---

# Agent prompt strings (binary reading)

Short phrases found in the installed Claude Code 2.1.325 and Codex CLI 0.169.0 executables, and
how Claude Code picks its notification channel. Nothing was run.

## Key points

- Claude Code permission dialogs use "Do you want to proceed?" (and "Do you want to allow ...?"),
  options "Yes, and don't ask again ..." and "No, and tell Claude what to do differently", footer
  "Esc to cancel".
- Codex CLI approvals use "Would you like to run the following command?", "Would you like to make
  the following edits?", "... needs your approval", the same "Yes, and don't ask again" and "No,
  and tell Codex what to do differently" options.
- Claude Code's `preferredNotifChannel` defaults to `auto`; outside iTerm2, Apple Terminal, kitty
  and ghostty that resolves to no notification at all, so a permission prompt rings no bell in
  Hangar unless the user set `terminal_bell` (reading of the bundled code, not measured live).
- Codex: whether an approval writes a bell by default was not determined.

## Significance for Hangar

The attention tracker sees a quiet permission dialog as "done". A queued message must not be
pasted there; [[agent-prompt-shapes]] lists the patterns `src/promptGuard.ts` checks.
