// The text a click on an OSC 8 link is judged by (termSafety.ts `osc8Decision`). A program can
// print part of an address as plain text and only the rest as the link (`https://github.com/`
// plain, `acme` linked), or let the line wrap in between. So the link's cells count together with
// the cells that touch them without a space, on the same row and across wrapped rows, in both
// directions. A hard line break counts as a wrap when it looks like one: the row above is full up
// to its last column and the row below starts with text. Cells nobody sees (drawn invisible, or
// in the color of their background) end a word like a space. Pure logic over a small view of
// xterm's buffer, so it can be tested without xterm.

import { contrastRatio } from "./theme";

/**
 * One cell: its characters ("" for an empty cell) and width (2 for CJK, 0 behind a wide one).
 * `hidden` for a cell nobody sees (`cellHidden`).
 */
export interface WordCell {
  chars: string;
  width: number;
  hidden?: boolean;
}

/**
 * The style of a cell as xterm's IBufferCell reports it. All optional, so a cell without style
 * (a test's) counts as visible.
 */
export interface CellStyle {
  isInvisible?(): number;
  isBold?(): number;
  getFgColor?(): number;
  getBgColor?(): number;
  isFgRGB?(): boolean;
  isBgRGB?(): boolean;
  isFgPalette?(): boolean;
  isBgPalette?(): boolean;
}

/** Colors this close (WCAG contrast ratio) read as one: text in them cannot be told apart. */
const SAME_COLOR = 1.1;

function hex(rgb: number): string {
  return `#${rgb.toString(16).padStart(6, "0")}`;
}

/**
 * Whether nobody sees a cell's text: drawn invisible (SGR 8), or in its own background color.
 * Colors compare as far as they can be known without the color scheme: two palette colors by
 * index (bold base colors are drawn bright), two RGB colors by contrast. Default colors and mixed
 * kinds count as visible.
 *
 * `minimumContrast` is the terminal's `minimumContrastRatio`: above 1, xterm lifts text in its
 * background color until it reads, so such text is seen and only SGR 8 hides a cell.
 */
export function cellHidden(cell: CellStyle, minimumContrast = 1): boolean {
  if ((cell.isInvisible?.() ?? 0) !== 0) return true;
  if (minimumContrast > 1) return false;
  const fg = cell.getFgColor?.() ?? 0;
  const bg = cell.getBgColor?.() ?? 0;
  if (cell.isFgPalette?.() === true && cell.isBgPalette?.() === true) {
    const bold = (cell.isBold?.() ?? 0) !== 0;
    return (bold && fg < 8 ? fg + 8 : fg) === bg;
  }
  if (cell.isFgRGB?.() === true && cell.isBgRGB?.() === true) {
    return contrastRatio(hex(fg), hex(bg)) < SAME_COLOR;
  }
  return false;
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

/**
 * An empty cell, white space or a hidden cell ends a word; the right half of a wide character
 * does not.
 */
function isGap(cell: WordCell): boolean {
  if (cell.width === 0) return false;
  return cell.hidden === true || cell.chars === "" || /^\s+$/u.test(cell.chars);
}

/**
 * Whether row `y` continues the row above: wrapped, or (a hard break that looks like a wrap) the
 * row above is full up to its last column and row `y` starts with text.
 */
function continues(source: WordSource, y: number): boolean {
  const line = source(y);
  if (line === undefined) return false;
  if (line.isWrapped) return true;
  const above = source(y - 1);
  if (above === undefined || above.length === 0 || line.length === 0) return false;
  const last = above.cell(above.length - 1);
  const first = line.cell(0);
  return last !== undefined && first !== undefined && !isGap(last) && !isGap(first);
}

/**
 * The visible text from `start` to `end` (both inclusive), widened by the non-blank cells that
 * touch it, through wrapped rows and breaks that look like a wrap. Hidden cells read as spaces.
 */
export function linkWord(source: WordSource, start: CellAt, end: CellAt): string {
  let first = start.y;
  while (first > 0 && start.y - first < ROW_LIMIT && continues(source, first)) first -= 1;
  let last = end.y;
  while (last - end.y < ROW_LIMIT && continues(source, last + 1)) last += 1;

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
    .map((cell) =>
      cell.width !== 0 && (cell.chars === "" || cell.hidden === true) ? " " : cell.chars,
    )
    .join("");
}
