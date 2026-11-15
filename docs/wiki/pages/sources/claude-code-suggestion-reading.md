---
title: Claude Code prompt suggestion (binary reading)
type: source
tags: [agents, claude-code, terminal, prompt]
sources: []
raw: raw/2026-11-15-claude-code-suggestion-reading.md
url: ""
retrieved: 2026-11-15
created: 2026-11-15
updated: 2026-11-15
---

# Claude Code prompt suggestion (binary reading)

Strings read from the installed Claude Code 2.1.325 binary.

## Key points

- After a turn Claude Code predicts the next prompt (`promptSuggestion`, setting `promptSuggestionEnabled`).
- It is shown "dim in the empty composer for Tab to take". Dim is how Ink draws `dimColor`, SGR 2 faint.
- The user reports that a Right arrow also takes it.

> [!question] Unverified
> The exact cell attributes on screen (faint versus a grey colour, a reverse-video cursor on the first
> character) were not recorded live.

## Significance for Hangar

Click-to-move and selection delete in the prompt ([[prompt-mouse-editing]]) treat faint cells right of the
cursor as not typed and never send a Right arrow into them.
