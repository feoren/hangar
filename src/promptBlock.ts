// Claude Code's input block as one line of text, for click-to-move and deleting a selection
// (terminalClick.ts). Claude Code wraps a long prompt itself and writes every screen row with a
// real line break, so the terminal sees separate lines, not wrapped rows (measured, see
// docs/wiki/pages/sources/claude-prompt-wrap-measurement.md). What its line editor does at a
// row boundary, as measured with Claude Code 2.1.325 through ConPTY:
//
// - A boundary at a word wrap or a new line (Shift+Enter) is one character for Left, Right and
//   Backspace, and the cursor sits on that character at the end of the upper row (a blank cell
//   after the last character).
// - A word that fills the row up to the edge is followed by a blank that is drawn nowhere. The
//   cursor then shows the same cell (the start of the lower row) before and after that blank.
// - A word longer than a row is split inside the word: no character at the boundary. The screen
//   looks the same as a blank after a word that ends exactly at the edge.
// - A wide character that does not fit at the end of the row wraps and leaves the last column
//   unused; no character at the boundary although the row ends before the edge.
// - Blanks at the end of a row are not drawn. Two typed blanks at a wrap look like one.
//
// So a boundary counts as one character only when that is certain; the hidden extra blank of the
// last case is the one risk that remains (documented in the wiki). Whatever the screen cannot tell
// apart returns null, and the key then goes to the program unchanged.

import { inputStart, type ClickCell, type CursorColumns, type WrappedLine } from "./terminalClick";

/** The mark in front of Claude Code's input. */
const INPUT_MARK = "❯";
/** The horizontal rule above and below the input block. */
const RULE = "─";
/**
 * Columns Claude Code leaves empty at the right edge of its input block (measured at 50 and 80
 * columns: the text area ends two columns before the edge).
 */
const RIGHT_MARGIN = 2;
/** The character a boundary stands for in the joined line. */
const BOUNDARY: ClickCell = { width: 1, text: "", dim: false, inverse: false };

function blank(cell: ClickCell | undefined): boolean {
  return cell === undefined || cell.text.trim() === "";
}

/** The row starts with the input mark (the first row of the block). */
export function isInputStartRow(row: readonly ClickCell[]): boolean {
  return row[0]?.text === INPUT_MARK;
}

/** The row is a horizontal rule (at least three `─` at its start). */
export function isRuleRow(row: readonly ClickCell[]): boolean {
  return [0, 1, 2].every((col) => row[col]?.text === RULE);
}

/** Length of the row without its trailing blank cells. */
function trimmedLength(cells: readonly ClickCell[]): number {
  let length = cells.length;
  while (length > 0 && blank(cells[length - 1])) length -= 1;
  return length;
}

/** The cells of the word that ends at `end`, or null when it holds a wide character. */
function wordLength(cells: readonly ClickCell[], from: number, to: number): number | null {
  let count = 0;
  for (let col = from; col !== to; col += from < to ? 1 : -1) {
    const cell = cells[col];
    if (blank(cell)) break;
    // A wide character may be what pushed the word to the next row.
    if (cell?.width !== 1) return null;
    count += 1;
  }
  return count;
}

/**
 * The block as one line of text, or null when the rows are not certainly Claude Code's input.
 * `rows` run from the row with the input mark to the rule below the block (inclusive) and need at
 * least one continuation row. Every continuation row starts with exactly as many blanks as the
 * first row has before its text (`inputStart`) and has no prompt mark; the rows end at the rule.
 * `cols` is the terminal width, which gives the right edge of the text area.
 */
export function claudeInputBlock(
  rows: readonly (readonly ClickCell[])[],
  cols: number,
): WrappedLine | null {
  const first = rows[0];
  const rule = rows[rows.length - 1];
  if (first === undefined || rule === undefined || rows.length < 3) return null;
  if (!isInputStartRow(first) || !isRuleRow(rule)) return null;
  const indent = inputStart(first);
  const textEnd = cols - RIGHT_MARGIN;
  if (indent < 1 || textEnd <= indent + 1) return null;
  const text = rows.slice(0, -1);
  for (const [index, row] of text.entries()) {
    // Nothing is drawn in the margin; a character there is not Claude Code's input line.
    if (row.slice(textEnd).some((cell) => !blank(cell))) return null;
    if (index > 0 && trimmedLength(row) > 0 && inputStart(row) !== indent) return null;
    if (index > 0 && row.slice(0, indent).some((cell) => !blank(cell))) return null;
  }

  const cells: ClickCell[] = [];
  const rowStarts: number[] = [];
  const rowSkips: number[] = [];
  const cursorColumns: CursorColumns[] = [];
  // The cursor at the start of a row is ambiguous after a word that fills the previous row.
  let startDoubtful = false;
  for (const [index, row] of text.entries()) {
    const skip = index === 0 ? 0 : indent;
    const slice = row.slice(skip);
    const length = trimmedLength(slice);
    const min = skip + (startDoubtful ? 1 : 0);
    rowStarts.push(cells.length);
    rowSkips.push(skip);
    const next = text[index + 1];
    if (next === undefined) {
      cells.push(...slice);
      cursorColumns.push({ min, max: cols - 1 });
      break;
    }
    const lower = next.slice(indent);
    if (length > 0) {
      const upperWord = wordLength(slice, length - 1, -1);
      const lowerWord = wordLength(lower, 0, lower.length);
      if (upperWord === null || lowerWord === null) return null;
      if (skip + length >= textEnd) {
        // A row filled up to the edge: a blank after a word, or a word split inside. Only a
        // word that cannot be one word with the start of the next row is followed by a blank.
        if (upperWord + lowerWord > textEnd - indent) return null;
        cursorColumns.push({ min, max: skip + length - 1 });
        startDoubtful = true;
      } else {
        cursorColumns.push({ min, max: skip + length });
        startDoubtful = false;
      }
    } else {
      // An empty row (a new line typed on an empty line).
      cursorColumns.push({ min, max: skip });
      startDoubtful = false;
    }
    cells.push(...slice.slice(0, length), BOUNDARY);
  }
  return { cells, rowStarts, rowSkips, cursorColumns };
}
