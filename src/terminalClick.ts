// A plain click in an agent's prompt moves the cursor there, and a marked part of the prompt can
// be deleted with Backspace or Delete.
//
// A terminal has no caret the mouse can place: the agent's input line reads keys only. So, like
// iTerm2's Option+click, Hangar turns a click on the cursor's own line into Left or Right arrow
// presses, and a selection on that line into arrows to its end plus one Backspace per character.
// Only on that line, which takes in the rows the terminal wrapped it onto (`joinWrappedRows`):
// Up and Down would page through the agent's prompt history. Never for plain
// shells (they get xterm's own Alt+click), never while the program tracks the mouse itself, and
// a click never when the press became a selection.
//
// The editable part of the line is read from the screen: it starts after the prompt mark (`>`,
// `❯`, `›`) and ends where the typed text ends. A suggestion the agent shows faint after the
// cursor (Claude Code's predicted next prompt, a placeholder) is not input: Right arrow or Tab
// would take it, so nothing here ever moves into it.

import type { ShellKind } from "./pathQuote";

/** One cell of the cursor's line: wide characters take two cells, the second with width 0. */
export interface ClickCell {
  width: number;
  /** The cell's text; empty for an untouched cell and for the second half of a wide character. */
  text: string;
  /** Drawn faint (SGR 2): how agent CLIs show a suggestion or placeholder that is not input. */
  dim: boolean;
  /** Drawn in reverse video: how Ink based CLIs draw their own cursor. */
  inverse: boolean;
}

/** A selection on the cursor's line, cell columns, `end` exclusive (xterm's selection range). */
export interface ColumnRange {
  start: number;
  end: number;
}

/** What decides whether a click may move the cursor. */
export interface ClickContext {
  shell: ShellKind;
  /** xterm's `modes.mouseTrackingMode`; anything but "none" means the program wants the clicks. */
  mouseTracking: string;
  /** A selection exists after the press: the user marked text and did not mean to move. */
  selected: boolean;
  /** The press carried a modifier (Alt+click is xterm's own, Shift extends a selection). */
  modifier: boolean;
}

/** Hangar may edit the prompt with synthetic keys: an agent session that leaves the mouse alone. */
export function promptEditable(shell: ShellKind, mouseTracking: string): boolean {
  return shell === "agent" && mouseTracking === "none";
}

export function clickMovesCursor(context: ClickContext): boolean {
  return (
    promptEditable(context.shell, context.mouseTracking) && !context.selected && !context.modifier
  );
}

/** Characters agent CLIs put in front of their input line. */
const PROMPT_MARKS = new Set([">", "❯", "›", "»", "!"]);
/** Box sides some CLIs draw around their input. */
const BOX_SIDES = new Set(["│", "┃", "║"]);

function blank(cell: ClickCell | undefined): boolean {
  return cell === undefined || cell.text.trim() === "";
}

/**
 * The first column of the editable text: after a box side, the prompt mark and one space. A line
 * without a mark (a continuation line of a longer prompt) starts at its first character, so
 * leading blanks never count as input.
 */
export function inputStart(cells: readonly ClickCell[]): number {
  let col = 0;
  const skipBlanks = () => {
    while (col < cells.length && blank(cells[col])) col += 1;
  };
  skipBlanks();
  if (BOX_SIDES.has(cells[col]?.text ?? "")) {
    col += 1;
    skipBlanks();
  }
  if (PROMPT_MARKS.has(cells[col]?.text ?? "")) {
    col += 1;
    if (col < cells.length && blank(cells[col])) col += 1;
  }
  return col;
}

/**
 * Where a faint suggestion starts at or after the cursor, or null. Ink draws its cursor as a
 * reverse cell, so the suggestion's first character sits under the cursor in reverse video and
 * only the rest is faint: a reverse cursor cell right before the faint run belongs to it.
 */
export function suggestionStart(cells: readonly ClickCell[], cursorCol: number): number | null {
  for (let col = cursorCol; col < cells.length; col += 1) {
    const cell = cells[col];
    if (cell === undefined || blank(cell) || !cell.dim) continue;
    const cursor = cells[cursorCol];
    if (col === cursorCol + 1 && cursor !== undefined && cursor.inverse && !blank(cursor)) {
      return cursorCol;
    }
    return col;
  }
  return null;
}

/**
 * The column after the last typed character: the end of the line's text before any suggestion,
 * without a closing box side, and never left of the cursor (typed blanks before it count).
 */
export function inputEnd(cells: readonly ClickCell[], cursorCol: number): number {
  const limit = suggestionStart(cells, cursorCol) ?? cells.length;
  let last = -1;
  for (let col = cursorCol; col < limit; col += 1) {
    const cell = cells[col];
    // A box side is the frame's, not typed; one inside the text still ends before later text.
    if (cell === undefined || blank(cell) || BOX_SIDES.has(cell.text)) continue;
    last = col;
  }
  if (last < 0) return cursorCol;
  return Math.max(cursorCol, last + Math.max(cells[last]?.width ?? 1, 1));
}

/**
 * The arrow presses that move the cursor from `cursorCol` to the character under `clickCol`, both
 * cell columns on the same line. Counts characters, not cells, so a wide character is one press.
 * A click past the typed text (on a suggestion or blank space) stops at its end, a click on the
 * prompt mark at the first character. Empty when that is the cursor's column. `applicationCursor`
 * (DECCKM) selects the `ESC O` form.
 */
export function arrowsToColumn(
  cells: readonly ClickCell[],
  cursorCol: number,
  clickCol: number,
  applicationCursor: boolean,
): string {
  const low = Math.min(inputStart(cells), cursorCol);
  const high = inputEnd(cells, cursorCol);
  const target = Math.min(Math.max(startOfCharacter(cells, clickCol), low), high);
  return arrows(cells, cursorCol, target, applicationCursor);
}

/**
 * The keys that delete a selection from the prompt: arrows to its end, then one Backspace per
 * character (a wide character is one). Null when the selection reaches outside the typed text
 * (the prompt mark, a suggestion, blank space after the text); the key then goes on as usual.
 */
export function selectionDeleteKeys(
  cells: readonly ClickCell[],
  cursorCol: number,
  selection: ColumnRange,
  applicationCursor: boolean,
): string | null {
  const start = startOfCharacter(cells, selection.start);
  // A selection ending inside a wide character takes the whole character.
  const end = cells[selection.end]?.width === 0 ? selection.end + 1 : selection.end;
  if (end <= start || start < inputStart(cells) || end > inputEnd(cells, cursorCol)) return null;
  const count = characters(cells, start, end);
  if (count === 0) return null;
  return arrows(cells, cursorCol, end, applicationCursor) + "\x7f".repeat(count);
}

function arrows(
  cells: readonly ClickCell[],
  from: number,
  to: number,
  applicationCursor: boolean,
): string {
  if (from === to) return "";
  const presses = characters(cells, Math.min(from, to), Math.max(from, to));
  const prefix = applicationCursor ? "\x1bO" : "\x1b[";
  return (prefix + (to > from ? "C" : "D")).repeat(presses);
}

function characters(cells: readonly ClickCell[], from: number, to: number): number {
  let count = 0;
  for (let col = from; col < to; col += 1) {
    // A cell past the line's stored content counts as one blank character.
    if ((cells[col]?.width ?? 1) > 0) count += 1;
  }
  return count;
}

/** A click on the right half of a wide character means that character. */
function startOfCharacter(cells: readonly ClickCell[], col: number): number {
  return col > 0 && cells[col]?.width === 0 ? col - 1 : col;
}

/**
 * A point on one of a wrapped line's rows: `row` indexes the rows passed in (0 is the line's
 * first row), `col` is the cell column on that row.
 */
export interface RowPoint {
  row: number;
  col: number;
}

/** A selection over a wrapped line's rows, `end` exclusive like xterm's selection range. */
export interface RowRange {
  start: RowPoint;
  end: RowPoint;
}

/** One logical line built from screen rows: the cells in order and where each row begins. */
export interface WrappedLine {
  cells: readonly ClickCell[];
  /** The index into `cells` of each row's first cell. */
  rowStarts: readonly number[];
}

/**
 * Joins rows the terminal wrapped itself (xterm's `isWrapped`) into one line of cells. Such rows
 * hold one line the program wrote without a line break, so its line editor moves through them
 * with Left and Right as through one line. A wide character that did not fit at a row's end left
 * one never written cell there (xterm clears the rest of the row before it wraps); that cell is no
 * character and is dropped, so the arrow and Backspace counts stay exact.
 */
export function joinWrappedRows(rows: readonly (readonly ClickCell[])[]): WrappedLine {
  const cells: ClickCell[] = [];
  const rowStarts: number[] = [];
  rows.forEach((row, index) => {
    rowStarts.push(cells.length);
    const next = rows[index + 1];
    const last = row[row.length - 1];
    const padded =
      next !== undefined && last !== undefined && last.text === "" && next[0]?.width === 2;
    cells.push(...(padded ? row.slice(0, -1) : row));
  });
  return { cells, rowStarts };
}

/** The index into the joined cells of a point on a row; null for a row outside the line. */
export function lineOffset(line: WrappedLine, point: RowPoint): number | null {
  const start = line.rowStarts[point.row];
  if (start === undefined) return null;
  const end = line.rowStarts[point.row + 1] ?? line.cells.length;
  return start + Math.min(Math.max(point.col, 0), end - start);
}

/**
 * `arrowsToColumn` over a wrapped line: the click may be on any of its rows. Empty when the click
 * or the cursor lies on a row outside the line.
 */
export function arrowsToPoint(
  rows: readonly (readonly ClickCell[])[],
  cursor: RowPoint,
  click: RowPoint,
  applicationCursor: boolean,
): string {
  const line = joinWrappedRows(rows);
  const from = lineOffset(line, cursor);
  const to = lineOffset(line, click);
  if (from === null || to === null) return "";
  return arrowsToColumn(line.cells, from, to, applicationCursor);
}

/**
 * `selectionDeleteKeys` over a wrapped line: the selection may span its rows. Null when the
 * selection or the cursor reaches a row outside the line, so a selection over a real line break
 * (rows the terminal did not wrap itself) always goes to the usual key.
 */
export function rowSelectionDeleteKeys(
  rows: readonly (readonly ClickCell[])[],
  cursor: RowPoint,
  selection: RowRange,
  applicationCursor: boolean,
): string | null {
  const line = joinWrappedRows(rows);
  const from = lineOffset(line, cursor);
  const start = lineOffset(line, selection.start);
  const end = lineOffset(line, selection.end);
  if (from === null || start === null || end === null) return null;
  return selectionDeleteKeys(line.cells, from, { start, end }, applicationCursor);
}
