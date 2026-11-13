// Colored lines for the session peek: the last rows of a terminal buffer as runs of equally styled
// text, colors resolved to CSS through the session's color scheme. Pure: works on a small
// buffer interface (structurally a subset of xterm's IBuffer), so it is testable without xterm.

import type { TerminalColors } from "./terminalSchemes";
import { contrastRatio, type AnsiColor } from "./theme";

/** Columns a peek line reads at most; a peek is about 70 columns wide, so this leaves room. */
export const PEEK_COLUMNS = 160;

/** The parts of xterm's IBufferCell the peek reads. */
export interface CellLike {
  getWidth(): number;
  getChars(): string;
  getFgColor(): number;
  getBgColor(): number;
  isFgRGB(): boolean;
  isBgRGB(): boolean;
  isFgPalette(): boolean;
  isBgPalette(): boolean;
  isBold(): number;
  isDim(): number;
  isItalic(): number;
  isUnderline(): number;
  isInverse(): number;
  isInvisible(): number;
  isStrikethrough(): number;
}

/** The parts of xterm's IBufferLine the peek reads. */
export interface LineLike {
  readonly length: number;
  /** With `cell`, xterm fills that object instead of allocating a new one per call. */
  getCell(x: number, cell?: CellLike): CellLike | undefined;
  translateToString(trimRight?: boolean): string;
}

/** The parts of xterm's IBuffer the peek reads: rows 0 (oldest) to `length - 1` (newest). */
export interface BufferLike {
  readonly length: number;
  getLine(y: number): LineLike | undefined;
  /** A cell object to reuse while reading (xterm's IBuffer.getNullCell). */
  getNullCell?(): CellLike;
}

/** Text in one style. A `null` color means the peek's own (the scheme's default) color. */
export interface PeekRun {
  text: string;
  fg: string | null;
  bg: string | null;
  bold: boolean;
  dim: boolean;
  italic: boolean;
  underline: boolean;
  strike: boolean;
}

export type PeekLine = readonly PeekRun[];

/** What a peek shows: its lines and the scheme colors of the area behind them. */
export interface PeekContent {
  lines: readonly PeekLine[];
  background: string;
  foreground: string;
}

/** The 16 base colors in palette order (SGR 30 to 37, then 90 to 97). */
const ANSI_ORDER: readonly AnsiColor[] = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
  "brightBlack",
  "brightRed",
  "brightGreen",
  "brightYellow",
  "brightBlue",
  "brightMagenta",
  "brightCyan",
  "brightWhite",
];

/** Steps of the 6 x 6 x 6 color cube (palette 16 to 231), as xterm computes them. */
const CUBE_STEPS = [0, 0x5f, 0x87, 0xaf, 0xd7, 0xff] as const;

function hex(rgb: number): string {
  return `#${(rgb & 0xffffff).toString(16).padStart(6, "0")}`;
}

function channels(r: number, g: number, b: number): number {
  return (r << 16) | (g << 8) | b;
}

/**
 * CSS color of a 256-color palette index: 0 to 15 from the scheme, 16 to 231 the color cube,
 * 232 to 255 the gray ramp. Out of range falls back to the scheme's foreground.
 */
export function paletteColor(index: number, colors: TerminalColors): string {
  const base = ANSI_ORDER[index];
  if (base !== undefined) return colors[base];
  if (index >= 16 && index <= 231) {
    const cube = index - 16;
    const step = (n: number): number => CUBE_STEPS[n] ?? 0;
    return hex(
      channels(step(Math.floor(cube / 36)), step(Math.floor(cube / 6) % 6), step(cube % 6)),
    );
  }
  if (index >= 232 && index <= 255) {
    const level = 8 + (index - 232) * 10;
    return hex(channels(level, level, level));
  }
  return colors.foreground;
}

function rgbOf(color: string): [number, number, number] | null {
  const match = /^#([0-9a-f]{6})$/i.exec(color);
  if (match?.[1] === undefined) return null;
  const value = parseInt(match[1], 16);
  return [(value >> 16) & 0xff, (value >> 8) & 0xff, value & 0xff];
}

/** Steps a color 10 % towards black (`darker`) or white until it reaches `ratio` against `bg`. */
function shifted(fg: [number, number, number], bg: string, ratio: number, darker: boolean): string {
  let [r, g, b] = fg;
  const css = (): string => hex(channels(r, g, b));
  while (contrastRatio(css(), bg) < ratio && (darker ? r + g + b > 0 : r + g + b < 765)) {
    if (darker) {
      r -= Math.ceil(r * 0.1);
      g -= Math.ceil(g * 0.1);
      b -= Math.ceil(b * 0.1);
    } else {
      r = Math.min(255, r + Math.ceil((255 - r) * 0.1));
      g = Math.min(255, g + Math.ceil((255 - g) * 0.1));
      b = Math.min(255, b + Math.ceil((255 - b) * 0.1));
    }
  }
  return css();
}

/**
 * `fg` raised to at least `ratio` against `bg`, as xterm's minimumContrastRatio does (same steps,
 * first in the direction away from the background). Colors other than `#rrggbb` stay as they are.
 */
export function readableColor(fg: string, bg: string, ratio: number): string {
  if (ratio <= 1 || contrastRatio(fg, bg) >= ratio) return fg;
  const fgRgb = rgbOf(fg);
  const bgRgb = rgbOf(bg);
  if (fgRgb === null || bgRgb === null) return fg;
  const darkerFirst = contrastRatio(fg, "#000000") < contrastRatio(bg, "#000000");
  const first = shifted(fgRgb, bg, ratio, darkerFirst);
  if (contrastRatio(first, bg) >= ratio) return first;
  const second = shifted(fgRgb, bg, ratio, !darkerFirst);
  return contrastRatio(first, bg) > contrastRatio(second, bg) ? first : second;
}

/**
 * The style of one cell; `text` filled in by the caller. `minimumContrast` is the terminal's
 * minimumContrastRatio (halved for dim text, as in xterm).
 */
export function cellStyle(
  cell: CellLike,
  colors: TerminalColors,
  minimumContrast = 1,
): Omit<PeekRun, "text"> {
  const bold = cell.isBold() !== 0;
  let fg: string | null = null;
  if (cell.isFgRGB()) fg = hex(cell.getFgColor());
  else if (cell.isFgPalette()) {
    const index = cell.getFgColor();
    // xterm draws bold text of the 8 base colors in their bright variant (drawBoldTextInBrightColors).
    fg = paletteColor(bold && index < 8 ? index + 8 : index, colors);
  }
  let bg: string | null = null;
  if (cell.isBgRGB()) bg = hex(cell.getBgColor());
  else if (cell.isBgPalette()) bg = paletteColor(cell.getBgColor(), colors);

  const dim = cell.isDim() !== 0;
  if (cell.isInverse() !== 0) {
    const swappedFg = bg ?? colors.background;
    bg = fg ?? colors.foreground;
    fg = swappedFg;
  }
  // Dim text is drawn at half strength of its color, so the default needs a concrete value.
  if (dim && fg === null) fg = colors.foreground;
  if (cell.isInvisible() !== 0) fg = bg ?? colors.background;
  else if (fg !== null && minimumContrast > 1) {
    fg = readableColor(fg, bg ?? colors.background, minimumContrast / (dim ? 2 : 1));
  }
  return {
    fg,
    bg,
    bold,
    dim,
    italic: cell.isItalic() !== 0,
    underline: cell.isUnderline() !== 0,
    strike: cell.isStrikethrough() !== 0,
  };
}

function sameStyle(a: Omit<PeekRun, "text">, b: Omit<PeekRun, "text">): boolean {
  return (
    a.fg === b.fg &&
    a.bg === b.bg &&
    a.bold === b.bold &&
    a.dim === b.dim &&
    a.italic === b.italic &&
    a.underline === b.underline &&
    a.strike === b.strike
  );
}

/** Unstyled run: the fallback when a row's cells cannot be read. */
export function plainRun(text: string): PeekRun {
  return {
    text,
    fg: null,
    bg: null,
    bold: false,
    dim: false,
    italic: false,
    underline: false,
    strike: false,
  };
}

/** A run that shows nothing but blank space: trimmed at the end of a row like `trimEnd`. */
function blank(run: PeekRun): boolean {
  return run.bg === null && !run.underline && !run.strike && run.text.trim() === "";
}

/**
 * One buffer row as runs, at most `columns` cells: neighbors in the same style merge, the cell
 * after a wide character (width 0) is skipped, an empty cell counts as a space, and blank runs at
 * the end are trimmed (as the plain reader trims trailing spaces).
 */
export function lineRuns(
  line: LineLike,
  colors: TerminalColors,
  columns: number = PEEK_COLUMNS,
  minimumContrast = 1,
  reuse?: CellLike,
): PeekRun[] {
  const runs: PeekRun[] = [];
  const end = Math.min(line.length, columns);
  for (let x = 0; x < end; x += 1) {
    const cell = line.getCell(x, reuse);
    if (cell === undefined) return [plainRun(line.translateToString(true).slice(0, columns))];
    if (cell.getWidth() === 0) continue;
    const text = cell.getChars() || " ";
    const style = cellStyle(cell, colors, minimumContrast);
    const last = runs[runs.length - 1];
    if (last !== undefined && sameStyle(last, style)) last.text += text;
    else runs.push({ text, ...style });
  }
  while (runs.length > 0) {
    const last = runs[runs.length - 1];
    if (last === undefined) break;
    if (blank(last)) {
      runs.pop();
      continue;
    }
    if (last.bg === null && !last.underline && !last.strike) last.text = last.text.trimEnd();
    break;
  }
  return runs;
}

/**
 * The last `count` rows with content as colored runs: blank rows below the cursor are left out,
 * rows as they appear on screen (wrapped ones separately), the same rows the plain reader
 * (overview.ts tailLines) picks.
 */
export function styledTail(
  buffer: BufferLike,
  count: number,
  colors: TerminalColors,
  columns: number = PEEK_COLUMNS,
  minimumContrast = 1,
): PeekRun[][] {
  let last = buffer.length - 1;
  while (last >= 0 && (buffer.getLine(last)?.translateToString(true) ?? "").trim() === "") {
    last -= 1;
  }
  const reuse = buffer.getNullCell?.();
  const lines: PeekRun[][] = [];
  for (let y = Math.max(0, last - count + 1); y <= last; y += 1) {
    const line = buffer.getLine(y);
    lines.push(line === undefined ? [] : lineRuns(line, colors, columns, minimumContrast, reuse));
  }
  return lines;
}

/** Plain lines as unstyled runs, for a terminal whose cells cannot be read. */
export function plainLines(lines: readonly string[]): PeekRun[][] {
  return lines.map((text) => (text === "" ? [] : [plainRun(text)]));
}

function sameRun(a: PeekRun, b: PeekRun): boolean {
  return a.text === b.text && sameStyle(a, b);
}

export function sameLine(a: PeekLine, b: PeekLine): boolean {
  return (
    a.length === b.length &&
    a.every((run, index) => {
      const other = b[index];
      return other !== undefined && sameRun(run, other);
    })
  );
}

/** Whether two peeks show the same, so React need not render again. */
export function samePeekContent(a: PeekContent | null, b: PeekContent | null): boolean {
  if (a === null || b === null) return a === b;
  return (
    a.background === b.background &&
    a.foreground === b.foreground &&
    a.lines.length === b.lines.length &&
    a.lines.every((line, index) => {
      const other = b.lines[index];
      return other !== undefined && sameLine(line, other);
    })
  );
}

/** Inline CSS of a run, as React takes it. */
export interface RunStyle {
  color?: string;
  background?: string;
  fontWeight?: number;
  fontStyle?: "italic";
  textDecorationLine?: string;
}

/** Inline style of a run; `undefined` for text in the default look, which needs no span. */
export function runStyle(run: PeekRun): RunStyle | undefined {
  const style: RunStyle = {};
  // Dim text is drawn at half strength, as xterm does; cellStyle gives it a concrete color.
  if (run.fg !== null) {
    style.color = run.dim ? `color-mix(in srgb, ${run.fg} 50%, transparent)` : run.fg;
  }
  if (run.bg !== null) style.background = run.bg;
  if (run.bold) style.fontWeight = 700;
  if (run.italic) style.fontStyle = "italic";
  const decoration = [run.underline ? "underline" : "", run.strike ? "line-through" : ""]
    .filter((part) => part !== "")
    .join(" ");
  if (decoration !== "") style.textDecorationLine = decoration;
  return Object.keys(style).length === 0 ? undefined : style;
}
