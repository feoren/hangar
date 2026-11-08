// Content of the folder menu: sections, labels that tell same-named folders apart, and paths
// shortened in the middle so the start (drive, user) and the end (project) stay readable.

import { folderKey, isFavorite, samePath, type FolderPrefs } from "./folders";

export const ELLIPSIS = "\u2026";

/** Characters of a shortened path in menus and the sidebar. */
export const PATH_DISPLAY_LENGTH = 38;

function separatorOf(path: string): string {
  return path.includes("\\") ? "\\" : "/";
}

function segments(path: string): string[] {
  return path.split(/[\\/]+/).filter((part) => part.length > 0);
}

/**
 * Shortens a path in the middle: the root and first folder stay, then "…", then as many trailing
 * folders as fit. `C:\Users\ada\Dropbox\Repository\Local\hangar` becomes `C:\Users\…\Local\hangar`.
 * The last folder always stays, even if it alone is longer than `max`.
 */
export function elideMiddle(path: string, max = PATH_DISPLAY_LENGTH): string {
  if (path.length <= max) return path;
  const separator = separatorOf(path);
  const parts = segments(path);
  if (parts.length <= 2) return path;
  const lead = /^[\\/]+/.exec(path)?.[0].replace(/[\\/]/g, separator) ?? "";
  // A drive keeps the folder after it ("C:\Users"), a POSIX root its first folder ("/home").
  const headCount = /^[a-z]:$/i.test(parts[0] ?? "") && parts.length > 3 ? 2 : 1;
  const head = lead + parts.slice(0, headCount).join(separator);
  const rest = parts.slice(headCount);
  const tail: string[] = [];
  for (let index = rest.length - 1; index >= 0; index -= 1) {
    const part = rest[index] ?? "";
    const candidate = [part, ...tail];
    const text = `${head}${separator}${ELLIPSIS}${separator}${candidate.join(separator)}`;
    if (tail.length > 0 && text.length > max) break;
    tail.unshift(part);
  }
  if (tail.length >= rest.length) return path;
  return `${head}${separator}${ELLIPSIS}${separator}${tail.join(separator)}`;
}

/** Folder that contains `path`, `null` for a root. */
export function parentFolder(path: string): string | null {
  const parts = segments(path);
  if (parts.length <= 1) return null;
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  if (cut <= 0) return trimmed.slice(0, 1);
  const parent = trimmed.slice(0, cut);
  // "C:" alone would mean the drive's current folder.
  return /^[a-z]:$/i.test(parent) ? `${parent}\\` : parent;
}

/**
 * Short names for a list of folders: the last folder, extended by parent folders where two
 * entries would read the same (`work\hangar` and `fork\hangar`). Same order as `paths`.
 */
export function distinctNames(paths: readonly string[]): string[] {
  const parts = paths.map(segments);
  const depth = paths.map(() => 1);
  const name = (index: number) => {
    const list = parts[index] ?? [];
    const take = Math.min(depth[index] ?? 1, list.length);
    const text = list.slice(list.length - take).join(separatorOf(paths[index] ?? ""));
    return text === "" ? (paths[index] ?? "") : text;
  };
  for (let round = 0; round < 32; round += 1) {
    const groups = new Map<string, number[]>();
    paths.forEach((_, index) => {
      const key = folderKey(name(index)).toLowerCase();
      groups.set(key, [...(groups.get(key) ?? []), index]);
    });
    let grew = false;
    for (const members of groups.values()) {
      if (members.length < 2) continue;
      for (const index of members) {
        if ((depth[index] ?? 1) < (parts[index]?.length ?? 0)) {
          depth[index] = (depth[index] ?? 1) + 1;
          grew = true;
        }
      }
    }
    if (!grew) break;
  }
  return paths.map((_, index) => name(index));
}

export type FolderSectionKind = "running" | "favorites" | "recent";

export interface FolderEntry {
  path: string;
  /** Folder name, with parents where needed to tell it apart. */
  label: string;
  /** Shortened parent folder, empty for a root. */
  detail: string;
  favorite: boolean;
  /** The current start target. */
  current: boolean;
}

export interface FolderSection {
  kind: FolderSectionKind;
  entries: FolderEntry[];
}

/**
 * Sections of the folder menu, each folder once: running projects first, then favorites, then
 * recent folders. Empty sections are left out.
 */
export function folderMenuSections(input: {
  running: readonly string[];
  prefs: FolderPrefs;
  target: string | null;
}): FolderSection[] {
  const seen = new Set<string>();
  const take = (paths: readonly string[]) =>
    paths.filter((path) => {
      const key = folderKey(path);
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  const lists: [FolderSectionKind, string[]][] = [
    ["running", take(input.running)],
    ["favorites", take(input.prefs.favorites)],
    ["recent", take(input.prefs.recent)],
  ];
  const all = lists.flatMap(([, paths]) => paths);
  const names = distinctNames(all);
  const labels = new Map(all.map((path, index) => [path, names[index] ?? path]));
  return lists
    .filter(([, paths]) => paths.length > 0)
    .map(([kind, paths]) => ({
      kind,
      entries: paths.map((path) => {
        const parent = parentFolder(path);
        return {
          path,
          label: labels.get(path) ?? path,
          detail: parent === null ? "" : elideMiddle(parent, PATH_DISPLAY_LENGTH - 8),
          favorite: isFavorite(input.prefs, path),
          current: input.target !== null && samePath(path, input.target),
        };
      }),
    }));
}
