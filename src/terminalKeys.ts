// What a key press inside the terminal does, before xterm turns it into input for the process.
// Pure, so the rules for copy and paste are testable without a terminal.

import { focusCycleStep, isDictationChord, matchShortcut } from "./shortcuts";

type KeyInfo = Pick<
  KeyboardEvent,
  "type" | "key" | "code" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey" | "isComposing"
>;

/**
 * What an agent CLI reads as "new line, do not submit yet": ESC followed by CR, the same bytes
 * Alt+Enter sends, which agent CLIs such as Claude Code take as a line break in their prompt.
 */
export const AGENT_NEWLINE = "\x1b\r";

/**
 * - `app`: an app shortcut, F6 while `focusCycle` (the notes are open), or Ctrl+Shift+Space while
 *   `dictation` is on; xterm ignores the key, the window listener handles it.
 * - `copy`: copy the selection; `clearSelection` for Ctrl+C (Ctrl+Shift+C keeps it, like Windows
 *   Terminal). The default action is suppressed.
 * - `paste`: xterm ignores the key, but the browser's default action stays. It fires the native
 *   paste event on xterm's textarea, where the terminal sanitizes the text and pastes it
 *   (bracketed paste), or without text the path of the clipboard's image (clipboardPaste.ts).
 *   Reading the clipboard's text from script instead fails in WebView2.
 * - `newline`: Shift+Enter in an agent session. `send` is `AGENT_NEWLINE` on keydown and `null` for
 *   the keypress and keyup of the same key; xterm ignores all three, so it never sends its own CR
 *   (which would submit the prompt) and each press inserts exactly one new line.
 * - `deleteSelection`: Backspace or Delete with a selection in an agent session. The terminal tries
 *   to delete the selection from the prompt (terminalClick.ts) and hands the key back to xterm
 *   when the selection is not part of the typed text.
 * - `terminal`: xterm handles the key as usual (input for the process).
 */
export type TerminalKeyAction =
  | { kind: "app" }
  | { kind: "copy"; clearSelection: boolean }
  | { kind: "paste" }
  | { kind: "newline"; send: string | null }
  | { kind: "deleteSelection" }
  | { kind: "terminal" };

/**
 * Ctrl+C copies only with a selection; without one it goes to the process (interrupt).
 * `agent`: an agent CLI runs in the session, so Shift+Enter starts a new line in its prompt. A
 * plain shell gets Shift+Enter like Enter, as in other terminals, and Backspace with a selection
 * as a plain Backspace.
 */
export function terminalKeyAction(
  event: KeyInfo,
  hasSelection: boolean,
  focusCycle = false,
  agent = false,
  dictation = false,
): TerminalKeyAction {
  if (matchShortcut(event)) return { kind: "app" };
  // Held for dictation (keydown, keypress and keyup alike): the program never sees the chord.
  if (dictation && isDictationChord(event)) return { kind: "app" };
  if (agent && isShiftEnter(event)) {
    return { kind: "newline", send: event.type === "keydown" ? AGENT_NEWLINE : null };
  }
  if (agent && hasSelection && event.type === "keydown" && isPlainErase(event)) {
    return { kind: "deleteSelection" };
  }
  // F6 moves the focus to the notes while they are open; the process must not get it as well.
  if (focusCycle && event.type === "keydown" && focusCycleStep(event) !== null) {
    return { kind: "app" };
  }
  if (event.type !== "keydown" || !event.ctrlKey || event.altKey || event.metaKey) {
    return { kind: "terminal" };
  }
  if (event.code === "KeyC" && (event.shiftKey || hasSelection)) {
    return { kind: "copy", clearSelection: !event.shiftKey };
  }
  if (event.code === "KeyV") return { kind: "paste" };
  return { kind: "terminal" };
}

type TailKey = Pick<
  KeyboardEvent,
  "type" | "key" | "code" | "repeat" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey"
>;

/**
 * The tail of a dictation hold. Released Shift (or Ctrl) before Space, the held Space keeps
 * repeating as Ctrl+Space, which xterm sends as NUL, or as a plain space. `armed` runs from the
 * chord's keydown to the release of Space; meanwhile repeated Space events are swallowed. A fresh
 * Space press (no repeat) disarms, so a lost keyup never keeps it armed.
 */
export function dictationTail(
  armed: boolean,
  event: TailKey,
  dictation: boolean,
): { armed: boolean; swallow: boolean } {
  if (!dictation) return { armed: false, swallow: false };
  if (event.code !== "Space") return { armed, swallow: false };
  if (event.type === "keyup") return { armed: false, swallow: false };
  if (isDictationChord(event)) return { armed: true, swallow: false };
  if (armed && event.repeat) return { armed: true, swallow: true };
  return { armed: false, swallow: false };
}

/** Shift+Enter alone; never while an IME composes text, where Enter confirms the composition. */
function isShiftEnter(event: KeyInfo): boolean {
  return (
    event.key === "Enter" &&
    event.shiftKey &&
    !event.ctrlKey &&
    !event.altKey &&
    !event.metaKey &&
    !event.isComposing
  );
}

/** Backspace or Delete without a modifier: Ctrl+Backspace deletes a word in many CLIs. */
function isPlainErase(event: KeyInfo): boolean {
  return (
    (event.key === "Backspace" || event.key === "Delete") &&
    !event.shiftKey &&
    !event.ctrlKey &&
    !event.altKey &&
    !event.metaKey &&
    !event.isComposing
  );
}
