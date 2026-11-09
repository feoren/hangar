// Text the app sends into a session on the user's behalf ("Send to" from a note, a snippet).
// Unlike Ctrl+V, the user did not just see the text arrive in the clipboard, and the button says
// it adds no Enter. That only holds when the program has turned on bracketed paste (DECSET
// 2004): without it, xterm turns every line break into a carriage return and each line but the
// last runs at once. So the app asks first in that case, and also when the text holds characters
// that make it read differently than it runs (src/invisible.ts).

import { hasInvisible } from "./invisible";
import { sanitizePaste } from "./termSafety";

export interface PasteWarning {
  /** No bracketed paste and a line break: lines run as they arrive. */
  runsLines: boolean;
  /** Bidi controls or zero-width characters. */
  hidden: boolean;
}

export interface PasteTarget {
  /** Whether the program in the session has turned on bracketed paste right now. */
  bracketedPaste: () => boolean;
  pasteText: (text: string) => void;
}

/** Breaks and tabs are expected in pasted text; `hasInvisible` would count them too. */
const LAYOUT = /[\r\n\t]/g;
/** U+200D between two pictographs joins an emoji (family, profession), it hides nothing. */
const EMOJI_JOINER = /(?<=\p{Extended_Pictographic}\uFE0F?)\u200d(?=\p{Extended_Pictographic})/gu;

/** What to tell the user before pasting `text`, or `null` when it can go in as it is. */
export function pasteWarning(text: string, bracketedPaste: boolean): PasteWarning | null {
  const clean = sanitizePaste(text);
  const runsLines = !bracketedPaste && /[\r\n]/.test(clean);
  const hidden = hasInvisible(clean.replace(LAYOUT, "").replace(EMOJI_JOINER, ""));
  return runsLines || hidden ? { runsLines, hidden } : null;
}

/**
 * Pastes `text` into `target`, after `confirm` agreed if there is something to warn about.
 * Resolves to whether anything was pasted.
 */
export async function pasteFromApp(
  target: PasteTarget,
  text: string,
  confirm: (warning: PasteWarning) => Promise<boolean>,
): Promise<boolean> {
  if (sanitizePaste(text) === "") return false;
  const warning = pasteWarning(text, target.bracketedPaste());
  if (warning !== null && !(await confirm(warning))) return false;
  target.pasteText(text);
  return true;
}
