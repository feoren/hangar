// What a key press inside the terminal does, before xterm turns it into input for the process.
// Pure, so the rules for copy and paste are testable without a terminal.

import { matchShortcut } from "./shortcuts";

type KeyInfo = Pick<
  KeyboardEvent,
  "type" | "key" | "code" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey"
>;

/**
 * - `app`: an app shortcut; xterm ignores the key, the window listener handles it.
 * - `copy`: copy the selection; `clearSelection` for Ctrl+C (Ctrl+Shift+C keeps it, like Windows
 *   Terminal). The default action is suppressed.
 * - `paste`: xterm ignores the key, but the browser's default action stays. It fires the native
 *   paste event on xterm's textarea, where the terminal sanitizes the text and pastes it
 *   (bracketed paste). Reading the clipboard from script instead fails in WebView2.
 * - `terminal`: xterm handles the key as usual (input for the process).
 */
export type TerminalKeyAction =
  | { kind: "app" }
  | { kind: "copy"; clearSelection: boolean }
  | { kind: "paste" }
  | { kind: "terminal" };

/** Ctrl+C copies only with a selection; without one it goes to the process (interrupt). */
export function terminalKeyAction(event: KeyInfo, hasSelection: boolean): TerminalKeyAction {
  if (matchShortcut(event)) return { kind: "app" };
  if (event.type !== "keydown" || !event.ctrlKey || event.altKey || event.metaKey) {
    return { kind: "terminal" };
  }
  if (event.code === "KeyC" && (event.shiftKey || hasSelection)) {
    return { kind: "copy", clearSelection: !event.shiftKey };
  }
  if (event.code === "KeyV") return { kind: "paste" };
  return { kind: "terminal" };
}
