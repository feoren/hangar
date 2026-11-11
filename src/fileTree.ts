// File tree of a session's folder in the right panel (tab "Files"): the tree's state as a reducer,
// the visible rows, the keyboard steps of the WAI-ARIA tree pattern and the markings (teammates
// editing a file, changes of this device's sessions). The backend lists one folder per call
// (`list_session_folder`, file_tree.rs); everything here is pure and tested in fileTree.test.ts.

import type { FolderEntry, FolderListing } from "./api";
import type { ChangeKind, Claim } from "./claims";
import type { PeerView, TeamActivity } from "./team";

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
}

export interface FileMark {
  /** Teammate sessions changing the file, each person and agent once. */
  editors: TeamEditor[];
  /** How the folder's worktree changed the file (the session's own claims), `null` if unchanged. */
  change: ChangeKind | null;
  /** This device's other sessions (other worktrees) that change the file too, by ID. */
  alsoIn: number[];
}

export interface FolderMark {
  /** A teammate edits something inside. */
  team: boolean;
  /** Something inside is changed in this worktree. */
  change: boolean;
}

export interface TreeMarks {
  files: ReadonlyMap<string, FileMark>;
  folders: ReadonlyMap<string, FolderMark>;
}

export const NO_MARKS: TreeMarks = { files: new Map(), folders: new Map() };

export interface MarkInput {
  /** `FolderListing.root` and `.repoRoot` of the tree. */
  root: string | null;
  repoRoot: string | null;
  /** Teammates' devices sharing this repository (`peersOf`). */
  peers: readonly PeerView[];
  /** Claims of this device's sessions with their worktree roots; only the tree's worktree counts. */
  changes: readonly { root: string | null; claims: readonly Claim[] }[];
  /** Files of the session that other sessions of this device change too (repository relative). */
  overlaps: readonly { path: string; session: number }[];
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

/**
 * Every marking of the tree: teammates' files, the worktree's changes and collisions with this
 * device's other sessions, keyed by `treeKey`, plus the folders that contain them. Outside a
 * repository, or with a root outside it, nothing is marked.
 */
export function buildMarks(input: MarkInput): TreeMarks {
  const offset = rootOffset(input.root, input.repoRoot);
  if (offset === null || input.root === null || input.repoRoot === null) return NO_MARKS;
  const windows = folderKeyOf(input.root).windows;
  const repo = folderKeyOf(input.repoRoot).key;
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
  const markFolders = (path: string, team: boolean) => {
    const key = repoToTreeKey(path, offset, windows);
    if (key === null) return;
    for (const folder of ancestors(key)) {
      const entry = folders.get(folder) ?? { team: false, change: false };
      folders.set(folder, team ? { ...entry, team: true } : { ...entry, change: true });
    }
  };

  for (const peer of input.peers) {
    for (const session of peer.sessions) {
      const editor: TeamEditor = {
        member: peer.member,
        device: peer.device,
        name: peer.name,
        own: peer.own,
        agent: session.agent,
      };
      for (const path of session.files) {
        const entry = mark(path);
        if (entry === null) continue;
        if (!entry.editors.some((known) => sameEditor(known, editor))) entry.editors.push(editor);
        markFolders(path, true);
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
      markFolders(claim.path, false);
    }
  }

  for (const overlap of input.overlaps) {
    const entry = mark(overlap.path);
    if (entry === null) continue;
    if (!entry.alsoIn.includes(overlap.session)) entry.alsoIn.push(overlap.session);
  }

  return { files, folders };
}

/**
 * The teammates working in this session's repository: of every shared project this session is
 * shared with, the teammates' sessions with the same root commit, each device once. A project may
 * link several repositories and the presence names files relative to their repository, so the
 * sessions in the other ones stay out.
 */
export function peersOf(
  activities: ReadonlyMap<string, TeamActivity>,
  sessionId: number,
): PeerView[] {
  const peers = new Map<string, PeerView>();
  for (const activity of activities.values()) {
    const repo = activity.sessionRepos.find((entry) => entry.sessionId === sessionId)?.repo;
    if (repo === undefined) continue;
    for (const peer of activity.peers) {
      const sessions = peer.sessions.filter((session) => session.repo === repo);
      if (sessions.length === 0) continue;
      const device = `${peer.member}\n${peer.device}`;
      const known = peers.get(device);
      if (known === undefined) {
        peers.set(device, { ...peer, sessions });
        continue;
      }
      for (const session of sessions) {
        if (!known.sessions.some((other) => other.key === session.key)) {
          known.sessions.push(session);
        }
      }
    }
  }
  return [...peers.values()];
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

/** What a key on the tree does (WAI-ARIA tree view pattern); `null`: not the tree's key. */
export type TreeStep =
  | { kind: "focus"; path: string }
  | { kind: "expand"; path: string }
  | { kind: "collapse"; path: string }
  | { kind: "select"; path: string }
  | { kind: "none" };

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
      return { kind: "select", path: current.path };
    default:
      return null;
  }
}

/** The answer the backend gives while another listing of the same folder still runs. */
export function isBusyError(message: string): boolean {
  return message === "The folder is still being read";
}
