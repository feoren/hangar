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
`cursorLineRows`).

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

- One logical line only (see above). Rows joined by a real line break are separate lines: a selection
  across them, or a click on another of them, goes to the program unchanged.
- Typed text that ends in a box side character, or starts with a prompt mark on a continuation line, is
  partly outside the editable part; a selection over it falls back to the usual key. Safe direction: the
  rule never sends more Backspaces than selected characters.
- Typed text drawn faint by the agent would count as a suggestion.

> [!question] Unverified
> Live behaviour in Claude Code 2.1.325 (faint cells, real cursor position on the prompt line) and in
> Codex, Gemini and Aider prompts has not been checked in the running window.

## Open: prompts the agent wraps or breaks itself

Measured for Claude Code 2.1.325 through ConPTY ([[claude-prompt-wrap-measurement]]): Claude Code wraps a
long prompt itself and ends every screen row with a real line break, so its rows carry no `isWrapped` and
the cross-row rules above do not apply to them. Click and selection delete stay on the cursor's row there.
Left at the first character of a continuation row does reach the row above: at a word wrap it lands on the
separating blank, which stays at the end of the upper row; at a break inside a long word there is no
blank. Continuation rows are indented by two blanks and the block ends at a `─` rule.

> [!question] Unverified
> Still needed before the rules can join such rows, per CLI and version: real new lines (Shift+Enter sends
> ESC CR, `AGENT_NEWLINE` in `src/terminalKeys.ts`), several blanks or a wide character at a wrap point, a
> word ending exactly at the text area's right edge, Right arrow and Backspace across a row boundary, and
> Gemini CLI. A wrong guess sends one Backspace too many, so until then such selections keep the usual key.
