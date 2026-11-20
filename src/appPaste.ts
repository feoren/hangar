// Text the app sends into a session on the user's behalf ("Send to" from a note, a snippet).
// Unlike Ctrl+V, the user did not just see the text arrive in the clipboard, and the button says
// it adds no Enter. That only holds when the program has turned on bracketed paste (DECSET
// 2004): without it, xterm turns every line break into a carriage return and each line but the
// last runs at once. So the app asks first in that case, also when the text holds characters
// that make it read differently than it runs (src/invisible.ts), and when it seems to hold a
// secret key or token (src/secretScan.ts, ADR 0015).

import { hasInvisible } from "./invisible";
import { scanSecrets, type ShownKind } from "./secretScan";
import { sanitizePaste } from "./termSafety";

export interface PasteWarning {
  /** No bracketed paste and a line break: lines run as they arrive. */
  runsLines: boolean;
  /** Bidi controls or zero-width characters. */
  hidden: boolean;
  /** Kinds of secret the text seems to hold; left out when none. */
  secrets?: readonly ShownKind[];
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
  const secrets = scanSecrets(clean);
  if (!runsLines && !hidden && secrets.length === 0) return null;
  return secrets.length === 0 ? { runsLines, hidden } : { runsLines, hidden, secrets };
}

/**
 * Only the secret part of `warning`, for a paste whose line and character questions are switched
 * off in Settings: a key in the clipboard is asked about all the same.
 */
export function secretWarning(warning: PasteWarning | null): PasteWarning | null {
  const secrets = warning?.secrets ?? [];
  return secrets.length === 0 ? null : { runsLines: false, hidden: false, secrets };
}

/** Whether the user's answer to `asked` also covers what `now` warns about. */
function covers(asked: PasteWarning, now: PasteWarning): boolean {
  const secrets = (asked.secrets?.length ?? 0) > 0 || (now.secrets?.length ?? 0) === 0;
  return (asked.runsLines || !now.runsLines) && (asked.hidden || !now.hidden) && secrets;
}

/** How often the question is asked again when the program changed its mode meanwhile. */
const MAX_QUESTIONS = 2;

/**
 * Asks `confirm` about `warning` (what `pasteWarning` found for `text`); resolves to whether the
 * text may go in. The program can turn bracketed paste off while the question is open, so
 * `bracketedPaste` is read again after the answer: a yes given to a text whose lines would not
 * have run does not cover lines that now would, the question comes again with the new warning
 * (once; after that the text is dropped). Text without a warning needs no call, so it can go in
 * synchronously. `check` finds the warning anew; a caller that asks only about secrets passes one
 * that keeps to them, so a yes is not followed by a question the person switched off.
 */
export async function askBeforePaste(
  text: string,
  warning: PasteWarning,
  bracketedPaste: () => boolean,
  confirm: (warning: PasteWarning) => Promise<boolean>,
  check: (text: string, bracketedPaste: boolean) => PasteWarning | null = pasteWarning,
): Promise<boolean> {
  let asked = warning;
  for (let questions = 1; questions <= MAX_QUESTIONS; questions += 1) {
    if (!(await confirm(asked))) return false;
    const now = check(text, bracketedPaste());
    if (now === null || covers(asked, now)) return true;
    asked = now;
  }
  return false;
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
  const bracketed = () => target.bracketedPaste();
  if (warning !== null && !(await askBeforePaste(text, warning, bracketed, confirm))) return false;
  target.pasteText(text);
  return true;
}
