// Backend errors with a stable code: `[code] English text` (ManagerError in manager.rs,
// ArenaError in arena.rs). Known codes are shown in the window's own words; the English text is
// the fallback for codes this window does not know yet. Arena codes are translated by
// `arenaErrorText` (arena.ts).

import { t, type MessageKey } from "./i18n";

const CODE = /^\[([a-z_]+)\] /;

const MESSAGES: ReadonlyMap<string, MessageKey> = new Map<string, MessageKey>([
  ["bad_cwd", "terminal.missingFolder"],
  ["remote_cwd", "terminal.remoteFolder"],
]);

/** The leading `[code]` of a backend error and the English text after it; `null` without one. */
export function splitBackendError(message: string): { code: string; text: string } | null {
  const match = CODE.exec(message);
  if (match === null) return null;
  return { code: match[1] ?? "", text: message.slice(match[0].length) };
}

/** `message` as the user should read it: translated for a known code, else without the code. */
export function backendErrorText(message: string): string {
  const split = splitBackendError(message);
  if (split === null) return message;
  const key = MESSAGES.get(split.code);
  return key === undefined ? split.text : t(key);
}
