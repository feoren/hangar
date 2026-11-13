// Web addresses and file paths in a terminal line, made clickable (terminal.ts adds them to
// xterm). Only text work lives here: finding candidates, splitting a path into its segments,
// resolving it against the session's folder and choosing what a click does. Whether a path exists
// is asked of the backend (pathCheck.ts); only existing paths become links.

/** What a link leads to. `line` and `column` come from a `:12:5` suffix and are 1 based. */
export type LinkTarget =
  | { kind: "url"; url: string }
  | { kind: "path"; path: string; line: number | null; column: number | null };

/** A candidate in a line: UTF-16 offsets, `end` exclusive, quotes left out. */
export interface TextLink {
  start: number;
  end: number;
  /** End of the path itself, before a `:line:col` suffix; `end` for addresses. */
  pathEnd: number;
  target: LinkTarget;
}

/** Longer quoted text is prose, not a path. */
const MAX_QUOTED = 512;
const MAX_TOKEN = 1024;
/** Lines can be long (minified output); candidates past this are not looked for. */
const MAX_LINE = 4096;

const URL_PATTERN = /\bhttps?:\/\/[^\s<>"'`\\^{}|\p{Cc}]+/giu;
const QUOTES = new Set(["`", '"', "'"]);
/** A quote opens a quoted path only after these (or at the line start), so `it's` opens none. */
const BEFORE_QUOTE = /[\s([{=:,]/u;
/** And closes it only before these (or at the line end). */
const AFTER_QUOTE = /[\s)\]}.,;:!?]/u;
const LEADING_JUNK = /^[([{<'"`]+/u;
const TRAILING_JUNK = /[.,;:!?'"`>]+$/u;
/** Characters a path never holds; `?` and `*` are wildcards in what a program prints. */
const NOT_IN_PATH = /[<>|?*"\p{Cc}]/u;
const LINE_SUFFIX = /:(\d{1,7})(?::(\d{1,5}))?$/u;
const WINDOWS_ABSOLUTE = /^[A-Za-z]:[\\/]/u;
/** A bare file name like `README.md` or `.gitignore`: a name and an extension with a letter. */
const BARE_FILE =
  /^(?:[\p{L}\p{N}_\-@+~]+[\p{L}\p{N}_\-@+~.]*\.[A-Za-z][A-Za-z0-9_]{0,11}|\.[A-Za-z][\w.-]*)$/u;
const SEPARATOR = /[\\/]/u;

/**
 * Every candidate in `text`, possibly overlapping (a quoted span and the words inside it):
 * `pickLinks` chooses among the existing ones.
 */
export function findLinks(text: string): TextLink[] {
  const line = text.length > MAX_LINE ? text.slice(0, MAX_LINE) : text;
  const found: TextLink[] = [];
  const urls = findUrls(line);
  found.push(...urls);
  const insideUrl = (start: number, end: number) =>
    urls.some((url) => start < url.end && url.start < end);
  const add = (link: TextLink | null) => {
    if (link === null || insideUrl(link.start, link.end)) return;
    if (found.some((other) => other.start === link.start && other.end === link.end)) return;
    found.push(link);
  };
  for (const span of quotedSpans(line)) add(pathAt(line, span.start, span.end, true));
  for (const token of line.matchAll(/[^\s`"]+/gu)) {
    const raw = token[0];
    if (raw.length > MAX_TOKEN) continue;
    let start = token.index;
    let end = start + raw.length;
    const lead = LEADING_JUNK.exec(raw)?.[0].length ?? 0;
    start += lead;
    end = start + trimEnd(line.slice(start, end)).length;
    if (end > start) add(pathAt(line, start, end, false));
  }
  return found.sort((a, b) => a.start - b.start || b.end - a.end);
}

function findUrls(line: string): TextLink[] {
  const urls: TextLink[] = [];
  for (const match of line.matchAll(URL_PATTERN)) {
    const text = trimEnd(match[0]);
    try {
      const url = new URL(text);
      if (url.hostname === "") continue;
    } catch {
      continue;
    }
    const end = match.index + text.length;
    urls.push({ start: match.index, end, pathEnd: end, target: { kind: "url", url: text } });
  }
  return urls;
}

/**
 * Strips sentence punctuation at the end and closing brackets without an opening one in the text,
 * so `(see src/a.ts).` gives `src/a.ts` but `f(x)` keeps its bracket.
 */
function trimEnd(text: string): string {
  let result = text;
  for (;;) {
    const before = result;
    result = result.replace(TRAILING_JUNK, "");
    for (const [open, close] of [
      ["(", ")"],
      ["[", "]"],
      ["{", "}"],
    ] as const) {
      while (result.endsWith(close) && count(result, close) > count(result, open)) {
        result = result.slice(0, -1);
      }
    }
    if (result === before) return result;
  }
}

function count(text: string, char: string): number {
  return text.split(char).length - 1;
}

/** Spans between matching quotes, the quotes left out. */
function quotedSpans(line: string): { start: number; end: number }[] {
  const spans: { start: number; end: number }[] = [];
  let index = 0;
  while (index < line.length) {
    const quote = line.charAt(index);
    const opens = QUOTES.has(quote) && (index === 0 || BEFORE_QUOTE.test(line.charAt(index - 1)));
    if (!opens) {
      index += 1;
      continue;
    }
    let close = line.indexOf(quote, index + 1);
    while (close !== -1 && close + 1 < line.length && !AFTER_QUOTE.test(line.charAt(close + 1))) {
      close = line.indexOf(quote, close + 1);
    }
    if (close === -1 || close - index - 1 > MAX_QUOTED) {
      index += 1;
      continue;
    }
    if (close > index + 1) spans.push({ start: index + 1, end: close });
    index = close + 1;
  }
  return spans;
}

/** The path in `line[start, end)`, with an optional `:line:col` suffix, or `null`. */
function pathAt(line: string, start: number, end: number, quoted: boolean): TextLink | null {
  const text = line.slice(start, end);
  if (text.trim() !== text || text.includes("://")) return null;
  const suffix = LINE_SUFFIX.exec(text);
  const path = suffix ? text.slice(0, suffix.index) : text;
  if (!looksLikePath(path, quoted)) return null;
  const lineNumber = suffix?.[1] === undefined ? null : Number(suffix[1]);
  const column = suffix?.[2] === undefined ? null : Number(suffix[2]);
  return {
    start,
    end,
    pathEnd: start + path.length,
    target: { kind: "path", path, line: lineNumber, column },
  };
}

function looksLikePath(path: string, quoted: boolean): boolean {
  if (path === "" || path.length > MAX_TOKEN || NOT_IN_PATH.test(path)) return false;
  if (!quoted && /\s/u.test(path)) return false;
  // UNC and device paths are never looked up (pathCheck.ts, path_links.rs).
  if (/^[\\/]{2}/u.test(path)) return false;
  if (WINDOWS_ABSOLUTE.test(path)) return path.indexOf(":", 2) === -1;
  // A colon elsewhere is a scheme (`mailto:`) or a label (`error:`), not a path.
  if (path.includes(":")) return false;
  if (path.startsWith("/")) return path.length > 1;
  if (/^\.{1,2}[\\/]/u.test(path)) return true;
  if (SEPARATOR.test(path)) return /[\p{L}\p{N}]/u.test(path);
  return BARE_FILE.test(path);
}

/** One segment of a path: `[start, end)` within the path text. Opening it opens `[0, end)`. */
export interface PathSegment {
  start: number;
  end: number;
}

/** The names between separators, so `docs/wiki/index.md` gives `docs`, `wiki`, `index.md`. */
export function pathSegments(path: string): PathSegment[] {
  const segments: PathSegment[] = [];
  let start = 0;
  for (let index = 0; index <= path.length; index += 1) {
    if (index < path.length && !SEPARATOR.test(path.charAt(index))) continue;
    if (index > start) segments.push({ start, end: index });
    start = index + 1;
  }
  return segments;
}

/** A Windows folder: drive letter or UNC share. */
export function isWindowsPath(path: string): boolean {
  return /^[A-Za-z]:(?:[\\/]|$)/u.test(path) || /^\\\\/u.test(path);
}

/**
 * `path` as an absolute path in the style of the session's folder `cwd`, `.` and `..` resolved;
 * `null` when it cannot be resolved there (a UNC folder, a drive relative path, `/x` on Windows
 * that is not Git Bash's `/c/...`).
 */
export function resolvePath(cwd: string, path: string): string | null {
  return isWindowsPath(cwd) ? resolveWindows(cwd, path) : resolvePosix(cwd, path);
}

function resolveWindows(cwd: string, path: string): string | null {
  if (!/^[A-Za-z]:/u.test(cwd)) return null;
  const text = path.replace(/\//gu, "\\");
  let full: string;
  if (/^[A-Za-z]:(?:\\|$)/u.test(text)) full = text;
  else if (/^\\[A-Za-z](?:\\|$)/u.test(text)) {
    // Git Bash and MSYS print `/c/Users/...` for `C:\Users\...`.
    full = `${text.charAt(1).toUpperCase()}:${text.slice(2)}`;
  } else if (text.startsWith("\\") || /^[A-Za-z]:/u.test(text)) return null;
  else full = `${cwd.replace(/\//gu, "\\")}\\${text}`;
  const drive = full.slice(0, 2);
  const parts = normalize(full.slice(2).split("\\"));
  return `${drive}\\${parts.join("\\")}`;
}

function resolvePosix(cwd: string, path: string): string | null {
  if (!cwd.startsWith("/")) return null;
  const full = path.startsWith("/") ? path : `${cwd}/${path}`;
  return `/${normalize(full.split("/")).join("/")}`;
}

/** Drops empty and `.` parts and resolves `..`; above the root stays at the root. */
function normalize(parts: readonly string[]): string[] {
  const result: string[] = [];
  for (const part of parts) {
    if (part === "" || part === ".") continue;
    if (part === "..") result.pop();
    else result.push(part);
  }
  return result;
}

/**
 * `absolute` relative to the folder `root`, `/` separated, as the file viewer takes it; `null`
 * outside the folder or for the folder itself. Without case on Windows.
 */
export function relativeInside(root: string, absolute: string): string | null {
  const windows = isWindowsPath(root);
  const base = resolvePath(root, ".");
  if (base === null) return null;
  const separator = windows ? "\\" : "/";
  const prefix = base.endsWith(separator) ? base : base + separator;
  const fold = (text: string) => (windows ? text.toLowerCase() : text);
  if (!fold(absolute).startsWith(fold(prefix))) return null;
  const rest = absolute.slice(prefix.length);
  return rest === "" ? null : rest.split(separator).join("/");
}

/** File types the viewer shows only as "binary": their own program serves them better. */
const NOT_FOR_VIEWER =
  /\.(pdf|docx?|xlsx?|pptx?|od[tsp]|rtf|zip|7z|rar|tar|gz|tgz|bz2|xz|zst|exe|dll|msi|so|dylib|bin|iso|img|dmg|mp3|mp4|m4a|wav|flac|ogg|opus|mov|avi|mkv|webm|ico|bmp|tiff?|psd|heic|woff2?|ttf|otf|sqlite3?|db|class|jar|pyc|o|obj|lib|a|wasm|node)$/iu;

/** What a click on an existing path does. */
export type PathAction =
  { kind: "viewer"; relative: string } | { kind: "folder" } | { kind: "system" };

/**
 * A folder opens in the file manager. A file in the session's folder opens in Hangar's viewer,
 * unless the viewer could only call it binary; anything else in its default program.
 */
export function pathAction(kind: "file" | "dir", absolute: string, root: string): PathAction {
  if (kind === "dir") return { kind: "folder" };
  const relative = relativeInside(root, absolute);
  if (relative === null || NOT_FOR_VIEWER.test(relative)) return { kind: "system" };
  return { kind: "viewer", relative };
}

/**
 * Among overlapping candidates the longest existing one wins (a quoted path with spaces over the
 * words inside it); `live` says which candidates exist. Sorted by position.
 */
export function pickLinks<T extends TextLink>(
  candidates: readonly T[],
  live: (link: T) => boolean,
): T[] {
  const chosen: T[] = [];
  const bySize = [...candidates].sort(
    (a, b) => b.end - b.start - (a.end - a.start) || a.start - b.start,
  );
  for (const link of bySize) {
    if (!live(link)) continue;
    if (chosen.some((other) => link.start < other.end && other.start < link.end)) continue;
    chosen.push(link);
  }
  return chosen.sort((a, b) => a.start - b.start);
}

/** One cell as xterm stores it: a wide character's second cell has width 0. */
export interface LinkCell {
  chars: string;
  width: number;
}

/** A buffer cell position as xterm's links take it: 1 based column and buffer line. */
export interface CellPosition {
  x: number;
  y: number;
}

/** A line's text with the first and last cell of each UTF-16 unit. */
export interface CellText {
  text: string;
  from: CellPosition[];
  to: CellPosition[];
}

/**
 * Joins the rows of one logical line (a soft wrapped line spans several) into text, remembering
 * which cells each character came from. `firstRow` is the 0 based buffer line of `rows[0]`.
 */
export function cellText(rows: readonly (readonly LinkCell[])[], firstRow: number): CellText {
  let text = "";
  const from: CellPosition[] = [];
  const to: CellPosition[] = [];
  rows.forEach((cells, offset) => {
    const y = firstRow + offset + 1;
    cells.forEach((cell, x) => {
      if (cell.width === 0) return;
      const chars = cell.chars === "" ? " " : cell.chars;
      text += chars;
      for (let unit = 0; unit < chars.length; unit += 1) {
        from.push({ x: x + 1, y });
        to.push({ x: x + cell.width, y });
      }
    });
  });
  return { text, from, to };
}

/** A hover area of a link and what a click on it opens: the address, or the path up to there. */
export interface LinkSpan {
  start: number;
  end: number;
  opens: string;
  /** The whole link; a folder on the way otherwise. */
  whole: boolean;
}

/**
 * The hover areas of a link. Normally the whole link; with `segments` (Shift held) a path splits
 * into its names, each opening the path up to it. The last name keeps a `:line:col` suffix.
 */
export function linkSpans(link: TextLink, segments: boolean): LinkSpan[] {
  if (link.target.kind === "url") {
    return [{ start: link.start, end: link.end, opens: link.target.url, whole: true }];
  }
  const path = link.target.path;
  if (!segments) return [{ start: link.start, end: link.end, opens: path, whole: true }];
  const parts = pathSegments(path);
  return parts.map((part, index) => {
    const last = index === parts.length - 1;
    return {
      start: link.start + part.start,
      end: last ? link.end : link.start + part.end,
      opens: last ? path : path.slice(0, part.end),
      whole: last,
    };
  });
}
