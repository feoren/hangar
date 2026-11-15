// Backend errors with a stable code: `[code] English text` (ManagerError in manager.rs,
// ArenaError in arena.rs), and for errors that name a path or a reason
// `[code] English text\n<detail>` (SnapshotError in git_snapshots.rs). Known codes are shown in
// the window's own words; the English text is the fallback for codes this window does not know
// yet. Arena codes are translated by `arenaErrorText` (arena.ts), snapshot codes by
// `timelineErrorText` (timeline.ts).

import { t, type MessageKey } from "./i18n";

const CODE = /^\[([a-z_]+)\] /;

const MESSAGES: ReadonlyMap<string, MessageKey> = new Map<string, MessageKey>([
  ["bad_cwd", "terminal.missingFolder"],
  ["remote_cwd", "terminal.remoteFolder"],
]);

export interface BackendError {
  /** The stable code, `null` for an error without one. */
  code: string | null;
  /** The English text, without code and detail; the whole message when it has no code. */
  text: string;
  /** Everything after the first line break (a path, git's reason); `null` without one. */
  detail: string | null;
}

/** Takes a backend error apart; the English text of a coded error never holds a line break. */
export function splitBackendError(message: string): BackendError {
  const match = CODE.exec(message);
  if (match === null) return { code: null, text: message, detail: null };
  const rest = message.slice(match[0].length);
  const end = rest.indexOf("\n");
  return {
    code: match[1] ?? null,
    text: end < 0 ? rest : rest.slice(0, end),
    detail: end < 0 ? null : rest.slice(end + 1),
  };
}

/**
 * `message` as the user should read it: translated for a known code, else the English text
 * alone (a detail of an unknown code is left out).
 */
export function backendErrorText(message: string): string {
  const { code, text } = splitBackendError(message);
  if (code === null) return message;
  const key = MESSAGES.get(code);
  return key === undefined ? text : t(key);
}
