---
title: Claude Code prompt wrapping through ConPTY (measurement)
type: source
tags: [agents, claude-code, terminal, prompt, conpty, ink]
sources: []
raw: raw/2026-11-15-claude-prompt-wrap-measurement.md
url: ""
retrieved: 2026-11-15
created: 2026-11-15
updated: 2026-11-15
---

# Claude Code prompt wrapping through ConPTY (measurement)

Recorded output of Claude Code 2.1.325 on Windows 11 through a 50 column ConPTY while a long prompt was
typed and never sent.

## Key points

- Claude Code wraps a long prompt itself: every screen row of the input ends with a real line break
  (`\r\n` after ConPTY), so xterm never marks the rows `isWrapped`.
- It wraps at word boundaries; a word longer than a row is broken inside the word.
- Continuation rows start with two blanks, the width of `❯ ` on the first row; no prompt mark. The input
  block ends at a horizontal rule of `─`. At 50 columns the text area is columns 3 to 48.
- At a word wrap the separating blank stays on the upper row: Left at the first character of a
  continuation row puts the cursor on that blank, one column after the upper row's last character.
  Left therefore crosses the visual row boundary as through one text.
- At a break inside a long word no blank is involved.
- The placeholder in the empty prompt is drawn faint (SGR 2); Claude Code shows the real terminal cursor
  at the input position, not a reverse-video cell.

> [!question] Unverified
> Not measured: real new lines (Shift+Enter), several blanks or a wide character at a wrap point, a word
> that ends exactly at the text area's right edge, Right arrow across a row boundary, Backspace at a
> row's start, other widths, and other Ink based CLIs (Gemini CLI).

## Significance for Hangar

The cross-row prompt editing of [[prompt-mouse-editing]] joins only `isWrapped` rows, so it does not
apply to Claude Code's multi-row prompts; there click and selection delete stay on the cursor's row.
An extension would have to recognise the input block (first row with `❯`, continuation rows with the two
blank indent, rule below) and map row ends to text offsets as measured here.
