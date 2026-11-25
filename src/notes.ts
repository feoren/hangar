// Notes per working folder (ADR 0004): validate backend responses, name scopes, find titles for
// new notes, sort and search. Pure logic without the backend.

import type { NoteFolder } from "./api";
import type { MessageKey } from "./i18n";

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
  /**
   * A shared team note sealed with an older key than the team's newest: whoever wrote it held
   * that key, possibly after leaving the team (ADR 0006, "Old keys after a removal").
   */
  oldKey: boolean;
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
      oldKey: field(entry, "oldKey") === true,
    });
  }
  return result.sort((a, b) => a.order - b.order);
}

/** Comparison key of a scope: on Windows, paths are case insensitive, as in the backend. */
export function folderId(folder: NoteFolder): string {
  // Shared notes of a project never share a key with its private notes.
  if (folder.shared === true) return `shared:${folderId({ path: folder.path })}`;
  // No path is empty; that way "General" collides with no folder, not even with "/".
  if (folder.path === null) return "";
  const slashes = folder.path.replace(/[\\/]+/g, "/");
  const unified = slashes.length > 1 ? slashes.replace(/\/$/, "") : slashes;
  return /^[a-z]:/i.test(unified) || unified.startsWith("//") ? unified.toLowerCase() : unified;
}

export function sameFolder(a: NoteFolder, b: NoteFolder): boolean {
  return folderId(a) === folderId(b);
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
 * Is the title still the one Hangar gave a new note ("Note 3" in any language)? Then it may
 * follow the first line; a title the user chose stays.
 */
export function isUntitled(title: string, untitledBases: readonly string[]): boolean {
  const lower = title.trim().toLocaleLowerCase();
  return untitledBases.some((base) => {
    const prefix = `${base.toLocaleLowerCase()} `;
    return lower.startsWith(prefix) && /^\d+$/.test(lower.slice(prefix.length));
  });
}

/**
 * `known` with the answers in `updates` (note key, old key or not): keys listed there follow
 * the answer, every other key stays as it was, so a list of one scope does not forget the mark
 * of a note opened from another. The same set comes back when nothing changed.
 */
export function withOldKeys(
  known: ReadonlySet<string>,
  updates: readonly (readonly [string, boolean])[],
): ReadonlySet<string> {
  const changed = updates.some(([key, old]) => known.has(key) !== old);
  if (!changed) return known;
  const next = new Set(known);
  for (const [key, old] of updates) {
    if (old) next.add(key);
    else next.delete(key);
  }
  return next;
}

/** A note together with its scope: the list "All" and the search mix scopes. */
/** What the window is about to do with the open note's text. */
export type OldKeyAction =
  | { kind: "send"; sessionId: number | null }
  /** Out of the shared notes into this computer's, where the old-key mark cannot follow. */
  | { kind: "move"; to: NoteFolder };

/**
 * The question to ask before `action` for a note with an old team key (`old`), `null` to go
 * ahead at once. Sending puts the text in front of an agent; moving it to this computer drops the
 * mark, so the next "Send to" and an agent hand-over would no longer ask (ADR 0006, "Old keys
 * after a removal"). Moving into the shared notes has nothing to protect: they get the newest
 * key and the person's own write.
 */
export function oldKeyQuestion(
  old: boolean,
  action: OldKeyAction,
): { text: MessageKey; confirm: MessageKey } | null {
  if (!old) return null;
  if (action.kind === "send")
    return { text: "sharedNotes.oldKeySend", confirm: "paste.sendAnyway" };
  if (action.to.shared === true) return null;
  return { text: "sharedNotes.oldKeyMove", confirm: "sharedNotes.moveAnyway" };
}

export interface NoteRef {
  folder: NoteFolder;
  id: string;
  title: string;
  updatedAt: number | null;
}

export interface SearchHit extends NoteRef {
  folderKey: string;
  excerpt: string;
}

export interface TrashItem {
  id: string;
  title: string;
  deletedAt: number | null;
}

/** Same note (same scope, same ID). */
export function sameNote(
  a: { folder: NoteFolder; id: string },
  b: { folder: NoteFolder; id: string },
) {
  return a.id === b.id && sameFolder(a.folder, b.folder);
}

/** Key of a note for maps and React lists. */
export function noteKey(note: { folder: NoteFolder; id: string }): string {
  return `${folderId(note.folder)}
${note.id}`;
}

/** Most recent change first; notes without a time last, then by title. */
export function byLastChange<T extends { updatedAt: number | null; title: string }>(
  notes: readonly T[],
): T[] {
  return [...notes].sort((a, b) => {
    const diff = (b.updatedAt ?? -1) - (a.updatedAt ?? -1);
    return diff !== 0 ? diff : a.title.localeCompare(b.title);
  });
}

export function parseSearchHits(value: unknown): SearchHit[] {
  if (!Array.isArray(value)) return [];
  const result: SearchHit[] = [];
  for (const item of value) {
    const entry = record(item);
    if (!entry) continue;
    const folderKey = field(entry, "folderKey");
    const path = field(entry, "path");
    const id = field(entry, "id");
    const title = field(entry, "title");
    const excerpt = field(entry, "excerpt");
    if (typeof folderKey !== "string" || folderKey === "") continue;
    if (path !== null && typeof path !== "string") continue;
    if (typeof id !== "string" || id === "" || typeof title !== "string") continue;
    result.push({
      folderKey,
      folder: field(entry, "shared") === true ? { path, shared: true } : { path },
      id,
      title,
      excerpt: typeof excerpt === "string" ? excerpt : "",
      updatedAt: optionalTime(field(entry, "updatedAt")),
    });
  }
  return result;
}

export function parseTrash(value: unknown): TrashItem[] {
  if (!Array.isArray(value)) return [];
  const result: TrashItem[] = [];
  for (const item of value) {
    const entry = record(item);
    if (!entry) continue;
    const id = field(entry, "id");
    const title = field(entry, "title");
    if (typeof id !== "string" || id === "" || typeof title !== "string") continue;
    result.push({ id, title, deletedAt: optionalTime(field(entry, "deletedAt")) });
  }
  return result;
}
