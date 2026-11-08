// Favorite and recently used working folders for the "Starts in" menu. Stored per machine in
// localStorage; invalid content yields empty lists.

const STORAGE_KEY = "hangar.folders";

/** Recent folders kept; favorites have no limit of their own. */
export const RECENT_LIMIT = 8;
/** Protects storage and the menu against a broken or hand-edited value. */
const FAVORITES_LIMIT = 50;
const MAX_PATH_LENGTH = 4096;

export interface FolderPrefs {
  /** In the order they were added. */
  favorites: readonly string[];
  /** Most recent first, at most RECENT_LIMIT. */
  recent: readonly string[];
}

export const EMPTY_FOLDERS: FolderPrefs = { favorites: [], recent: [] };

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

export function isFavorite(prefs: FolderPrefs, path: string): boolean {
  const key = folderKey(path);
  return prefs.favorites.some((entry) => folderKey(entry) === key);
}

export function toggleFavorite(prefs: FolderPrefs, path: string): FolderPrefs {
  if (isFavorite(prefs, path)) return { ...prefs, favorites: without(prefs.favorites, path) };
  if (!validPath(path)) return prefs;
  return { ...prefs, favorites: [...prefs.favorites, path].slice(-FAVORITES_LIMIT) };
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

export function parseFolderPrefs(value: unknown): FolderPrefs {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return EMPTY_FOLDERS;
  return {
    favorites: pathList(Reflect.get(value, "favorites"), FAVORITES_LIMIT),
    recent: pathList(Reflect.get(value, "recent"), RECENT_LIMIT),
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
