// The text a click on an OSC 8 link is judged by (termSafety.ts `osc8Decision`). A program can
// print part of an address as plain text and only the rest as the link (`https://github.com/`
// plain, `acme` linked), or let the line wrap in between. So the link's cells count together with
// the cells that touch them without a space, on the same row and across wrapped rows, in both
// directions. Pure logic over a small view of xterm's buffer, so it can be tested without xterm.

/** One cell: its characters ("" for an empty cell) and width (2 for CJK, 0 behind a wide one). */
export interface WordCell {
  chars: string;
  width: number;
}

/** One buffer row; `isWrapped` means it continues the row above. */
export interface WordLine {
  isWrapped: boolean;
  length: number;
  cell: (x: number) => WordCell | undefined;
}

/** The buffer, by 0-based row. */
export type WordSource = (y: number) => WordLine | undefined;

/** A cell position, 0-based. */
export interface CellAt {
  x: number;
  y: number;
}

/** How far a word is followed in either direction; a longer one is no address anyone reads. */
export const WORD_LIMIT = 2048;
/** Rows followed upwards or downwards through wrapping. */
const ROW_LIMIT = 64;

/** An empty cell or white space ends a word; the right half of a wide character does not. */
function isGap(cell: WordCell): boolean {
  if (cell.width === 0) return false;
  return cell.chars === "" || /^\s+$/u.test(cell.chars);
}

/**
 * The visible text from `start` to `end` (both inclusive), widened by the non-blank cells that
 * touch it, through wrapped rows.
 */
export function linkWord(source: WordSource, start: CellAt, end: CellAt): string {
  let first = start.y;
  while (first > 0 && start.y - first < ROW_LIMIT && source(first)?.isWrapped === true) first -= 1;
  let last = end.y;
  while (last - end.y < ROW_LIMIT && source(last + 1)?.isWrapped === true) last += 1;

  const cells: WordCell[] = [];
  let from = -1;
  let to = -1;
  for (let y = first; y <= last; y += 1) {
    const line = source(y);
    if (line === undefined) continue;
    for (let x = 0; x < line.length; x += 1) {
      if (y === start.y && x === start.x) from = cells.length;
      if (y === end.y && x === end.x) to = cells.length;
      cells.push(line.cell(x) ?? { chars: "", width: 1 });
    }
  }
  if (from < 0 || to < from) return "";

  let left = from;
  while (left > 0 && from - left < WORD_LIMIT) {
    const before = cells[left - 1];
    if (before === undefined || isGap(before)) break;
    left -= 1;
  }
  let right = to;
  while (right < cells.length - 1 && right - to < WORD_LIMIT) {
    const after = cells[right + 1];
    if (after === undefined || isGap(after)) break;
    right += 1;
  }
  return cells
    .slice(left, right + 1)
    .map((cell) => (cell.chars === "" && cell.width !== 0 ? " " : cell.chars))
    .join("");
}
