// The text a click on an OSC 8 link is judged by (termSafety.ts `osc8Decision`). A program can
// print part of an address as plain text and only the rest as the link (`https://github.com/`
// plain, `acme` linked), or let the line wrap in between. So the link's cells count together with
// the cells that touch them without a space, on the same row and across wrapped rows, in both
// directions. A hard line break counts as a wrap when it looks like one: the row above is full up
// to its last column and the row below starts with text. Cells nobody sees (drawn invisible, or
// in the color of their background) end a word like a space. Pure logic over a small view of
// xterm's buffer, so it can be tested without xterm.

import { paletteColor } from "./peekStyle";
import { schemeColors, type TerminalColors } from "./terminalSchemes";
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
  isDim?(): number;
  isInverse?(): number;
  getFgColor?(): number;
  getBgColor?(): number;
  isFgRGB?(): boolean;
  isBgRGB?(): boolean;
  isFgPalette?(): boolean;
  isBgPalette?(): boolean;
}

/** Colors this close (WCAG contrast ratio) read as one: text in them cannot be told apart. */
const SAME_COLOR = 1.1;
/**
 * And this close in every channel (0 to 255). Contrast alone measures lightness: green text on a
 * blue background of the same lightness reads well, and counting it hidden would blank it out.
 */
const SAME_CHANNEL = 24;
/** The scheme a cell is judged against when none is given (tests, the default look). */
const DEFAULT_COLORS = schemeColors("hangar", "dark");

function hex(rgb: number): string {
  return `#${(rgb & 0xffffff).toString(16).padStart(6, "0")}`;
}

/** A cell color as CSS: RGB as it is, a palette index or the default through the scheme. */
function cellColor(
  rgb: boolean | undefined,
  palette: boolean | undefined,
  value: number,
  fallback: string,
  colors: TerminalColors,
): string {
  if (rgb === true) return hex(value);
  if (palette === true) return paletteColor(value, colors);
  return fallback;
}

/** The red, green and blue channels of a `#rrggbb` color. */
function channels(color: string): number[] {
  return [1, 3, 5].map((at) => parseInt(color.slice(at, at + 2), 16) || 0);
}

/** `fg` half way towards `bg`, as xterm draws dim text (SGR 2) at half opacity. */
function halfWay(fg: string, bg: string): string {
  const back = channels(bg);
  const mixed = channels(fg).map((value, at) => Math.round((value + (back[at] ?? 0)) / 2));
  return `#${mixed.map((value) => value.toString(16).padStart(2, "0")).join("")}`;
}

/** Whether text in `fg` on `bg` cannot be told apart: close in lightness and in every channel. */
function indistinct(fg: string, bg: string): boolean {
  if (contrastRatio(fg, bg) >= SAME_COLOR) return false;
  const back = channels(bg);
  return channels(fg).every((value, at) => Math.abs(value - (back[at] ?? 0)) <= SAME_CHANNEL);
}

/**
 * Whether nobody sees a cell's text: drawn invisible (SGR 8), or in a color too close to its
 * background, in lightness and in every channel. Both colors resolve as the terminal draws them, through the active scheme
 * (`colors`): default colors, palette indices (bold base colors drawn bright), RGB, swapped by
 * inverse (SGR 7), dim text (SGR 2) half way towards the background.
 *
 * `minimumContrast` is the terminal's `minimumContrastRatio`: above 1, xterm lifts text in its
 * background color until it reads, so such text is seen and only SGR 8 hides a cell.
 */
export function cellHidden(
  cell: CellStyle,
  minimumContrast = 1,
  colors: TerminalColors = DEFAULT_COLORS,
): boolean {
  if ((cell.isInvisible?.() ?? 0) !== 0) return true;
  if (minimumContrast > 1) return false;
  let fgIndex = cell.getFgColor?.() ?? 0;
  const fgPalette = cell.isFgPalette?.();
  if (fgPalette === true && (cell.isBold?.() ?? 0) !== 0 && fgIndex < 8) fgIndex += 8;
  let fg = cellColor(cell.isFgRGB?.(), fgPalette, fgIndex, colors.foreground, colors);
  let bg = cellColor(
    cell.isBgRGB?.(),
    cell.isBgPalette?.(),
    cell.getBgColor?.() ?? 0,
    colors.background,
    colors,
  );
  if ((cell.isInverse?.() ?? 0) !== 0) [fg, bg] = [bg, fg];
  if ((cell.isDim?.() ?? 0) !== 0) fg = halfWay(fg, bg);
  return indistinct(fg, bg);
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

/** The texts a link is judged by (termSafety.ts `osc8DecisionOf`). */
export interface LinkTexts {
  /** The link's own cells, from its start to its end. */
  own: string;
  /** Those cells widened by the non-blank cells that touch them (`linkWord`). */
  word: string;
}

function cellText(cell: WordCell): string {
  return cell.width !== 0 && (cell.chars === "" || cell.hidden === true) ? " " : cell.chars;
}

/**
 * The visible text from `start` to `end` (both inclusive), widened by the non-blank cells that
 * touch it, through wrapped rows and breaks that look like a wrap. Hidden cells read as spaces.
 */
export function linkWord(source: WordSource, start: CellAt, end: CellAt): string {
  return linkTexts(source, start, end).word;
}

/**
 * The link's own text and the word around it (`linkWord`); both empty for a range outside the
 * buffer. Hidden cells read as spaces in both.
 */
export function linkTexts(source: WordSource, start: CellAt, end: CellAt): LinkTexts {
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
  if (from < 0 || to < from) return { own: "", word: "" };

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
  const text = (cut: WordCell[]) => cut.map(cellText).join("");
  return { own: text(cells.slice(from, to + 1)), word: text(cells.slice(left, right + 1)) };
}
