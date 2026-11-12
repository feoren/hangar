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
  | "quickNote"
  | "toggleChanges"
  | "closeSession"
  | "fontBigger"
  | "fontSmaller"
  | "fontReset"
  | "openSettings"
  | "nextWaiting"
  | "newSession"
  | "find"
  | "launcher"
  | "searchAll"
  | "overview"
  | "home";

type KeyInfo = Pick<KeyboardEvent, "key" | "code" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">;

export function matchShortcut(event: KeyInfo): AppAction | null {
  if (!event.ctrlKey || event.altKey || event.metaKey) return null;
  if (event.key === "Tab") return event.shiftKey ? "prevSession" : "nextSession";
  if (event.shiftKey && event.code === "KeyB") return "toggleSidebar";
  if (event.shiftKey && event.code === "KeyW") return "closeSession";
  // Quick note: Ctrl+Shift+N. Ctrl+N alone stays with shell and agent (e.g. history forward).
  if (event.shiftKey && event.code === "KeyN") return "quickNote";
  // Changes of the session's folder (D for diff). Ctrl+D alone is end of input in every shell.
  if (event.shiftKey && event.code === "KeyD") return "toggleChanges";
  // To the next session waiting for you (A for attention).
  if (event.shiftKey && event.code === "KeyA") return "nextWaiting";
  // New session with the most recently used program, like a new tab in Windows Terminal.
  if (event.shiftKey && event.code === "KeyT") return "newSession";
  // Search in the scrollback, as in Windows Terminal. Ctrl+F alone moves the cursor (Readline).
  if (event.shiftKey && event.code === "KeyF") return "find";
  // Quick access to sessions, programs, folders and snippets, as the command palette in VS Code
  // and Windows Terminal. Ctrl+P alone is history back in Readline and PSReadLine.
  if (event.shiftKey && event.code === "KeyP") return "launcher";
  // Search the output of every session (S for sessions). Ctrl+S alone is forward history search
  // in Readline and PSReadLine.
  if (event.shiftKey && event.code === "KeyS") return "searchAll";
  // Overview of all sessions as tiles. Ctrl+Tab already switches to the next session, and Ctrl+M
  // alone is Enter in every terminal.
  if (event.shiftKey && event.code === "KeyM") return "overview";
  // The home view (H for home): the sessions step aside and keep running. Ctrl+H alone is
  // Backspace in every terminal.
  if (event.shiftKey && event.code === "KeyH") return "home";
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
 * Alt+Up and Alt+Down on a focused session row in the sidebar move that session one place. The row
 * handles this itself instead of going through `matchShortcut`: in the terminal, Alt+arrow
 * belongs to shells and agent CLIs (history, word motion), so the keys only count while a row has
 * the focus. Without Alt, the arrows stay with the list.
 */
export function rowMoveStep(event: KeyInfo): -1 | 1 | null {
  if (!event.altKey || event.ctrlKey || event.shiftKey || event.metaKey) return null;
  if (event.key === "ArrowUp") return -1;
  if (event.key === "ArrowDown") return 1;
  return null;
}

/** Places F6 moves the focus between while the notes are open, in this order. */
export type FocusZone = "terminal" | "list" | "editor";

/**
 * F6 and Shift+F6 move the focus between terminal, notes list and editor, as F6 moves between
 * panes in Windows programs. Only while the notes panel is open: otherwise F6 stays with the
 * program in the terminal (a few TUIs such as Midnight Commander use it).
 */
export function focusCycleStep(event: KeyInfo): 1 | -1 | null {
  if (event.key !== "F6" || event.ctrlKey || event.altKey || event.metaKey) return null;
  return event.shiftKey ? -1 : 1;
}

/** The next zone from `current` among those available; unknown or none: the first one. */
export function nextFocusZone(
  zones: readonly FocusZone[],
  current: FocusZone | null,
  step: 1 | -1,
): FocusZone | null {
  if (zones.length === 0) return null;
  const index = current === null ? -1 : zones.indexOf(current);
  if (index < 0) return step === 1 ? (zones[0] ?? null) : (zones[zones.length - 1] ?? null);
  return zones[(index + step + zones.length) % zones.length] ?? null;
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
    action === "quickNote" ||
    action === "toggleChanges" ||
    action === "openSettings" ||
    action === "find" ||
    action === "launcher" ||
    action === "searchAll" ||
    action === "overview" ||
    action === "home"
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
  quickNote: { mods: ["ctrl", "shift"], key: "N" },
  toggleChanges: { mods: ["ctrl", "shift"], key: "D" },
  closeSession: { mods: ["ctrl", "shift"], key: "W" },
  fontBigger: { mods: ["ctrl"], key: "Plus" },
  fontSmaller: { mods: ["ctrl"], key: "Minus" },
  fontReset: { mods: ["ctrl"], key: "0" },
  openSettings: { mods: ["ctrl"], key: "," },
  nextWaiting: { mods: ["ctrl", "shift"], key: "A" },
  newSession: { mods: ["ctrl", "shift"], key: "T" },
  find: { mods: ["ctrl", "shift"], key: "F" },
  searchAll: { mods: ["ctrl", "shift"], key: "S" },
  overview: { mods: ["ctrl", "shift"], key: "M" },
  home: { mods: ["ctrl", "shift"], key: "H" },
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
