// The file a session's `{{file}}` variable means: the one last opened in the viewer or selected in
// the Files tab, per session, relative to its folder. Kept outside React: the tree reports it on
// every selection, and only sending a snippet reads it.

const files = new Map<number, string>();

/** A file of `sessionId` was selected in the tree or opened in the viewer. */
export function rememberFile(sessionId: number, path: string): void {
  if (path.trim() === "") return;
  files.set(sessionId, path);
}

/** The session's last file, `null` when none was chosen yet. */
export function fileOf(sessionId: number): string | null {
  return files.get(sessionId) ?? null;
}

/** The session is gone: its file goes too. */
export function forgetFile(sessionId: number): void {
  files.delete(sessionId);
}
