// App shortcuts. They are intercepted before xterm sends them to the process; hence only
// combinations that agent CLIs and shells do not need. Ctrl+B, for example, belongs to Claude Code
// (background tasks) and tmux, so Ctrl+Shift+B collapses the sidebar.
//
// Collisions with Shift: a terminal without an extended keyboard protocol sends the same control
// character for Ctrl+Shift+letter as for Ctrl+letter. Anyone pressing Ctrl+Shift+A (instead of
// Ctrl+A, start of line in Readline and PSReadLine) or Ctrl+Shift+T (instead of Ctrl+T, transpose
// characters or fzf file search) reaches Hangar with it, not the shell; without Shift everything
// stays with the shell. Claude Code, Codex, Gemini CLI and Aider bind no Ctrl+Shift
// combination. Ctrl+Shift+T also opens something new (a tab) in Windows Terminal.

import type { Translate } from "./i18n";

export type AppAction =
  | "nextSession"
  | "prevSession"
  | "toggleSidebar"
  | "toggleNotes"
  | "closeSession"
  | "fontBigger"
  | "fontSmaller"
  | "fontReset"
  | "openSettings"
  | "nextWaiting"
  | "newSession"
  | "find"
  | "launcher";

type KeyInfo = Pick<KeyboardEvent, "key" | "code" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">;

export function matchShortcut(event: KeyInfo): AppAction | null {
  if (!event.ctrlKey || event.altKey || event.metaKey) return null;
  if (event.key === "Tab") return event.shiftKey ? "prevSession" : "nextSession";
  if (event.shiftKey && event.code === "KeyB") return "toggleSidebar";
  if (event.shiftKey && event.code === "KeyW") return "closeSession";
  // Notes: Ctrl+Shift+N. Ctrl+N alone stays with shell and agent (e.g. history forward).
  if (event.shiftKey && event.code === "KeyN") return "toggleNotes";
  // To the next session waiting for you (A for attention).
  if (event.shiftKey && event.code === "KeyA") return "nextWaiting";
  // New session with the most recently used program, like a new tab in Windows Terminal.
  if (event.shiftKey && event.code === "KeyT") return "newSession";
  // Search in the scrollback, as in Windows Terminal. Ctrl+F alone moves the cursor (Readline).
  if (event.shiftKey && event.code === "KeyF") return "find";
  // Quick access to sessions, programs, folders and snippets, as the command palette in VS Code
  // and Windows Terminal. Ctrl+P alone is history back in Readline and PSReadLine.
  if (event.shiftKey && event.code === "KeyP") return "launcher";
  // By character instead of key position: on a German keyboard "+" sits elsewhere than on US.
  if (event.key === "+" || event.key === "=" || event.code === "NumpadAdd") return "fontBigger";
  if (!event.shiftKey && (event.key === "-" || event.code === "NumpadSubtract")) {
    return "fontSmaller";
  }
  if (!event.shiftKey && (event.code === "Digit0" || event.code === "Numpad0")) return "fontReset";
  // Ctrl+Comma as in VS Code and Windows Terminal; shells and agent CLIs do not use it.
  if (!event.shiftKey && (event.key === "," || event.code === "Comma")) return "openSettings";
  return null;
}

/**
 * Keys that make the WebView reload the page (F5, Ctrl+F5, Ctrl+R, Ctrl+Shift+R). A reload
 * disconnects the UI from all running sessions; the app therefore intercepts these keys, but only
 * outside the terminal. Inside the terminal they belong to the program (Ctrl+R is the shell's
 * reverse search), and xterm itself prevents the reload there.
 */
export function isReloadKey(event: KeyInfo): boolean {
  if (event.altKey || event.metaKey) return false;
  if (event.key === "F5") return true;
  return event.ctrlKey && event.code === "KeyR";
}

/**
 * Held key: the browser repeats keydown. For "New session" that would mean one session per
 * repeat; switching and font size, on the other hand, may repeat.
 */
export function ignoresRepeat(action: AppAction): boolean {
  return (
    action === "newSession" ||
    action === "closeSession" ||
    action === "toggleSidebar" ||
    action === "toggleNotes" ||
    action === "openSettings" ||
    action === "find" ||
    action === "launcher"
  );
}

export type Modifier = "ctrl" | "shift";

export interface Shortcut {
  mods: readonly Modifier[];
  key: string;
}

/** Language neutral; `formatShortcut` turns it into "Strg+Umschalt+B" or "Ctrl+Shift+B". */
export const SHORTCUT_LABELS: Record<AppAction, Shortcut> = {
  nextSession: { mods: ["ctrl"], key: "Tab" },
  prevSession: { mods: ["ctrl", "shift"], key: "Tab" },
  toggleSidebar: { mods: ["ctrl", "shift"], key: "B" },
  toggleNotes: { mods: ["ctrl", "shift"], key: "N" },
  closeSession: { mods: ["ctrl", "shift"], key: "W" },
  fontBigger: { mods: ["ctrl"], key: "Plus" },
  fontSmaller: { mods: ["ctrl"], key: "Minus" },
  fontReset: { mods: ["ctrl"], key: "0" },
  openSettings: { mods: ["ctrl"], key: "," },
  nextWaiting: { mods: ["ctrl", "shift"], key: "A" },
  newSession: { mods: ["ctrl", "shift"], key: "T" },
  find: { mods: ["ctrl", "shift"], key: "F" },
  launcher: { mods: ["ctrl", "shift"], key: "P" },
};

/** Keys of the shortcut in display order, modifiers in the UI language. */
export function shortcutKeys(shortcut: Shortcut, t: Translate): string[] {
  const names: Record<Modifier, string> = { ctrl: t("key.ctrl"), shift: t("key.shift") };
  return [...shortcut.mods.map((mod) => names[mod]), shortcut.key];
}

export function formatShortcut(shortcut: Shortcut, t: Translate): string {
  return shortcutKeys(shortcut, t).join("+");
}
