// "Private | Shared" above the notes list. Private lists the notes on this computer (with the scope
// chips), Shared the shared notes of one project (ADR 0006). The switch exists only where a
// project can have shared notes; elsewhere the panel shows the private notes as before. Pure logic;
// the panel calls the backend.

import type { NoteFolder } from "./api";
import { isShared, showsGroups, type SharedStatus } from "./sharedNotes";

export type NotesSide = "private" | "shared";

export const DEFAULT_NOTES_SIDE: NotesSide = "private";

const STORAGE_KEY = "hangar.notesSide";

/** A stored value; anything unknown is the default. */
export function parseNotesSide(value: unknown): NotesSide {
  return value === "shared" ? "shared" : DEFAULT_NOTES_SIDE;
}

export function loadNotesSide(): NotesSide {
  try {
    return parseNotesSide(localStorage.getItem(STORAGE_KEY));
  } catch {
    return DEFAULT_NOTES_SIDE;
  }
}

export function saveNotesSide(side: NotesSide): void {
  try {
    localStorage.setItem(STORAGE_KEY, side);
  } catch {
    // Without storage the panel simply opens on the private notes next time.
  }
}

/**
 * Whether the panel offers the switch: only for a project whose shared notes are known to exist
 * as a feature (any state but "unavailable"). While the state is loading it stays hidden.
 */
export function sideSwitchShown(hasProject: boolean, status: SharedStatus | null): boolean {
  return hasProject && showsGroups(status);
}

/**
 * The side the list shows. A remembered "shared" survives loading (no flash of private notes);
 * without a project or without shared notes the panel is private.
 */
export function effectiveSide(
  chosen: NotesSide,
  hasProject: boolean,
  status: SharedStatus | null,
): NotesSide {
  if (chosen === "private" || !hasProject) return "private";
  if (status === null) return "shared";
  return showsGroups(status) ? "shared" : "private";
}

export function sideOf(folder: NoteFolder): NotesSide {
  return isShared(folder) ? "shared" : "private";
}

/**
 * Scopes the panel lists: the private ones of the scope and, once it can be read, the project's
 * shared one. Both sides load together so that each can show its count.
 */
export function listedFolders(
  privateFolders: readonly NoteFolder[],
  shared: NoteFolder | null,
): NoteFolder[] {
  return shared === null ? [...privateFolders] : [...privateFolders, shared];
}

/** Scopes of one side, e.g. for the trash. */
export function sideFolders(
  side: NotesSide,
  privateFolders: readonly NoteFolder[],
  shared: NoteFolder | null,
): NoteFolder[] {
  if (side === "private") return [...privateFolders];
  return shared === null ? [] : [shared];
}

export function onSide<T extends { folder: NoteFolder }>(
  notes: readonly T[],
  side: NotesSide,
): T[] {
  return notes.filter((note) => sideOf(note.folder) === side);
}

export function sideCounts(notes: readonly { folder: NoteFolder }[]): Record<NotesSide, number> {
  const counts = { private: 0, shared: 0 };
  for (const note of notes) counts[sideOf(note.folder)] += 1;
  return counts;
}

/**
 * Where a new note goes: into the shared notes on the shared side while they are unlocked
 * (`shared` set), else where the private side puts it. `null`: nowhere, the side offers no new
 * note (shared notes locked or off).
 */
export function newNoteTarget(
  side: NotesSide,
  privateTarget: NoteFolder,
  shared: NoteFolder | null,
): NoteFolder | null {
  return side === "shared" ? shared : privateTarget;
}
