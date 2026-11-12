// Favorite and recently used working folders for the "Starts in" menu, plus a color per project
// folder. Stored per machine in localStorage; invalid content yields empty lists.

import { MARKER_COLORS, type MarkerColorId } from "./marker";

const STORAGE_KEY = "hangar.folders";

/** Recent folders kept; favorites have no limit of their own. */
export const RECENT_LIMIT = 8;
/** Protects storage and the menu against a broken or hand-edited value. */
const FAVORITES_LIMIT = 50;
const MAX_PATH_LENGTH = 4096;
/** Folder colors kept; a hand-edited value cannot grow the storage without bound. */
const COLORS_LIMIT = 100;

/** A project folder's color: one of the session marker palette's tones (marker.ts). */
export type FolderColor = MarkerColorId;

export function isFolderColor(value: unknown): value is FolderColor {
  return MARKER_COLORS.some((entry) => entry.id === value);
}

/** The design token that draws a folder color (`--folder-<id>` in styles.css). */
export function folderColorCss(color: FolderColor): string {
  return `var(--folder-${color})`;
}

export interface FolderPrefs {
  /** In the order the person arranged them; new ones go last. */
  favorites: readonly string[];
  /** Most recent first, at most RECENT_LIMIT. */
  recent: readonly string[];
  /** Color per project folder, by `folderKey`; folders without one are missing. */
  colors: Readonly<Record<string, FolderColor>>;
}

export const EMPTY_FOLDERS: FolderPrefs = { favorites: [], recent: [], colors: {} };

/**
 * Windows paths (drive letter or UNC) compare without case, all of them without trailing
 * separators. A root keeps its separator: `C:\` and `/` stay roots.
 */
export function folderKey(path: string): string {
  const windows = /^[a-z]:[\\/]?/i.test(path) || /^[\\/]{2}[^\\/]/.test(path);
  let trimmed = path.replace(/[\\/]+$/, "");
  if (trimmed === "") trimmed = path.slice(0, 1);
  if (/^[a-z]:$/i.test(trimmed)) trimmed += "\\";
  if (!windows) return trimmed;
  return trimmed.replace(/\//g, "\\").toLowerCase();
}

export function samePath(a: string, b: string): boolean {
  return folderKey(a) === folderKey(b);
}

function without(list: readonly string[], path: string): string[] {
  const key = folderKey(path);
  return list.filter((entry) => folderKey(entry) !== key);
}

/** A session started here: the folder moves to the front of the recent list. */
export function recordRecent(prefs: FolderPrefs, path: string): FolderPrefs {
  if (!validPath(path)) return prefs;
  if (prefs.recent.length > 0 && prefs.recent[0] === path) return prefs;
  return { ...prefs, recent: [path, ...without(prefs.recent, path)].slice(0, RECENT_LIMIT) };
}

export function isFavorite(prefs: Pick<FolderPrefs, "favorites">, path: string): boolean {
  const key = folderKey(path);
  return prefs.favorites.some((entry) => folderKey(entry) === key);
}

export function toggleFavorite(prefs: FolderPrefs, path: string): FolderPrefs {
  if (isFavorite(prefs, path)) return { ...prefs, favorites: without(prefs.favorites, path) };
  if (!validPath(path)) return prefs;
  return { ...prefs, favorites: [...prefs.favorites, path].slice(-FAVORITES_LIMIT) };
}

/** Takes a folder out of the menu: off the favorites and the recent list, and its color goes. */
export function forgetFolder(prefs: FolderPrefs, path: string): FolderPrefs {
  return {
    favorites: without(prefs.favorites, path),
    recent: without(prefs.recent, path),
    colors: withoutColor(prefs.colors, folderKey(path)),
  };
}

/**
 * Moves the favorite `path` right before (with `after`: right after) the favorite `target`.
 * The same prefs when either is no favorite or nothing changes.
 */
export function moveFavorite(
  prefs: FolderPrefs,
  path: string,
  target: string,
  after: boolean,
): FolderPrefs {
  const from = prefs.favorites.findIndex((entry) => samePath(entry, path));
  const moved = prefs.favorites[from];
  if (moved === undefined || samePath(path, target)) return prefs;
  const rest = prefs.favorites.filter((_, index) => index !== from);
  const at = rest.findIndex((entry) => samePath(entry, target));
  if (at < 0) return prefs;
  const cut = after ? at + 1 : at;
  const favorites = [...rest.slice(0, cut), moved, ...rest.slice(cut)];
  return favorites.every((entry, index) => entry === prefs.favorites[index])
    ? prefs
    : { ...prefs, favorites };
}

/** The folder's color, `null` without one. */
export function folderColor(prefs: FolderPrefs, path: string): FolderColor | null {
  const key = folderKey(path);
  return Object.hasOwn(prefs.colors, key) ? (prefs.colors[key] ?? null) : null;
}

/** Gives a folder a color; `null` takes it away. The newest choice is kept last. */
export function setFolderColor(
  prefs: FolderPrefs,
  path: string,
  color: FolderColor | null,
): FolderPrefs {
  if (!validPath(path) || folderColor(prefs, path) === color) return prefs;
  const key = folderKey(path);
  const rest = Object.entries(withoutColor(prefs.colors, key));
  if (color === null) return { ...prefs, colors: Object.fromEntries(rest) };
  const entries: [string, FolderColor][] = [...rest, [key, color]];
  return { ...prefs, colors: Object.fromEntries(entries.slice(-COLORS_LIMIT)) };
}

function withoutColor(
  colors: Readonly<Record<string, FolderColor>>,
  key: string,
): Record<string, FolderColor> {
  return Object.fromEntries(Object.entries(colors).filter(([other]) => other !== key));
}

/** Recent folders that are not favorites anyway, so no folder appears twice in the menu. */
export function recentOnly(prefs: FolderPrefs): string[] {
  return prefs.recent.filter((path) => !isFavorite(prefs, path));
}

function validPath(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.trim() !== "" &&
    value.length <= MAX_PATH_LENGTH &&
    // eslint-disable-next-line no-control-regex
    !/[\u0000-\u001f]/.test(value)
  );
}

function pathList(value: unknown, limit: number): string[] {
  if (!Array.isArray(value)) return [];
  const result: string[] = [];
  for (const entry of value) {
    if (!validPath(entry) || result.some((path) => samePath(path, entry))) continue;
    result.push(entry);
    if (result.length === limit) break;
  }
  return result;
}

/** Stored colors: known tones on valid folders only, keyed again; anything else is dropped. */
function colorMap(value: unknown): Record<string, FolderColor> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return {};
  const entries: [string, FolderColor][] = [];
  for (const [path, color] of Object.entries(value)) {
    if (!validPath(path) || !isFolderColor(color)) continue;
    entries.push([folderKey(path), color]);
  }
  return Object.fromEntries(entries.slice(-COLORS_LIMIT));
}

export function parseFolderPrefs(value: unknown): FolderPrefs {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return EMPTY_FOLDERS;
  return {
    favorites: pathList(Reflect.get(value, "favorites"), FAVORITES_LIMIT),
    recent: pathList(Reflect.get(value, "recent"), RECENT_LIMIT),
    // Versions before folder colors stored none.
    colors: colorMap(Reflect.get(value, "colors")),
  };
}

export function loadFolderPrefs(): FolderPrefs {
  try {
    const text = localStorage.getItem(STORAGE_KEY);
    return text === null ? EMPTY_FOLDERS : parseFolderPrefs(JSON.parse(text));
  } catch {
    return EMPTY_FOLDERS;
  }
}

export function saveFolderPrefs(prefs: FolderPrefs): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // Without storage, the menu starts empty next time.
  }
}

/** Last folder of a path for the menu label; roots and drives stay as they are. */
export function folderName(path: string): string {
  const parts = path.split(/[\\/]+/).filter((part) => part.length > 0);
  const last = parts.at(-1);
  if (last === undefined || parts.length === 1) return path;
  return last;
}
