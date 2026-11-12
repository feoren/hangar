// Overview of all sessions (Ctrl+Shift+M): tiles in a grid with the last lines of each terminal.
// Pure: the number of columns, the arrow key moves and the lines a tile shows are testable
// without React or xterm.

import type { RowSource } from "./sessionSearch";

/** Narrowest tile in CSS pixels; below this the lines get too short to read. */
export const TILE_MIN_WIDTH = 260;
/** Lines of output a tile shows. */
export const TILE_LINES = 12;
/** How often the tiles read their terminals again; a few times per second is live enough. */
export const TILE_REFRESH_MS = 400;

/**
 * Columns for `count` tiles in `width` pixels: close to a square grid (2 x 2 for four, 3 x 3 for
 * nine), but never more than fit at the minimum tile width and never fewer than one.
 */
export function gridColumns(count: number, width: number, minWidth = TILE_MIN_WIDTH): number {
  if (count <= 1) return 1;
  const fit = Math.max(1, Math.floor(width / minWidth));
  return Math.max(1, Math.min(fit, Math.ceil(Math.sqrt(count)), count));
}

export type GridKey = "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" | "Home" | "End";

export function isGridKey(key: string): key is GridKey {
  return (
    key === "ArrowLeft" ||
    key === "ArrowRight" ||
    key === "ArrowUp" ||
    key === "ArrowDown" ||
    key === "Home" ||
    key === "End"
  );
}

/**
 * The tile an arrow key leads to. Left and right run through the tiles in reading order and stop
 * at the ends; up and down keep the column and stop at the first and last row (in a last row with
 * fewer tiles, down goes to its last tile).
 */
export function gridMove(index: number, count: number, columns: number, key: GridKey): number {
  if (count <= 0) return -1;
  const current = Math.min(Math.max(index, 0), count - 1);
  const cols = Math.max(1, columns);
  switch (key) {
    case "ArrowLeft":
      return Math.max(0, current - 1);
    case "ArrowRight":
      return Math.min(count - 1, current + 1);
    case "ArrowUp":
      return current - cols >= 0 ? current - cols : current;
    case "ArrowDown": {
      if (current + cols < count) return current + cols;
      // Already in the last row: stay.
      const lastRowStart = Math.floor((count - 1) / cols) * cols;
      return current >= lastRowStart ? current : count - 1;
    }
    case "Home":
      return 0;
    case "End":
      return count - 1;
  }
}

/**
 * The last `count` rows with content: blank rows below the cursor (a fresh or cleared screen) are
 * left out, trailing spaces trimmed. Rows as they appear on screen, wrapped ones separately.
 */
export function tailLines(source: RowSource, count: number = TILE_LINES): string[] {
  let last = source.length - 1;
  while (last >= 0 && (source.row(last)?.text.trim() ?? "") === "") last -= 1;
  const lines: string[] = [];
  for (let index = Math.max(0, last - count + 1); index <= last; index += 1) {
    lines.push((source.row(index)?.text ?? "").trimEnd());
  }
  return lines;
}
