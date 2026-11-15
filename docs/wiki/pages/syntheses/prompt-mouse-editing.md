---
title: Prompt mouse editing in agent sessions
type: synthesis
tags: [terminal, agents, input, mouse, xterm]
sources: ["[[claude-code-suggestion-reading]]", "[[claude-prompt-wrap-measurement]]"]
created: 2026-11-15
updated: 2026-11-15
---

# Prompt mouse editing in agent sessions

An agent CLI's input line reads keys only, so Hangar turns mouse actions in that line into keys.
Code: `src/terminalClick.ts` (pure rules, tests in `src/terminalClick.test.ts`), `src/terminalKeys.ts`
(the `deleteSelection` key action), `src/terminal.ts` (`moveCursorToClick`, `selectionDeleteKeys`,
`editableLine`, `claudeBlockLine`), `src/promptBlock.ts` (Claude Code's input block).

## When

Only in agent sessions (`promptEditable`: shell kind `agent`, xterm `mouseTrackingMode` `none`).
Plain shells keep xterm's own behaviour (Alt+click). Only on the cursor's own line: Up and Down would
page through the agent's prompt history, so a selection that spans a real line break is left to the
usual keys.

The cursor's line takes in the rows the terminal wrapped it onto itself: xterm marks a row it continued
after reaching the right margin with `isWrapped` (`InputHandler.print`, read in
`node_modules/@xterm/xterm` 6.0.0). Such rows hold one line the program wrote without a line break, so
Left and Right move through them as through one line. `joinWrappedRows` joins them into one line of
cells (at most 64 rows, else nothing is edited); clicks and selections on any of those rows map onto it
(`lineOffset`, `arrowsToPoint`, `rowSelectionDeleteKeys`). A wide character that did not fit at a row's
end leaves one never written cell there (xterm clears the rest of the row before wrapping); that cell is
dropped, so the counts stay exact.

## The editable part of the line

Read from the cells of the cursor's line (width, text, faint, reverse):

- **Start** (`inputStart`): after a leading box side (`│`), the prompt mark (`>`, `❯`, `›`, `»`, `!`)
  and one space. A line without a mark (continuation line) starts at its first character, so leading
  blanks never count as input.
- **Suggestion** (`suggestionStart`): the first faint non-blank cell at or after the cursor. Claude Code
  shows its predicted next prompt and placeholders dim ([[claude-code-suggestion-reading]]). Ink draws its
  cursor as a reverse cell; a reverse cursor cell directly before the faint run is the suggestion's first
  character.
- **End** (`inputEnd`): after the last typed character before the suggestion, without a closing box
  side, never left of the cursor.

## Click

A plain click (no drag, no double click, no modifier, no link) on the cursor's line sends Left or Right
arrows, one per character (a wide character is one). The target is clamped to the editable part: a click
on the suggestion or on blank space after the text leaves the cursor at the end of the typed text, so no
Right arrow takes the suggestion. A click on the prompt mark goes to the first character.

## Delete a selection

Backspace or Delete without a modifier while text is selected (`deleteSelection`): when the selection
lies on the cursor's line and wholly inside the editable part, Hangar sends arrows to the selection's end
and one Backspace (`DEL`, 0x7f) per character, then clears the selection. A selection ending in the right
half of a wide character takes the whole character. Otherwise the key goes to the program unchanged
(xterm's Backspace or Delete). xterm's `getSelectionPosition()` is 0-based with an exclusive end column,
although its typings say 1-based (read in `node_modules/@xterm/xterm` 6.0.0,
`SelectionModel.finalSelectionStart/End`).

## Limits

- One logical line only (see above), or Claude Code's input block. Other rows joined by a real line
  break are separate lines: a selection across them, or a click on another of them, goes to the program
  unchanged.
- Typed text that ends in a box side character, or starts with a prompt mark on a continuation line, is
  partly outside the editable part; a selection over it falls back to the usual key. Safe direction: the
  rule never sends more Backspaces than selected characters.
- Typed text drawn faint by the agent would count as a suggestion.

> [!question] Unverified
> Live behaviour in Claude Code 2.1.325 (faint cells, real cursor position on the prompt line) and in
> Codex, Gemini and Aider prompts has not been checked in the running window.

## Claude Code's own wrapping and new lines

Claude Code 2.1.325 wraps a long prompt itself and writes every row with a real line break, so no row is
`isWrapped` ([[claude-prompt-wrap-measurement]]). `claudeInputBlock` in `src/promptBlock.ts` therefore
reads the input block (`claudeBlockLine` in `src/terminal.ts` collects its rows) and joins it into one line
of text, only when all of this holds:

- the first row starts with `❯` and the row above it is a rule (`─`); the last row of the block is a rule;
- every row in between starts with exactly as many blanks as `inputStart` gives for the first row
  (two), then text without a prompt mark or box side, or is empty; nothing is drawn in the two right
  margin columns (the text area ends two columns before the edge, measured at 50 and 80 columns);
- at most 64 rows.

A row boundary then stands for characters as follows (Left, Right and Backspace were measured):

| Boundary | Characters | Why |
| --- | --- | --- |
| Upper row ends before the edge (word wrap, or Shift+Enter) | 1 | the cursor sits on that cell after the last letter |
| Empty row (new line on an empty line) | 1 | same |
| Upper row filled to the edge, last word and next row's first word cannot be one word (together longer than the text width) | 1 | the blank after the word is drawn nowhere |
| Upper row filled to the edge, together longer than the text width | unknown, nothing is edited | a word longer than a row is split inside the word with 0 characters; the screen looks the same |
| A wide character in the last word above or the first word below | unknown, nothing is edited | a wide character that does not fit wraps and leaves a column unused, 0 characters |

A new line (Shift+Enter, `AGENT_NEWLINE`) looks like a word wrap and counts the same, so the two never
need telling apart. After a row filled to the edge the cursor at the start of the lower row names two text
positions (before and after the blank); a cursor there, or right of an upper row's boundary cell, makes
every rule return nothing. A click or selection on a row of the block works like on the isWrapped rows;
the indent counts as no text (a point in it is the row's first character).

> [!warning] Residual risk
> Blanks at the end of a row are not drawn, so two blanks typed at a wrap (or before a Shift+Enter) look
> like one (run B1). The boundary is counted as one character; the Backspace count never exceeds the
> selected characters, but the arrows may stop one character short, so the deletion can be shifted by one
> in that rare case. Typing a blank twice right at a wrap is the only known trigger.

> [!question] Unverified
> Live behaviour in the running Hangar window (the real xterm cells for these rows: whether ConPTY leaves
> the blanks inside the text untouched as measured with the emulator, and the rule under the block through
> Hangar's own terminal size); other widths below 50; a third blank at a wrap; Gemini CLI (not installed)
> and other Ink based prompts, which keep to the single row rules.
