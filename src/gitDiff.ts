// Changes panel (Ctrl+Shift+D): checks the answer of `git_diff`, splits the unified diff per file
// and classifies its lines. Pure, so a 2 MiB diff can be tested without rendering it. The panel
// shows the text as text nodes only, never as HTML.

import type { MessageKey } from "./i18n";
import { hasInvisible } from "./invisible";

export interface FileChange {
  path: string;
  /** Porcelain v1 XY code, for example " M", "A ", "??". */
  status: string;
  additions: number;
  deletions: number;
}

export interface GitDiff {
  isRepo: boolean;
  files: FileChange[];
  diff: string;
  /** The diff text was cut at 2 MiB. */
  truncated: boolean;
}

/** Lines of a file shown at first and per "Show more". */
export const DIFF_PAGE_LINES = 300;
/** Files listed at first and per "Show more"; the totals above the list count all of them. */
export const FILE_PAGE = 500;
/** Files up to this many lines open by themselves ... */
const AUTO_OPEN_FILE_LINES = 200;
/** ... as long as the open files together stay below this. */
const AUTO_OPEN_TOTAL_LINES = 800;

function count(value: unknown): number {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 ? value : 0;
}

function fileChange(value: unknown): FileChange | null {
  if (typeof value !== "object" || value === null) return null;
  const path: unknown = Reflect.get(value, "path");
  const status: unknown = Reflect.get(value, "status");
  if (typeof path !== "string" || path === "" || typeof status !== "string") return null;
  return {
    path,
    status,
    additions: count(Reflect.get(value, "additions")),
    deletions: count(Reflect.get(value, "deletions")),
  };
}

/** Answer of `git_diff`; `null` if malformed. */
export function parseGitDiff(value: unknown): GitDiff | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const isRepo: unknown = Reflect.get(value, "isRepo");
  const files: unknown = Reflect.get(value, "files");
  const diff: unknown = Reflect.get(value, "diff");
  const truncated: unknown = Reflect.get(value, "truncated");
  if (typeof isRepo !== "boolean" || !Array.isArray(files) || typeof diff !== "string") return null;
  return {
    isRepo,
    files: files.map(fileChange).filter((file): file is FileChange => file !== null),
    diff,
    truncated: truncated === true,
  };
}

export type ChangeKind = "added" | "deleted" | "renamed" | "untracked" | "conflict" | "modified";

/** Kind of change from the porcelain XY code; the index (X) and the working tree (Y) together. */
export function changeKind(status: string): ChangeKind {
  const x = status.charAt(0);
  const y = status.charAt(1);
  if (status === "??") return "untracked";
  if (x === "U" || y === "U" || status === "AA" || status === "DD") return "conflict";
  if (x === "R" || y === "R" || x === "C" || y === "C") return "renamed";
  if (x === "A" || y === "A") return "added";
  if (x === "D" || y === "D") return "deleted";
  return "modified";
}

/** One letter for the list (shape, not only color) and its spoken name. */
export const CHANGE_LABELS: Record<ChangeKind, { letter: string; name: MessageKey }> = {
  added: { letter: "A", name: "changes.added" },
  deleted: { letter: "D", name: "changes.deleted" },
  renamed: { letter: "R", name: "changes.renamed" },
  untracked: { letter: "U", name: "changes.untracked" },
  conflict: { letter: "!", name: "changes.conflict" },
  modified: { letter: "M", name: "changes.modified" },
};

export interface FileDiff {
  /** New path (the old one for a deleted file). */
  path: string;
  /** Lines from the first hunk header on, each with its leading character. */
  lines: string[];
  /** "Binary files ... differ": there are no lines to show. */
  binary: boolean;
}

/** Undoes git's C-style quoting of unusual paths (`"a/\303\244.txt"`), UTF-8 octal included. */
export function unquotePath(raw: string): string {
  if (!(raw.length >= 2 && raw.startsWith('"') && raw.endsWith('"'))) return raw;
  const body = raw.slice(1, -1);
  const bytes: number[] = [];
  const encoder = new TextEncoder();
  const escapes: Record<string, number> = { n: 10, t: 9, r: 13, '"': 34, "\\": 92, a: 7, b: 8 };
  for (let i = 0; i < body.length; i += 1) {
    const char = body.charAt(i);
    if (char !== "\\") {
      bytes.push(...encoder.encode(char));
      continue;
    }
    const next = body.charAt(i + 1);
    const octal = /^[0-7]{3}/.exec(body.slice(i + 1, i + 4));
    if (octal) {
      bytes.push(parseInt(octal[0], 8));
      i += 3;
    } else if (next in escapes) {
      bytes.push(escapes[next] ?? 0);
      i += 1;
    } else {
      bytes.push(92);
    }
  }
  return new TextDecoder().decode(new Uint8Array(bytes));
}

/** Path of a `--- a/x` or `+++ b/x` line without the prefix; `null` for `/dev/null`. */
function sidePath(line: string): string | null {
  const raw = unquotePath(line.slice(4).replace(/\t$/, ""));
  if (raw === "/dev/null") return null;
  return raw.startsWith("a/") || raw.startsWith("b/") ? raw.slice(2) : raw;
}

/** Path from `diff --git a/x b/x` when no `---`/`+++` lines follow (binary, mode only, rename). */
function headerPath(header: string): string {
  const rest = header.slice("diff --git ".length);
  if (rest.startsWith('"')) {
    const parts = /^("(?:[^"\\]|\\.)*") ("(?:[^"\\]|\\.)*"|.*)$/.exec(rest);
    const second = parts?.[2];
    if (second !== undefined) return unquotePath(second).replace(/^b\//, "");
  }
  // Same path on both sides (the usual case): "a/<p> b/<p>".
  const half = (rest.length - 1) / 2;
  if (Number.isInteger(half) && rest.slice(2, half) === rest.slice(half + 3)) {
    return rest.slice(half + 3);
  }
  const at = rest.lastIndexOf(" b/");
  return at >= 0 ? rest.slice(at + 3) : rest;
}

/** Splits `git diff` output into files; the text before the first `diff --git` is ignored. */
export function splitDiff(diff: string): FileDiff[] {
  const files: FileDiff[] = [];
  if (diff === "") return files;
  const lines = diff.split("\n");
  // A trailing newline gives an empty last element that is no line of the diff.
  if (lines[lines.length - 1] === "") lines.pop();
  let current: (FileDiff & { inBody: boolean; oldPath: string | null }) | null = null;
  const finish = () => {
    if (current) files.push({ path: current.path, lines: current.lines, binary: current.binary });
  };
  for (const raw of lines) {
    const line = raw.endsWith("\r") ? raw.slice(0, -1) : raw;
    if (line.startsWith("diff --git ")) {
      finish();
      current = {
        path: headerPath(line),
        lines: [],
        binary: false,
        inBody: false,
        oldPath: null,
      };
      continue;
    }
    if (!current) continue;
    if (!current.inBody) {
      if (line.startsWith("@@")) {
        current.inBody = true;
        current.lines.push(line);
      } else if (line.startsWith("--- ")) {
        current.oldPath = sidePath(line);
      } else if (line.startsWith("+++ ")) {
        current.path = sidePath(line) ?? current.oldPath ?? current.path;
      } else if (line.startsWith("rename to ")) {
        current.path = unquotePath(line.slice("rename to ".length));
      } else if (line.startsWith("Binary files ")) {
        current.binary = true;
      }
      continue;
    }
    current.lines.push(line);
  }
  finish();
  return files;
}

export type LineKind = "hunk" | "add" | "del" | "context" | "note";

/** Class of a diff body line: added, removed, unchanged, hunk header or "\ No newline". */
export function lineKind(line: string): LineKind {
  switch (line.charAt(0)) {
    case "+":
      return "add";
    case "-":
      return "del";
    case "@":
      return line.startsWith("@@") ? "hunk" : "context";
    case "\\":
      return "note";
    default:
      return "context";
  }
}

export interface ChangeEntry extends FileChange {
  kind: ChangeKind;
  /** `null`: no content to show (untracked, or cut off by the 2 MiB limit). */
  diff: FileDiff | null;
  /** Path or changed lines hold invisible or bidi characters (shown as placeholders). */
  hidden: boolean;
}

function hiddenIn(path: string, diff: FileDiff | null): boolean {
  return hasInvisible(path) || (diff?.lines.some(hasInvisible) ?? false);
}

/**
 * The file list in git's order, each with its part of the diff. Parts without a list entry (the
 * list and the text come from two git calls) are added at the end.
 */
export function changeEntries(result: GitDiff): ChangeEntry[] {
  const byPath = new Map(splitDiff(result.diff).map((file) => [file.path, file]));
  const entries: ChangeEntry[] = result.files.map((file) => {
    const diff = byPath.get(file.path) ?? null;
    byPath.delete(file.path);
    return { ...file, kind: changeKind(file.status), diff, hidden: hiddenIn(file.path, diff) };
  });
  for (const diff of byPath.values()) {
    let additions = 0;
    let deletions = 0;
    for (const line of diff.lines) {
      const kind = lineKind(line);
      if (kind === "add") additions += 1;
      else if (kind === "del") deletions += 1;
    }
    entries.push({
      path: diff.path,
      status: " M",
      additions,
      deletions,
      kind: "modified",
      diff,
      hidden: hiddenIn(diff.path, diff),
    });
  }
  return entries;
}

/** Files open from the start: small ones, while the total stays short enough to read. */
export function initiallyOpen(entries: readonly ChangeEntry[]): Set<string> {
  const open = new Set<string>();
  let total = 0;
  for (const entry of entries) {
    const size = entry.diff?.lines.length ?? 0;
    if (size === 0 || size > AUTO_OPEN_FILE_LINES) continue;
    if (total + size > AUTO_OPEN_TOTAL_LINES) break;
    total += size;
    open.add(entry.path);
  }
  return open;
}

/** Folder part and file name, so the list can show the name first. */
export function splitPath(path: string): { folder: string; name: string } {
  const at = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return at < 0
    ? { folder: "", name: path }
    : { folder: path.slice(0, at), name: path.slice(at + 1) };
}
