// Notes per working folder (ADR 0004): validate backend responses, name scopes, find titles for
// new notes, reorder tabs. Pure logic without the backend.

import type { NoteFolder } from "./api";
import { shortPath } from "./format";

export interface NoteFolderInfo {
  folderKey: string;
  /** `null`: the General area. */
  path: string | null;
  noteCount: number;
  updatedAt: number | null;
}

export interface NoteInfo {
  id: string;
  title: string;
  updatedAt: number | null;
  order: number;
}

export const GENERAL_FOLDER: NoteFolder = { path: null };

/** The backend's size limit for one note (UTF-8). */
export const MAX_NOTE_BYTES = 1024 * 1024;
export const MAX_TITLE_LENGTH = 200;

function record(value: unknown): object | null {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? value : null;
}

function field(value: object, name: string): unknown {
  return name in value ? Reflect.get(value, name) : undefined;
}

function optionalTime(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

export function parseNoteFolders(value: unknown): NoteFolderInfo[] {
  if (!Array.isArray(value)) return [];
  const result: NoteFolderInfo[] = [];
  for (const item of value) {
    const entry = record(item);
    if (!entry) continue;
    const folderKey = field(entry, "folderKey");
    const path = field(entry, "path");
    const count = field(entry, "noteCount");
    if (typeof folderKey !== "string" || folderKey === "") continue;
    if (path !== null && typeof path !== "string") continue;
    if (typeof count !== "number" || !Number.isInteger(count) || count < 0) continue;
    result.push({
      folderKey,
      path,
      noteCount: count,
      updatedAt: optionalTime(field(entry, "updatedAt")),
    });
  }
  return result;
}

export function parseNotes(value: unknown): NoteInfo[] {
  if (!Array.isArray(value)) return [];
  const result: NoteInfo[] = [];
  for (const item of value) {
    const entry = record(item);
    if (!entry) continue;
    const id = field(entry, "id");
    const title = field(entry, "title");
    const order = field(entry, "order");
    if (typeof id !== "string" || id === "" || typeof title !== "string") continue;
    if (result.some((note) => note.id === id)) continue;
    result.push({
      id,
      title,
      updatedAt: optionalTime(field(entry, "updatedAt")),
      order: typeof order === "number" && Number.isFinite(order) ? order : result.length,
    });
  }
  return result.sort((a, b) => a.order - b.order);
}

/** Comparison key of a scope: on Windows, paths are case insensitive, as in the backend. */
export function folderId(folder: NoteFolder): string {
  // No path is empty; that way "General" collides with no folder, not even with "/".
  if (folder.path === null) return "";
  const slashes = folder.path.replace(/[\\/]+/g, "/");
  const unified = slashes.length > 1 ? slashes.replace(/\/$/, "") : slashes;
  return /^[a-z]:/i.test(unified) || unified.startsWith("//") ? unified.toLowerCase() : unified;
}

export function sameFolder(a: NoteFolder, b: NoteFolder): boolean {
  return folderId(a) === folderId(b);
}

/** Short name for tabs and sidebar: the last two folders, "General" without a folder. */
export function folderLabel(folder: NoteFolder, general: string): string {
  return folder.path === null ? general : shortPath(folder.path);
}

/** "Note 1", "Note 2" …: the smallest free number, so titles do not repeat. */
export function untitledTitle(base: string, existing: readonly string[]): string {
  const taken = new Set(existing.map((title) => title.trim().toLowerCase()));
  for (let number = 1; ; number += 1) {
    const title = `${base} ${String(number)}`;
    if (!taken.has(title.toLowerCase())) return title;
  }
}

/** Valid title by the backend's rules, or `null`. */
export function cleanTitle(text: string): string | null {
  const title = text.trim();
  if (title.length === 0 || title.length > MAX_TITLE_LENGTH) return null;
  // The backend rejects control characters (line breaks included).
  for (const char of title) {
    const code = char.codePointAt(0) ?? 0;
    if (code < 0x20 || code === 0x7f) return null;
  }
  return title;
}

/** Moves `id` to position `target` (0 to length - 1). Unknown IDs change nothing. */
export function moveNote(ids: readonly string[], id: string, target: number): string[] {
  const index = ids.indexOf(id);
  if (index < 0) return [...ids];
  const rest = ids.filter((other) => other !== id);
  const position = Math.min(Math.max(Math.round(target), 0), rest.length);
  return [...rest.slice(0, position), id, ...rest.slice(position)];
}

/** Which tab becomes active after deleting: the right neighbor, otherwise the left one. */
export function neighbourAfterDelete(ids: readonly string[], id: string): string | null {
  const index = ids.indexOf(id);
  if (index < 0) return ids[0] ?? null;
  return ids[index + 1] ?? ids[index - 1] ?? null;
}

/** Longest title Hangar derives from the text itself; the tab truncates anyway. */
export const AUTO_TITLE_LENGTH = 60;

/**
 * Title from the first line with content: without leading Markdown characters (heading, list,
 * quote, task) and without emphasis, truncated to AUTO_TITLE_LENGTH characters. `null` if the
 * text has no such line yet.
 */
export function titleFromText(text: string): string | null {
  for (const raw of text.split(/\r?\n/)) {
    const line = raw
      .replace(/^\s*(?:#{1,6}\s+|>\s*|[-*+]\s+(?:\[[ xX]\]\s+)?|\d+[.)]\s+)*/, "")
      .replace(/[*_`~]+/g, "")
      .replace(/\s+/g, " ")
      .trim();
    if (line === "" || /^[-=#>]+$/.test(line)) continue;
    const chars = Array.from(line);
    const short =
      chars.length > AUTO_TITLE_LENGTH
        ? `${chars
            .slice(0, AUTO_TITLE_LENGTH - 1)
            .join("")
            .trimEnd()}…`
        : line;
    return cleanTitle(short);
  }
  return null;
}

/**
 * Does the title still come from Hangar? Yes if it is named like a new note ("Note 3" in any
 * language) or matches exactly what would be derived from the previous text. Then it follows
 * the first line; a title the user chose stays.
 */
export function isAutoTitle(
  title: string,
  untitledBases: readonly string[],
  previousText: string,
): boolean {
  const trimmed = title.trim();
  const lower = trimmed.toLocaleLowerCase();
  for (const base of untitledBases) {
    const prefix = `${base.toLocaleLowerCase()} `;
    if (lower.startsWith(prefix) && /^\d+$/.test(lower.slice(prefix.length))) return true;
  }
  const derived = titleFromText(previousText);
  return derived !== null && derived === trimmed;
}
