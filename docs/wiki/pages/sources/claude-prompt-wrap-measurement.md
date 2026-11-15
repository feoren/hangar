---
title: Claude Code prompt wrapping through ConPTY (measurement)
type: source
tags: [agents, claude-code, terminal, prompt, conpty, ink]
sources: []
raw: raw/2026-11-15-claude-prompt-wrap-measurement.md, raw/2026-11-15-claude-prompt-edit-measurement.md
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

## Editing across the rows (second measurement)

Raw: `raw/2026-11-15-claude-prompt-edit-measurement.md`. Same program and method, but the screen is
rebuilt with a VT emulator (`pyte`) so the real cursor position is read after every key. Enter was never
sent (Shift+Enter is ESC CR). No `claude` process was left over.

- **A word wrap is one character** for Left, Right and Backspace. The cursor sits on a cell right after
  the last character of the upper row; Right from there reaches the first character of the lower row
  (run A). Seven Backspaces from the start of the lower row's second word removed the blank, the five
  letters of the first word and the boundary, and the text re-wrapped.
- **Blanks at the end of a row are not drawn.** Two typed blanks at a wrap (run B1, B2) are both at the
  upper row's end; the screen is the same as for one blank. Left from the lower row's start took three
  presses to cross them (cursor on the second blank, on the first, on the last letter).
- **A row filled to the edge** (run C1: the last letter in the last text column): the next typed blank
  is drawn nowhere. The cursor shows the start of the lower row both after the last letter and after
  the blank (Left stays on the same cell once). Backspace x3 from there removed "xt" of `next` and the
  blank, not a visible letter.
- **A word longer than a row** is split inside the word (run C2): Left from the lower row's start moves
  onto the last letter of the upper row; no character at the boundary. The screen is the same as a
  blank after a word that ends exactly at the edge.
- **A wide character** that does not fit at the end of the row wraps and leaves the last text column
  unused (run D, 45 letters, a wide character, `bc`): one Left from the wide character reaches the last
  letter, so the boundary holds no character although the row ends one column early.
- **Shift+Enter (ESC CR)** gives a row that looks exactly like a word wrap: two blanks indent, no mark,
  rule below (run E). Left from its first character puts the cursor right after the upper row's last
  character; one Backspace joins the rows. So a new line is one character, as a word wrap.
- **Right margin:** the text area ends two columns before the right edge at 50 and at 80 columns (run F:
  76 letters on the first row at 80 columns). At 30 columns the layout was different and was not used.
- Gemini CLI is not installed here and was not measured.

> [!question] Unverified
> Other widths below 50, a third blank or more at a wrap, a blank before a Shift+Enter, several
> Shift+Enter in a row with text in between at the edge, other Claude Code versions, and other Ink based
> CLIs.

## Significance for Hangar

[[prompt-mouse-editing]] recognises Claude Code's input block (first row with `❯`, continuation rows with
the two blank indent, rules above and below) and maps the row boundaries to text offsets as measured
here; whatever the screen cannot tell apart is left to the usual keys.
