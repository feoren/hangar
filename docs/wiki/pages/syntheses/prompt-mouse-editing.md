---
title: Prompt mouse editing in agent sessions
type: synthesis
tags: [terminal, agents, input, mouse, xterm]
sources: ["[[claude-code-suggestion-reading]]"]
created: 2026-11-15
updated: 2026-11-15
---

# Prompt mouse editing in agent sessions

An agent CLI's input line reads keys only, so Hangar turns mouse actions in that line into keys.
Code: `src/terminalClick.ts` (pure rules, tests in `src/terminalClick.test.ts`), `src/terminalKeys.ts`
(the `deleteSelection` key action), `src/terminal.ts` (`moveCursorToClick`, `selectionDeleteKeys`,
`cursorLineCells`).

## When

Only in agent sessions (`promptEditable`: shell kind `agent`, xterm `mouseTrackingMode` `none`).
Plain shells keep xterm's own behaviour (Alt+click). Only on the cursor's own line: Up and Down would
page through the agent's prompt history, so a multi-line prompt is edited one line at a time and a
selection that spans lines is left to the usual keys.

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

- Single line only (see above).
- Typed text that ends in a box side character, or starts with a prompt mark on a continuation line, is
  partly outside the editable part; a selection over it falls back to the usual key. Safe direction: the
  rule never sends more Backspaces than selected characters.
- Typed text drawn faint by the agent would count as a suggestion.

> [!question] Unverified
> Live behaviour in Claude Code 2.1.325 (faint cells, real cursor position on the prompt line) and in
> Codex, Gemini and Aider prompts has not been checked in the running window.
