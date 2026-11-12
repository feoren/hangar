// Search across the output of every open session (Ctrl+Shift+S). Pure: the terminal hands in its
// buffer as rows, so matching, the order of results, the caps and the way back to a hit can be
// tested without xterm. Large scrollbacks are read in chunks with a pause in between, so typing
// and the terminals stay responsive while a search runs.

/** One buffer row as xterm keeps it: its text and whether it continues the row above (soft wrap). */
export interface BufferRowText {
  /** Untrimmed, so the text of a row that continues on the next one stays aligned. */
  text: string;
  wrapped: boolean;
}

/** Read access to a terminal buffer: rows 0 (oldest) to `length - 1` (newest). */
export interface RowSource {
  readonly length: number;
  row: (index: number) => BufferRowText | undefined;
}

/** Soft wrapped rows joined into the line the program wrote. */
export interface LogicalLine {
  /** First buffer row of the line. */
  row: number;
  text: string;
  /** Offset in `text` where each of its rows starts; the first is 0. */
  rowStarts: number[];
}

export interface LineMatch {
  sessionId: number;
  /** First buffer row of the line, where the search found it. */
  row: number;
  /** The whole line, to find it again after new output moved it. */
  text: string;
  /** First hit in the line (UTF-16 offset) and its length. */
  start: number;
  length: number;
}

export interface SearchTarget {
  sessionId: number;
  source: RowSource;
}

/** Hits shown at most; more would mean reading, not finding. */
export const SEARCH_LIMIT = 200;
/** Hits per session at most, so one chatty session cannot hide all others. */
export const SESSION_LIMIT = 50;
/** Rows read before the search pauses for the event loop. */
export const CHUNK_ROWS = 2000;

/**
 * Lowercase that keeps every UTF-16 offset in place (a few characters, such as the Turkish dotted
 * I, grow when lowercased), so a hit in the folded text is a hit at the same offset in the text.
 */
export function foldCase(text: string): string {
  const lower = text.toLowerCase();
  if (lower.length === text.length) return lower;
  let folded = "";
  for (const char of text) {
    const small = char.toLowerCase();
    folded += small.length === char.length ? small : char;
  }
  return folded;
}

/** Offset of the first hit of `needle` (already folded unless case sensitive), or -1. */
export function findInLine(text: string, needle: string, caseSensitive: boolean): number {
  if (needle === "") return -1;
  return (caseSensitive ? text : foldCase(text)).indexOf(needle);
}

/** The logical line whose last row is `end`; walks up while rows continue the one above. */
export function lineEndingAt(source: RowSource, end: number): LogicalLine | null {
  if (end < 0 || end >= source.length) return null;
  let start = end;
  while (start > 0 && source.row(start)?.wrapped === true) start -= 1;
  return joinRows(source, start, end);
}

/** The logical line that starts at `start`, or `null` if a row there continues another one. */
export function lineStartingAt(source: RowSource, start: number): LogicalLine | null {
  const first = source.row(start);
  if (first === undefined || (first.wrapped && start > 0)) return null;
  let end = start;
  while (end + 1 < source.length && source.row(end + 1)?.wrapped === true) end += 1;
  return joinRows(source, start, end);
}

function joinRows(source: RowSource, start: number, end: number): LogicalLine {
  let text = "";
  const rowStarts: number[] = [];
  for (let index = start; index <= end; index += 1) {
    rowStarts.push(text.length);
    text += source.row(index)?.text ?? "";
  }
  return { row: start, text: text.trimEnd(), rowStarts };
}

export interface SearchOptions {
  caseSensitive?: boolean;
  limit?: number;
  perSession?: number;
  chunkRows?: number;
}

export interface SearchHooks {
  /** Gives the event loop a turn; called between chunks. */
  pause: () => Promise<void>;
  /** A newer query replaced this one: stop at the next chunk. */
  cancelled: () => boolean;
  /** Hits so far, after every chunk that found some, so the list fills while the search runs. */
  onProgress?: (matches: readonly LineMatch[]) => void;
}

export interface SearchResult {
  matches: LineMatch[];
  /** Some hits were left out by a cap. */
  truncated: boolean;
  /** Stopped because a newer query replaced it. */
  cancelled: boolean;
}

/**
 * Finds the query in every target, sessions in the given order, each from its newest line up;
 * one hit per line. Stops at the caps and whenever `cancelled` says so.
 */
export async function searchTargets(
  targets: readonly SearchTarget[],
  query: string,
  options: SearchOptions,
  hooks: SearchHooks,
): Promise<SearchResult> {
  const caseSensitive = options.caseSensitive ?? false;
  const limit = options.limit ?? SEARCH_LIMIT;
  const perSession = options.perSession ?? SESSION_LIMIT;
  const chunkRows = Math.max(1, options.chunkRows ?? CHUNK_ROWS);
  const needle = caseSensitive ? query : foldCase(query);
  const matches: LineMatch[] = [];
  let truncated = false;
  if (needle.trim() === "") return { matches, truncated, cancelled: false };
  let budget = chunkRows;
  for (const target of targets) {
    let found = 0;
    let end = target.source.length - 1;
    while (end >= 0) {
      if (budget <= 0) {
        budget = chunkRows;
        hooks.onProgress?.(matches);
        await hooks.pause();
        if (hooks.cancelled()) return { matches, truncated, cancelled: true };
      }
      const line = lineEndingAt(target.source, end);
      if (line === null) break;
      budget -= end - line.row + 1;
      end = line.row - 1;
      const start = findInLine(line.text, needle, caseSensitive);
      if (start < 0) continue;
      if (matches.length >= limit || found >= perSession) {
        truncated = true;
        if (matches.length >= limit) return { matches, truncated, cancelled: false };
        break;
      }
      matches.push({
        sessionId: target.sessionId,
        row: line.row,
        text: line.text,
        start,
        length: query.length,
      });
      found += 1;
    }
  }
  return { matches, truncated, cancelled: false };
}

/** How far `relocate` looks around the old row; output may have pushed the line up meanwhile. */
export const RELOCATE_ROWS = 5000;

/**
 * The line of a hit in the buffer now. New output with a full scrollback drops the oldest rows,
 * so the line moves up; the nearest line with the same text wins, upwards first. `null` when the
 * line is gone.
 */
export function relocate(
  source: RowSource,
  row: number,
  text: string,
  reach: number = RELOCATE_ROWS,
): LogicalLine | null {
  for (let distance = 0; distance <= reach; distance += 1) {
    for (const candidate of distance === 0 ? [row] : [row - distance, row + distance]) {
      if (candidate < 0 || candidate >= source.length) continue;
      const line = lineStartingAt(source, candidate);
      if (line?.text === text) return line;
    }
  }
  return null;
}

/** One cell of a buffer row: its characters (`""` when empty) and its width in cells. */
export interface CellText {
  chars: string;
  /** 0 for the second half of a wide character, which holds no text of its own. */
  width: number;
}

/**
 * Column of a UTF-16 offset in one row's text: a wide character takes two cells but appears once
 * in the text, a character outside the BMP takes two UTF-16 units but one cell, an empty cell
 * reads as one space.
 */
export function columnOf(cells: readonly CellText[], offset: number): number {
  let consumed = 0;
  for (let column = 0; column < cells.length; column += 1) {
    const cell = cells[column];
    if (cell === undefined || cell.width === 0) continue;
    if (consumed >= offset) return column;
    consumed += cell.chars === "" ? 1 : cell.chars.length;
  }
  return cells.length;
}

/** Row and offset within that row for an offset in a logical line. */
export function rowOfOffset(line: LogicalLine, offset: number): { row: number; offset: number } {
  let index = 0;
  for (let next = 1; next < line.rowStarts.length; next += 1) {
    if ((line.rowStarts[next] ?? Number.POSITIVE_INFINITY) > offset) break;
    index = next;
  }
  return { row: line.row + index, offset: offset - (line.rowStarts[index] ?? 0) };
}

export interface Excerpt {
  before: string;
  match: string;
  after: string;
}

/**
 * A short piece of the line around the hit: some context before (cut with "…"), the hit, and the
 * rest up to `width` characters. Leading whitespace is dropped.
 */
export function excerpt(
  text: string,
  start: number,
  length: number,
  context = 32,
  width = 120,
): Excerpt {
  let from = Math.max(0, start - context);
  while (from < start && /\s/.test(text[from] ?? "")) from += 1;
  // Only a cut through text gets an ellipsis, not dropped indentation.
  const cut = text.slice(0, from).trim() !== "";
  const before = (cut ? "…" : "") + text.slice(from, start);
  const match = text.slice(start, start + length);
  const room = Math.max(0, width - before.length - match.length);
  const tail = text.slice(start + length);
  const after = tail.length > room ? `${tail.slice(0, room)}…` : tail;
  return { before, match, after };
}
