// The sub-agent definitions and their figures, shared by Settings, the sidebar, quick access and
// the sub-agent tree. Read from the backend when the window asks (start, the project folders
// change, Settings opens, after a change); never stored in the window.

import { useSyncExternalStore } from "react";

import { api, errorText } from "./api";
import {
  EMPTY_LIBRARY,
  parseFigures,
  parseLibrary,
  type FigureView,
  type Library,
} from "./subAgentLibrary";

export interface LibraryState {
  library: Library;
  figures: ReadonlyMap<string, FigureView>;
  /** The newest refresh failed: why. The last good listing stays. */
  error: string | null;
}

let state: LibraryState = { library: EMPTY_LIBRARY, figures: new Map(), error: null };
let folders: readonly string[] = [];
/** Answers of older refreshes are dropped. */
let generation = 0;
const listeners = new Set<() => void>();

function publish(next: LibraryState): void {
  state = next;
  for (const listener of listeners) listener();
}

/** Reads the listing (global and the project folders) and the figures again. */
export async function refreshLibrary(): Promise<void> {
  generation += 1;
  const mine = generation;
  try {
    const [library, figures] = await Promise.all([
      api.subAgentLibrary(folders),
      api.subAgentFigures(),
    ]);
    if (mine !== generation) return;
    publish({
      library: parseLibrary(library) ?? EMPTY_LIBRARY,
      figures: parseFigures(figures),
      error: null,
    });
  } catch (e) {
    if (mine === generation) publish({ ...state, error: errorText(e) });
  }
}

/**
 * Brings Hangar's copies of the user's definitions in other programs up to date for the current
 * folders (the backend replaces only files of the user's names, after a backup), then reads the
 * listing again. A failed sync still reads the listing.
 */
export async function syncLibrary(): Promise<void> {
  let failed: string | null = null;
  try {
    await api.syncSubAgents(folders);
  } catch (e) {
    failed = errorText(e);
  }
  await refreshLibrary();
  // The listing still shows what is there; why the copies were not brought up to date stays
  // visible until the next run.
  if (failed !== null) publish({ ...state, error: failed });
}

/** The project folders whose `.claude/agents` count; a change syncs and reads the listing again. */
export function setLibraryFolders(next: readonly string[]): void {
  const unique = [...new Set(next)].slice(0, 32);
  const same =
    unique.length === folders.length && unique.every((folder, i) => folder === folders[i]);
  // The first call reads the global definitions even without a project folder.
  if (same && generation > 0) return;
  folders = unique;
  void syncLibrary();
}

/** A backend answer with every figure (after choosing one). */
export function applyFigures(value: unknown): void {
  publish({ ...state, figures: parseFigures(value) });
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function useSubAgentLibrary(): LibraryState {
  return useSyncExternalStore(
    subscribe,
    () => state,
    () => state,
  );
}

/** For tests: back to the empty start. */
export function resetLibraryStore(): void {
  generation += 1;
  folders = [];
  state = { library: EMPTY_LIBRARY, figures: new Map(), error: null };
}
