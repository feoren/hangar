// Which notes the panel lists: the chips "This project", "All", "General", or one other project
// (opened from quick access or a search hit). Pure logic; the panel loads the scopes it names.

import type { NoteFolder } from "./api";
import { folderId, GENERAL_FOLDER, sameFolder, type NoteFolderInfo } from "./notes";

export type NotesScope =
  | { kind: "project" }
  | { kind: "all" }
  | { kind: "general" }
  | { kind: "folder"; folder: NoteFolder };

export const DEFAULT_SCOPE: NotesScope = { kind: "project" };

/** "This project" needs a session; without one, the panel shows General. */
export function effectiveScope(scope: NotesScope, sessionFolder: NoteFolder | null): NotesScope {
  if (scope.kind === "project" && sessionFolder === null) return { kind: "general" };
  if (scope.kind === "folder") {
    if (scope.folder.path === null) return { kind: "general" };
    if (sessionFolder !== null && sameFolder(scope.folder, sessionFolder)) {
      return { kind: "project" };
    }
  }
  return scope;
}

/**
 * Scopes whose notes the list shows. "All": General, every scope with notes and the active
 * session's folder, each once.
 */
export function scopeFolders(
  scope: NotesScope,
  sessionFolder: NoteFolder | null,
  folders: readonly NoteFolderInfo[],
): NoteFolder[] {
  const shown = effectiveScope(scope, sessionFolder);
  switch (shown.kind) {
    case "project":
      return sessionFolder === null ? [GENERAL_FOLDER] : [sessionFolder];
    case "general":
      return [GENERAL_FOLDER];
    case "folder":
      return [shown.folder];
    case "all": {
      const result: NoteFolder[] = [];
      const seen = new Set<string>();
      const add = (folder: NoteFolder) => {
        const id = folderId(folder);
        if (seen.has(id)) return;
        seen.add(id);
        result.push(folder);
      };
      add(GENERAL_FOLDER);
      for (const info of folders) if (info.noteCount > 0) add({ path: info.path });
      if (sessionFolder !== null) add(sessionFolder);
      return result;
    }
  }
}

/** Where a new note goes: the scope shown; for "All" the active project, else General. */
export function newNoteFolder(scope: NotesScope, sessionFolder: NoteFolder | null): NoteFolder {
  const shown = effectiveScope(scope, sessionFolder);
  switch (shown.kind) {
    case "project":
    case "all":
      return sessionFolder ?? GENERAL_FOLDER;
    case "general":
      return GENERAL_FOLDER;
    case "folder":
      return shown.folder;
  }
}

/** The scope that shows `folder` (quick access "Notes in …"). */
export function scopeForFolder(folder: NoteFolder, sessionFolder: NoteFolder | null): NotesScope {
  return effectiveScope({ kind: "folder", folder }, sessionFolder);
}

export function sameScope(a: NotesScope, b: NotesScope): boolean {
  if (a.kind === "folder" && b.kind === "folder") return sameFolder(a.folder, b.folder);
  return a.kind === b.kind;
}

/** Does the list mix scopes, so each row should name its project? */
export function mixesScopes(scope: NotesScope, sessionFolder: NoteFolder | null): boolean {
  return effectiveScope(scope, sessionFolder).kind === "all";
}

/** The backend's key of a scope, from `list_note_folders`; `null` while it is not listed yet. */
export function folderKeyOf(folder: NoteFolder, folders: readonly NoteFolderInfo[]): string | null {
  if (folder.path === null) return "general";
  return folders.find((info) => sameFolder({ path: info.path }, folder))?.folderKey ?? null;
}
