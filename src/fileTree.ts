// File tree of a session's folder in the right panel (tab "Files"): the tree's state as a reducer,
// the visible rows, the keyboard steps of the WAI-ARIA tree pattern and the markings (teammates
// editing a file, changes of this device's sessions). The backend lists one folder per call
// (`list_session_folder`, file_tree.rs); everything here is pure and tested in fileTree.test.ts.

import type { FolderEntry, FolderListing, PathEntry } from "./api";
import type { ChangeKind, Claim } from "./claims";
import { quotePath } from "./pathQuote";
import type { PeerView } from "./team";

// ---------------------------------------------------------------------------------------------
// Paths

/** A drive letter or UNC path: compared without case, `\` and `/` alike. */
export function isWindowsPath(path: string): boolean {
  return /^[a-z]:([\\/]|$)/i.test(path) || /^[\\/]{2}[^\\/]/.test(path);
}

export interface FolderKey {
  /** `/` separated, without a trailing separator, lower case on Windows. */
  key: string;
  windows: boolean;
}

/**
 * A folder as a comparison key. The backend spells the tree's root as Windows does (`C:\x`) and
 * the claims' worktree root as git does (`C:/x`); both must meet. On POSIX a backslash is part of
 * a name and stays.
 */
export function folderKeyOf(path: string): FolderKey {
  const plain = path.replace(/^\\\\\?\\UNC\\/i, "\\\\").replace(/^\\\\\?\\/, "");
  const windows = isWindowsPath(plain);
  let key = windows ? plain.replace(/\\/g, "/") : plain;
  while (key.length > 1 && key.endsWith("/") && !/^[a-z]:\/$/i.test(key)) key = key.slice(0, -1);
  return { key: windows ? key.toLowerCase() : key, windows };
}

/**
 * Where the tree's root lies inside the repository: `""` at its top, `"src/app"` below it, `null`
 * when the root is not inside `repoRoot` at all (or either is missing).
 */
export function rootOffset(root: string | null, repoRoot: string | null): string | null {
  if (root === null || repoRoot === null) return null;
  const tree = folderKeyOf(root);
  const repo = folderKeyOf(repoRoot);
  if (tree.windows !== repo.windows) return null;
  if (tree.key === repo.key) return "";
  const prefix = repo.key.endsWith("/") ? repo.key : `${repo.key}/`;
  return tree.key.startsWith(prefix) ? tree.key.slice(prefix.length) : null;
}

/** A path relative to the tree's root as the markings compare it. */
export function treeKey(path: string, windows: boolean): string {
  return windows ? path.toLowerCase() : path;
}

/**
 * A repository relative path (git spells it with `/`) as a tree key; `null` for a file outside the
 * tree's root. `offset` comes from `rootOffset` and is already a key.
 */
export function repoToTreeKey(path: string, offset: string, windows: boolean): string | null {
  const key = treeKey(path.replace(/^\.\//, ""), windows);
  if (key === "" || key.startsWith("/")) return null;
  if (offset === "") return key;
  const prefix = `${offset}/`;
  return key.startsWith(prefix) && key.length > prefix.length ? key.slice(prefix.length) : null;
}

/** `a/b/c.txt` → `["a", "a/b"]`: the folders that contain a path. */
export function ancestors(path: string): string[] {
  const parts = path.split("/");
  const found: string[] = [];
  for (let index = 1; index < parts.length; index += 1) {
    found.push(parts.slice(0, index).join("/"));
  }
  return found;
}

/**
 * What a right click on a tree row types into the terminal: `@` and the path from the project's
 * folder, as agent CLIs mention files, a folder with a trailing `/`, and a space so the next word
 * does not stick to it. A path with spaces is quoted the way agents read quoted paths.
 */
export function mentionText(path: string, folder: boolean): string {
  return `@${quotePath(folder ? `${path}/` : path, "agent")} `;
}

/**
 * A mention names its path relative to the tree's folder, so only a session working in that same
 * folder may take a dragged row; another pane's session would read a wrong path.
 */
export function takesMention(treeCwd: string, sessionCwd: string): boolean {
  return folderKeyOf(treeCwd).key === folderKeyOf(sessionCwd).key;
}

/** The mention of a tree row, for the right click and for dragging it onto a terminal. */
export function rowMention(row: ItemRow): string {
  return mentionText(row.path, row.entry.kind === "dir");
}

export function childPath(parent: string, name: string): string {
  return parent === "" ? name : `${parent}/${name}`;
}

// ---------------------------------------------------------------------------------------------
// Markings

/** One teammate session working on a file. */
export interface TeamEditor {
  member: string;
  device: string;
  name: string;
  /** Another device of the signed-in person. */
  own: boolean;
  agent: string;
  /** Since when this device first saw them on the file (ms), `null` when unknown. */
  since: number | null;
}

/** One of this device's other sessions changing a file, in another worktree of the repository. */
export interface SessionEditor {
  session: number;
  /** Since when the change is held (ms), `null` when unknown (seen only as a collision). */
  since: number | null;
}

export interface FileMark {
  /** Teammate sessions changing the file, each person and agent once. */
  editors: TeamEditor[];
  /** How the folder's worktree changed the file (the session's own claims), `null` if unchanged. */
  change: ChangeKind | null;
  /** This device's other sessions (other worktrees) that change the file too. */
  alsoIn: SessionEditor[];
}

export interface FolderMark {
  /** A teammate or another session of this device changes something inside. */
  team: boolean;
  /** Something inside is changed in this worktree. */
  change: boolean;
  /** Who changes something inside: teammates, each person and agent once ... */
  editors: TeamEditor[];
  /** ... and this device's other sessions, by ID. */
  sessions: number[];
}

export interface TreeMarks {
  files: ReadonlyMap<string, FileMark>;
  folders: ReadonlyMap<string, FolderMark>;
}

export const NO_MARKS: TreeMarks = { files: new Map(), folders: new Map() };

/** Whose hold on a file `MarkInput.sinceOf` is asked about. */
export type HolderRef =
  | { kind: "team"; member: string; device: string; session: string }
  | { kind: "worktree"; root: string };

/** Another worktree of the same repository with sessions of this device in it. */
export interface OtherWorktree {
  root: string;
  sessions: readonly number[];
  claims: readonly Claim[];
}

export interface MarkInput {
  /** `FolderListing.root` and `.repoRoot` of the tree. */
  root: string | null;
  repoRoot: string | null;
  /** Teammates' devices sharing this repository (`peersOf`, repoPeers.ts). */
  peers: readonly PeerView[];
  /** Claims of this device's sessions with their worktree roots; only the tree's worktree counts. */
  changes: readonly { root: string | null; claims: readonly Claim[] }[];
  /** Files of the session that other sessions of this device change too (repository relative). */
  overlaps: readonly { path: string; session: number }[];
  /** Other worktrees of the repository and their sessions (`otherWorktrees`, fileActivity.ts). */
  others?: readonly OtherWorktree[];
  /** Since when a holder holds a repository relative path (fileActivity.ts), `null` if unknown. */
  sinceOf?: (holder: HolderRef, path: string) => number | null;
}

/** Worse changes win when two sessions report the same file differently. */
const CHANGE_RANK: Record<ChangeKind, number> = {
  untracked: 0,
  added: 1,
  modified: 2,
  deleted: 3,
  conflicted: 4,
};

function sameEditor(a: TeamEditor, b: TeamEditor): boolean {
  return a.member === b.member && a.device === b.device && a.agent === b.agent;
}

/** The earlier of two times; an unknown time gives way to a known one. */
function earlier(a: number | null, b: number | null): number | null {
  if (a === null) return b;
  if (b === null) return a;
  return Math.min(a, b);
}

function addEditor(list: TeamEditor[], editor: TeamEditor): void {
  const known = list.find((other) => sameEditor(other, editor));
  if (known === undefined) list.push({ ...editor });
  else known.since = earlier(known.since, editor.since);
}

function addSession(list: SessionEditor[], editor: SessionEditor): void {
  const known = list.find((other) => other.session === editor.session);
  if (known === undefined) list.push({ ...editor });
  else known.since = earlier(known.since, editor.since);
}

/**
 * Every marking of the tree: teammates' files, the worktree's changes and the files this device's
 * other sessions change (other worktrees, collisions), keyed by `treeKey`, plus the folders that
 * contain them. Outside a repository, or with a root outside it, nothing is marked.
 */
export function buildMarks(input: MarkInput): TreeMarks {
  const offset = rootOffset(input.root, input.repoRoot);
  if (offset === null || input.root === null || input.repoRoot === null) return NO_MARKS;
  const windows = folderKeyOf(input.root).windows;
  const repo = folderKeyOf(input.repoRoot).key;
  const sinceOf = input.sinceOf ?? (() => null);
  const files = new Map<string, FileMark>();
  const folders = new Map<string, FolderMark>();
  const mark = (path: string): FileMark | null => {
    const key = repoToTreeKey(path, offset, windows);
    if (key === null) return null;
    let entry = files.get(key);
    if (entry === undefined) {
      entry = { editors: [], change: null, alsoIn: [] };
      files.set(key, entry);
    }
    return entry;
  };
  const folderMarks = (path: string): FolderMark[] => {
    const key = repoToTreeKey(path, offset, windows);
    if (key === null) return [];
    return ancestors(key).map((folder) => {
      let entry = folders.get(folder);
      if (entry === undefined) {
        entry = { team: false, change: false, editors: [], sessions: [] };
        folders.set(folder, entry);
      }
      return entry;
    });
  };

  for (const peer of input.peers) {
    for (const session of peer.sessions) {
      const holder: HolderRef = {
        kind: "team",
        member: peer.member,
        device: peer.device,
        session: session.key,
      };
      for (const path of session.files) {
        const entry = mark(path);
        if (entry === null) continue;
        const editor: TeamEditor = {
          member: peer.member,
          device: peer.device,
          name: peer.name,
          own: peer.own,
          agent: session.agent,
          since: sinceOf(holder, path),
        };
        addEditor(entry.editors, editor);
        for (const folder of folderMarks(path)) {
          folder.team = true;
          addEditor(folder.editors, editor);
        }
      }
    }
  }

  for (const worktree of input.changes) {
    if (worktree.root === null || folderKeyOf(worktree.root).key !== repo) continue;
    for (const claim of worktree.claims) {
      // A deleted file has no row; its folder shows the change still.
      if (claim.kind !== "deleted") {
        const entry = mark(claim.path);
        if (entry === null) continue;
        if (entry.change === null || CHANGE_RANK[claim.kind] > CHANGE_RANK[entry.change]) {
          entry.change = claim.kind;
        }
      }
      for (const folder of folderMarks(claim.path)) folder.change = true;
    }
  }

  for (const worktree of input.others ?? []) {
    // The tree's own worktree is the change coloring above, not someone else.
    if (folderKeyOf(worktree.root).key === repo) continue;
    const holder: HolderRef = { kind: "worktree", root: worktree.root };
    for (const claim of worktree.claims) {
      const since = sinceOf(holder, claim.path);
      // A deleted file has no row; its folders still show who works there.
      const entry = claim.kind === "deleted" ? null : mark(claim.path);
      const inside = folderMarks(claim.path);
      for (const session of worktree.sessions) {
        if (entry !== null) addSession(entry.alsoIn, { session, since });
        for (const folder of inside) {
          folder.team = true;
          if (!folder.sessions.includes(session)) folder.sessions.push(session);
        }
      }
    }
  }

  for (const overlap of input.overlaps) {
    const entry = mark(overlap.path);
    if (entry === null) continue;
    addSession(entry.alsoIn, { session: overlap.session, since: null });
  }

  return { files, folders };
}

/**
 * How many others work inside a folder, for the count on a collapsed folder: each person once (as
 * `avatarStack` counts them) and each of this device's other sessions.
 */
export function folderPeople(mark: FolderMark): number {
  return avatarStack(mark.editors, 0).more + mark.sessions.length;
}

/**
 * The row lies on the path of this worktree's changes: a changed file, or a folder with a change
 * somewhere inside (also while it is collapsed). Its name then shows in the "working" color, so
 * the way from the top folder down to every file the agent touched stands out.
 */
export function onChangePath(
  kind: FolderEntry["kind"],
  fileMark: FileMark | null,
  folderMark: FolderMark | null,
): boolean {
  if (kind === "dir") return folderMark?.change === true;
  return kind === "file" && fileMark !== null && fileMark.change !== null;
}

/** Avatars at the end of a row: each person once, at most `max`, the rest as "+N". */
export function avatarStack(
  editors: readonly TeamEditor[],
  max: number,
): { shown: TeamEditor[]; more: number } {
  const people: TeamEditor[] = [];
  for (const editor of editors) {
    if (!people.some((known) => known.member === editor.member && known.own === editor.own)) {
      people.push(editor);
    }
  }
  return { shown: people.slice(0, max), more: Math.max(0, people.length - max) };
}

/** The letter of a change, as VS Code's git decorations show it. */
export const CHANGE_LETTERS: Record<ChangeKind, string> = {
  added: "A",
  modified: "M",
  deleted: "D",
  untracked: "U",
  conflicted: "!",
};

// ---------------------------------------------------------------------------------------------
// Tree state

export type FolderLoad =
  | { kind: "loading" }
  | { kind: "failed"; message: string }
  | { kind: "loaded"; entries: readonly FolderEntry[]; truncated: boolean };

export interface TreeState {
  root: string | null;
  repoRoot: string | null;
  /** Per folder relative to the root (`""` is the root itself), `/` separated. */
  folders: ReadonlyMap<string, FolderLoad>;
  expanded: ReadonlySet<string>;
  /** The row with the roving focus and selection. */
  focused: string | null;
}

export const INITIAL_TREE: TreeState = {
  root: null,
  repoRoot: null,
  folders: new Map(),
  expanded: new Set(),
  focused: null,
};

export type TreeAction =
  | { type: "request"; path: string }
  | { type: "loaded"; path: string; listing: FolderListing }
  | { type: "failed"; path: string; message: string }
  | { type: "expand"; path: string }
  | { type: "collapse"; path: string }
  | { type: "focus"; path: string };

function withFolder(state: TreeState, path: string, load: FolderLoad): TreeState {
  const folders = new Map(state.folders);
  folders.set(path, load);
  return { ...state, folders };
}

export function treeReducer(state: TreeState, action: TreeAction): TreeState {
  switch (action.type) {
    case "request": {
      // Reading again keeps what is shown; only a folder never read shows "loading".
      const current = state.folders.get(action.path);
      if (current?.kind === "loaded" || current?.kind === "loading") return state;
      return withFolder(state, action.path, { kind: "loading" });
    }
    case "loaded": {
      const next = withFolder(state, action.path, {
        kind: "loaded",
        entries: action.listing.entries,
        truncated: action.listing.truncated,
      });
      return action.path === ""
        ? { ...next, root: action.listing.root, repoRoot: action.listing.repoRoot }
        : next;
    }
    case "failed": {
      // A folder already shown keeps its entries when reading it again fails.
      if (state.folders.get(action.path)?.kind === "loaded") return state;
      return withFolder(state, action.path, { kind: "failed", message: action.message });
    }
    case "expand": {
      if (state.expanded.has(action.path)) return state;
      const expanded = new Set(state.expanded);
      expanded.add(action.path);
      return { ...state, expanded };
    }
    case "collapse": {
      if (!state.expanded.has(action.path)) return state;
      const expanded = new Set(state.expanded);
      expanded.delete(action.path);
      return { ...state, expanded };
    }
    case "focus":
      return state.focused === action.path ? state : { ...state, focused: action.path };
  }
}

/** Folders to read again on a refresh: the root and every expanded folder that is visible. */
export function foldersToRefresh(state: TreeState): string[] {
  const shown = [""];
  for (const path of state.expanded) {
    if (ancestors(path).every((folder) => state.expanded.has(folder))) shown.push(path);
  }
  return shown.sort((a, b) => a.split("/").length - b.split("/").length);
}

export interface ItemRow {
  kind: "item";
  path: string;
  name: string;
  entry: FolderEntry;
  /** 0 for the root's entries. */
  depth: number;
  /** `null` for the root's entries. */
  parent: string | null;
  expanded: boolean;
  /** 1-based position among its siblings, and their number (aria-posinset, aria-setsize). */
  position: number;
  siblings: number;
  /** Filtered tree: a folder shown only because a match lies inside it. */
  context?: boolean;
}

export type HintKind = "loading" | "failed" | "empty" | "truncated";

export interface HintRow {
  kind: "hint";
  /** The folder the hint belongs to. */
  folder: string;
  depth: number;
  hint: HintKind;
  message: string;
}

export type TreeRow = ItemRow | HintRow;

/** The rows on screen, top to bottom: entries of expanded folders below them, hints in between. */
export function visibleRows(state: TreeState): TreeRow[] {
  const rows: TreeRow[] = [];
  const walk = (folder: string, depth: number) => {
    const load = state.folders.get(folder);
    if (load === undefined || load.kind === "loading") {
      rows.push({ kind: "hint", folder, depth, hint: "loading", message: "" });
      return;
    }
    if (load.kind === "failed") {
      rows.push({ kind: "hint", folder, depth, hint: "failed", message: load.message });
      return;
    }
    if (load.entries.length === 0) {
      rows.push({ kind: "hint", folder, depth, hint: "empty", message: "" });
    }
    load.entries.forEach((entry, index) => {
      const path = childPath(folder, entry.name);
      const expanded = entry.kind === "dir" && state.expanded.has(path);
      rows.push({
        kind: "item",
        path,
        name: entry.name,
        entry,
        depth,
        parent: folder === "" ? null : folder,
        expanded,
        position: index + 1,
        siblings: load.entries.length,
      });
      if (expanded) walk(path, depth + 1);
    });
    if (load.truncated) rows.push({ kind: "hint", folder, depth, hint: "truncated", message: "" });
  };
  walk("", 0);
  return rows;
}

// ---------------------------------------------------------------------------------------------
// Filter

/** Matches the filtered tree shows at most; beyond them a hint asks for a longer filter. */
export const MAX_FILTER_MATCHES = 300;

/** The filter's words: lower case, split at spaces. */
export function filterWords(query: string): string[] {
  return query.trim().toLowerCase().split(/\s+/).filter(Boolean);
}

/**
 * How well a path matches, `null` for no match. Every word must appear somewhere in the path and
 * at least one in the last name, so "src" finds the folder `src` and `src.rs`, not every file
 * inside `src/`, while "src main" finds `src/main.rs`. A word in the name counts more, at its
 * start more still.
 */
export function filterScore(path: string, words: readonly string[]): number | null {
  if (words.length === 0) return null;
  const lower = path.toLowerCase();
  const name = lower.slice(lower.lastIndexOf("/") + 1);
  let score = 0;
  let inName = false;
  for (const word of words) {
    if (!lower.includes(word)) return null;
    if (name.startsWith(word)) {
      score += 3;
      inName = true;
    } else if (name.includes(word)) {
      score += 2;
      inName = true;
    } else {
      score += 1;
    }
  }
  return inName ? score : null;
}

/** Every path the tree has read so far: what the filter searches until the full index is in. */
export function loadedPaths(state: Pick<TreeState, "folders">): PathEntry[] {
  const paths: PathEntry[] = [];
  for (const [folder, load] of state.folders) {
    if (load.kind !== "loaded") continue;
    for (const entry of load.entries) {
      paths.push({ path: childPath(folder, entry.name), kind: entry.kind });
    }
  }
  return paths;
}

export interface FilterResult {
  /** The matches and the folders above them, as a tree with every such folder open. */
  rows: ItemRow[];
  /** Matches in all, also those beyond the limit. */
  matched: number;
}

/** One collator for every comparison: `localeCompare` with options builds one per call. */
const naturalOrder = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/** Folders first, then the rest, each in natural order (as the backend sorts a folder). */
function compareEntries(a: PathEntry, b: PathEntry): number {
  const group = (entry: PathEntry) => (entry.kind === "dir" ? 0 : 1);
  return (
    group(a) - group(b) ||
    naturalOrder.compare(a.path, b.path) ||
    (a.path < b.path ? -1 : a.path > b.path ? 1 : 0)
  );
}

function depthOf(path: string): number {
  let depth = 0;
  for (let at = path.indexOf("/"); at >= 0; at = path.indexOf("/", at + 1)) depth += 1;
  return depth;
}

/**
 * The filtered tree: the best `max` matches with the folders that lead to them, those folders
 * open. Paths appear once even when the source names them twice.
 */
export function filterTree(
  entries: readonly PathEntry[],
  query: string,
  max = MAX_FILTER_MATCHES,
): FilterResult {
  const words = filterWords(query);
  const scored: { entry: PathEntry; score: number; depth: number }[] = [];
  const seen = new Set<string>();
  for (const entry of entries) {
    if (seen.has(entry.path)) continue;
    seen.add(entry.path);
    const score = filterScore(entry.path, words);
    if (score !== null) scored.push({ entry, score, depth: depthOf(entry.path) });
  }
  scored.sort((a, b) => b.score - a.score || a.depth - b.depth || compareEntries(a.entry, b.entry));
  const kept = scored.slice(0, max).map((item) => item.entry);

  const shown = new Map<string, { entry: PathEntry; matched: boolean }>();
  for (const entry of kept) shown.set(entry.path, { entry, matched: true });
  for (const entry of kept) {
    for (const folder of ancestors(entry.path)) {
      if (!shown.has(folder)) {
        shown.set(folder, { entry: { path: folder, kind: "dir" }, matched: false });
      }
    }
  }
  const children = new Map<string, PathEntry[]>();
  for (const { entry } of shown.values()) {
    const cut = entry.path.lastIndexOf("/");
    const parent = cut < 0 ? "" : entry.path.slice(0, cut);
    const list = children.get(parent) ?? [];
    list.push(entry);
    children.set(parent, list);
  }

  const rows: ItemRow[] = [];
  const walk = (folder: string, depth: number) => {
    const list = (children.get(folder) ?? []).sort(compareEntries);
    list.forEach((entry, index) => {
      const open = children.has(entry.path);
      const name = entry.path.slice(entry.path.lastIndexOf("/") + 1);
      rows.push({
        kind: "item",
        path: entry.path,
        name,
        entry: { name, kind: entry.kind, ignored: false },
        depth,
        parent: folder === "" ? null : folder,
        expanded: open,
        position: index + 1,
        siblings: list.length,
        context: shown.get(entry.path)?.matched !== true,
      });
      if (open) walk(entry.path, depth + 1);
    });
  };
  walk("", 0);
  return { rows, matched: scored.length };
}

/**
 * A key on the filtered tree. Moving works as in the tree; folders do not fold, because the
 * filter decides what shows: Left goes to the folder above, Right, Enter and Space on a folder
 * show it in the whole tree (`reveal`) with the filter cleared.
 */
export function filteredStep(
  rows: readonly TreeRow[],
  focused: string | null,
  key: string,
): TreeStep | null {
  const step = treeStep(rows, focused, key);
  if (step === null || (step.kind !== "expand" && step.kind !== "collapse")) return step;
  if (key === "ArrowLeft") {
    const row = rows.find(
      (item): item is ItemRow => item.kind === "item" && item.path === step.path,
    );
    return row?.parent ? { kind: "focus", path: row.parent } : { kind: "none" };
  }
  return { kind: "reveal", path: step.path };
}

/** Folders to open so that `path` shows in the whole tree: those above it and, for a folder, itself. */
export function revealFolders(path: string, folder: boolean): string[] {
  return folder ? [...ancestors(path), path] : ancestors(path);
}

/** What a key on the tree does (WAI-ARIA tree view pattern); `null`: not the tree's key. */
export type TreeStep =
  | { kind: "focus"; path: string }
  | { kind: "expand"; path: string }
  | { kind: "collapse"; path: string }
  | { kind: "select"; path: string }
  /** Show the file in the viewer window. */
  | { kind: "open"; path: string }
  /** Filtered tree: clear the filter and show this folder in the whole tree. */
  | { kind: "reveal"; path: string }
  | { kind: "none" };

/**
 * A key typed on the tree that starts the filter: one printable character without Ctrl, Alt or
 * Meta. Space stays the tree's (it folds a folder).
 */
export function startsFilter(key: string, modifiers: boolean): boolean {
  // Named keys ("Enter", "ArrowDown") are words; a character is one code point.
  return !modifiers && key !== " " && Array.from(key).length === 1;
}

export function treeStep(
  rows: readonly TreeRow[],
  focused: string | null,
  key: string,
): TreeStep | null {
  const items = rows.filter((row): row is ItemRow => row.kind === "item");
  const first = items[0];
  const last = items[items.length - 1];
  if (first === undefined || last === undefined) return null;
  const index = items.findIndex((row) => row.path === focused);
  const current = items[index] ?? first;
  const focus = (row: ItemRow | undefined): TreeStep =>
    row === undefined ? { kind: "none" } : { kind: "focus", path: row.path };
  switch (key) {
    case "ArrowDown":
      return index < 0 ? focus(first) : focus(items[index + 1]);
    case "ArrowUp":
      return index < 0 ? focus(first) : focus(items[index - 1]);
    case "Home":
      return focus(first);
    case "End":
      return focus(last);
    case "ArrowRight":
      if (current.entry.kind !== "dir") return { kind: "none" };
      if (!current.expanded) return { kind: "expand", path: current.path };
      return focus(items.find((row) => row.parent === current.path));
    case "ArrowLeft":
      if (current.expanded) return { kind: "collapse", path: current.path };
      return focus(items.find((row) => row.path === current.parent));
    case "Enter":
    case " ":
      if (current.entry.kind === "dir") {
        return current.expanded
          ? { kind: "collapse", path: current.path }
          : { kind: "expand", path: current.path };
      }
      // Enter opens a file in the viewer, as in VS Code's explorer; links are never opened.
      return key === "Enter" && current.entry.kind === "file"
        ? { kind: "open", path: current.path }
        : { kind: "select", path: current.path };
    default:
      return null;
  }
}

/** The answer the backend gives while another listing of the same folder still runs. */
export function isBusyError(message: string): boolean {
  return message === "The folder is still being read";
}
