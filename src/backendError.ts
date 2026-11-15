// Backend errors with a stable code: `[code] English text` (ManagerError in manager.rs). Known
// codes are shown in the window's own words; the English text is the fallback for codes this
// window does not know yet.

import { t, type MessageKey } from "./i18n";

const CODE = /^\[([a-z_]+)\] /;

const MESSAGES: ReadonlyMap<string, MessageKey> = new Map<string, MessageKey>([
  ["bad_cwd", "terminal.missingFolder"],
  ["remote_cwd", "terminal.remoteFolder"],
]);

/** `message` as the user should read it: translated for a known code, else without the code. */
export function backendErrorText(message: string): string {
  const match = CODE.exec(message);
  if (match === null) return message;
  const key = MESSAGES.get(match[1] ?? "");
  return key === undefined ? message.slice(match[0].length) : t(key);
}
