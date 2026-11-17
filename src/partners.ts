// Partner projects (ADR 0012): projects that may send each other cross-project messages, each
// with a short name. The backend keeps and checks the list (partners.rs); this file reads its
// answer, suggests short names and finds a project's partners for the window. Pure.

import { folderKey, folderName } from "./folders";
import { hasInvisible } from "./invisible";

/** `^[a-z0-9-]{1,32}$`, as the backend checks it (`partners.rs` `valid_name`). */
export const MAX_NAME_LENGTH = 32;
const NAME = /^[a-z0-9-]{1,32}$/;

export interface NamedProject {
  name: string;
  /** The repository's main folder, or the plain folder, as the project card names it. */
  folder: string;
}

export interface PartnerBook {
  projects: readonly NamedProject[];
  /** Each pair once; it holds in both directions. */
  pairs: readonly (readonly [string, string])[];
}

export const EMPTY_BOOK: PartnerBook = { projects: [], pairs: [] };

export function validName(name: string): boolean {
  return NAME.test(name);
}

/**
 * A short name from a folder's name: lower case, every run of other characters one dash, at
 * most MAX_NAME_LENGTH; empty when nothing usable is left (a folder named only "日本").
 */
export function suggestName(folder: string): string {
  return folderName(folder)
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, MAX_NAME_LENGTH)
    .replace(/-+$/, "");
}

function namedProject(value: unknown): NamedProject | null {
  if (typeof value !== "object" || value === null) return null;
  const name: unknown = Reflect.get(value, "name");
  const folder: unknown = Reflect.get(value, "folder");
  if (typeof name !== "string" || typeof folder !== "string") return null;
  if (!validName(name) || folder.trim() === "" || hasInvisible(folder)) return null;
  return { name, folder };
}

/** The backend's answer; anything malformed reads as no partners at all. */
export function parsePartnerBook(value: unknown): PartnerBook {
  if (typeof value !== "object" || value === null) return EMPTY_BOOK;
  const projects: unknown = Reflect.get(value, "projects");
  const pairs: unknown = Reflect.get(value, "pairs");
  if (!Array.isArray(projects) || !Array.isArray(pairs)) return EMPTY_BOOK;
  const parsed = projects.map(namedProject);
  if (parsed.some((project) => project === null)) return EMPTY_BOOK;
  const known = parsed.filter((project): project is NamedProject => project !== null);
  const names = new Set(known.map((project) => project.name));
  const parsedPairs: [string, string][] = [];
  for (const pair of pairs) {
    if (!Array.isArray(pair) || pair.length !== 2) return EMPTY_BOOK;
    const a: unknown = pair[0];
    const b: unknown = pair[1];
    if (typeof a !== "string" || typeof b !== "string" || a === b) return EMPTY_BOOK;
    if (!names.has(a) || !names.has(b)) return EMPTY_BOOK;
    parsedPairs.push([a, b]);
  }
  return { projects: known, pairs: parsedPairs };
}

function within(key: string, folder: string): boolean {
  if (key === folder) return true;
  const separator =
    folder.endsWith("\\") || folder.endsWith("/") ? "" : folder.includes("\\") ? "\\" : "/";
  return key.startsWith(folder + separator);
}

/** The named project at exactly this folder (a project card's root). */
export function namedAt(book: PartnerBook, folder: string): NamedProject | undefined {
  const key = folderKey(folder);
  return book.projects.find((project) => folderKey(project.folder) === key);
}

/**
 * The named project a session belongs to: its folder holds the session's repository (a worktree
 * names it) or its working folder. The backend decides for good; this only picks what the menu
 * offers.
 */
export function projectOfSession(
  book: PartnerBook,
  session: { cwd: string; worktree?: { repoRoot: string } | undefined },
): NamedProject | undefined {
  const keys = [session.cwd, session.worktree?.repoRoot]
    .filter((path): path is string => path !== undefined && path.trim() !== "")
    .map(folderKey);
  return book.projects.find((project) => {
    const folder = folderKey(project.folder);
    return keys.some((key) => within(key, folder));
  });
}

/** The partners of the project named `name`, in both directions, in pair order. */
export function partnersOf(book: PartnerBook, name: string): NamedProject[] {
  const byName = new Map(book.projects.map((project) => [project.name, project]));
  const found: NamedProject[] = [];
  for (const [a, b] of book.pairs) {
    const other = a === name ? b : b === name ? a : null;
    const project = other === null ? undefined : byName.get(other);
    if (project !== undefined) found.push(project);
  }
  return found;
}
