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
import { currentPlatform, type Platform } from "./platform";

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
  | "reopenRecent"
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
  // The most recently opened session that is not open, again (O for open). Ctrl+Shift+T, the
  // browsers' key for this, starts a new session here already. Ctrl+O alone is
  // operate-and-get-next in Readline and stays with the shell.
  if (event.shiftKey && event.code === "KeyO") return "reopenRecent";
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
    action === "reopenRecent" ||
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

/** The key held for dictation, chosen in Settings > Dictation. */
export type DictationKey = "controlRight" | "f8" | "f9" | "f10" | "ctrlShiftSpace";

/** In the order the settings list them. */
export const DICTATION_KEYS: readonly DictationKey[] = [
  "controlRight",
  "f8",
  "f9",
  "f10",
  "ctrlShiftSpace",
];

/**
 * Right Ctrl: a single key, and shells and agent CLIs never see a modifier pressed alone. Ctrl+C
 * and friends typed with it keep working (`KeyHold`). Mac keyboards mostly lack a right control
 * key, so the Mac starts with the chord.
 */
export function defaultDictationKey(platform: Platform): DictationKey {
  return platform === "mac" ? "ctrlShiftSpace" : "controlRight";
}

export const DEFAULT_DICTATION_KEY: DictationKey = defaultDictationKey(currentPlatform());

export function isDictationKey(value: unknown): value is DictationKey {
  return DICTATION_KEYS.some((key) => key === value);
}

/**
 * Right Ctrl records only after it was held this long with no other key: a tap, or Ctrl+C typed
 * with the right Ctrl, never opens the microphone.
 */
export const CONTROL_HOLD_DELAY_MS = 300;

const FUNCTION_KEYS: Partial<Record<DictationKey, string>> = { f8: "F8", f9: "F9", f10: "F10" };

function noModifier(event: KeyInfo): boolean {
  return !event.ctrlKey && !event.shiftKey && !event.altKey && !event.metaKey;
}

/**
 * Dictation chord: Ctrl+Shift+Space. Space keeps both modifiers free of letters, and shells and
 * agent CLIs bind no Ctrl+Shift+Space (without Shift, Ctrl+Space is set-mark in Readline and the
 * IME switch on Windows, so it stays with them).
 */
export function isDictationChord(event: KeyInfo): boolean {
  return (
    event.ctrlKey && event.shiftKey && !event.altKey && !event.metaKey && event.code === "Space"
  );
}

/**
 * The key press that starts a hold of `key`. Right Ctrl goes by its position (`code`): AltGr on
 * Windows arrives as a left Ctrl plus right Alt, so it never matches, and Shift, Alt or Meta held
 * along make it some other shortcut. F8 to F10 count only without a modifier.
 */
export function startsDictation(key: DictationKey, event: KeyInfo): boolean {
  if (key === "ctrlShiftSpace") return isDictationChord(event);
  if (key === "controlRight") {
    return event.code === "ControlRight" && !event.shiftKey && !event.altKey && !event.metaKey;
  }
  return event.code === FUNCTION_KEYS[key] && noModifier(event);
}

/**
 * Whether the terminal leaves this event (keydown, keypress or keyup alike) to the app while
 * dictation is on with `key`, so the program never sees it. Right Ctrl is not taken: alone it
 * sends the program nothing, and with another key it is that program's shortcut.
 */
export function isDictationKeyEvent(key: DictationKey, event: KeyInfo): boolean {
  return key !== "controlRight" && startsDictation(key, event);
}

/**
 * Releasing the key ends the hold; for the chord, releasing any of its keys, so a lost keyup of
 * one never keeps it on.
 */
export function endsDictation(
  key: DictationKey,
  event: Pick<KeyboardEvent, "key" | "code">,
): boolean {
  if (key === "ctrlShiftSpace") {
    return event.code === "Space" || event.key === "Control" || event.key === "Shift";
  }
  if (key === "controlRight") return event.code === "ControlRight";
  return event.code === FUNCTION_KEYS[key];
}

type HoldKey = KeyInfo & Pick<KeyboardEvent, "repeat">;

export interface HoldHandlers {
  /** The hold began: open the microphone. */
  onStart: () => void;
  /** The key went up, or the window lost the focus: close it and write the text. */
  onEnd: () => void;
  /** Right Ctrl became part of a shortcut after `onStart`: close it and drop the text. */
  onDiscard: () => void;
}

/**
 * - `waiting`: right Ctrl is down, not yet for `CONTROL_HOLD_DELAY_MS`.
 * - `held`: `onStart` ran.
 * - `spoiled`: right Ctrl is still down, but another key or the mouse made it a shortcut. Nothing
 *   starts until it goes up.
 */
type HoldPhase =
  | { kind: "idle" }
  | { kind: "waiting"; timer: ReturnType<typeof setTimeout> }
  | { kind: "held" }
  | { kind: "spoiled" };

/**
 * Hold semantics for the dictation key: `onStart` once per hold (key repeats ignored), `onEnd`
 * once on its release or on `release` (window blur). Right Ctrl is a modifier, so it starts only
 * after `CONTROL_HOLD_DELAY_MS` without another key, and another key or a click while it is down
 * hands it back to that shortcut: before the start nothing happens, after it `onDiscard`. The
 * methods return whether the event belongs to the hold, so the caller can stop its default; right
 * Ctrl's own events never do.
 */
export class KeyHold {
  private phase: HoldPhase = { kind: "idle" };

  constructor(
    private readonly key: DictationKey,
    private readonly handlers: HoldHandlers,
  ) {}

  get active(): boolean {
    return this.phase.kind === "held";
  }

  /** Right Ctrl shares its key with shortcuts; the other keys belong to dictation alone. */
  private get modifier(): boolean {
    return this.key === "controlRight";
  }

  private own(event: HoldKey): boolean {
    return this.modifier ? event.code === "ControlRight" : startsDictation(this.key, event);
  }

  keydown(event: HoldKey): boolean {
    if (this.phase.kind === "spoiled" && this.own(event) && !event.repeat) {
      // A fresh press: the release of the spoiled one got lost.
      this.phase = { kind: "idle" };
    }
    if (this.phase.kind === "idle") {
      if (!startsDictation(this.key, event)) return false;
      if (!event.repeat) this.begin();
      return !this.modifier;
    }
    if (this.own(event)) return !this.modifier;
    if (this.modifier) this.interrupt();
    return false;
  }

  keyup(event: Pick<KeyboardEvent, "key" | "code">): boolean {
    if (this.phase.kind === "idle" || !endsDictation(this.key, event)) return false;
    this.finish();
    return !this.modifier;
  }

  /** A mouse button or the wheel: with right Ctrl down, that is Ctrl+click or Ctrl+wheel. */
  pointer(): void {
    if (this.modifier) this.interrupt();
  }

  /** The window lost the focus: its keyup would never arrive. */
  release(): void {
    this.finish();
  }

  private begin(): void {
    if (!this.modifier) {
      this.phase = { kind: "held" };
      this.handlers.onStart();
      return;
    }
    const timer = setTimeout(() => {
      if (this.phase.kind !== "waiting") return;
      this.phase = { kind: "held" };
      this.handlers.onStart();
    }, CONTROL_HOLD_DELAY_MS);
    this.phase = { kind: "waiting", timer };
  }

  private interrupt(): void {
    const phase = this.phase;
    if (phase.kind === "idle" || phase.kind === "spoiled") return;
    if (phase.kind === "waiting") clearTimeout(phase.timer);
    this.phase = { kind: "spoiled" };
    if (phase.kind === "held") this.handlers.onDiscard();
  }

  private finish(): void {
    const phase = this.phase;
    if (phase.kind === "waiting") clearTimeout(phase.timer);
    this.phase = { kind: "idle" };
    if (phase.kind === "held") this.handlers.onEnd();
  }
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
  reopenRecent: { mods: ["ctrl", "shift"], key: "O" },
  find: { mods: ["ctrl", "shift"], key: "F" },
  searchAll: { mods: ["ctrl", "shift"], key: "S" },
  overview: { mods: ["ctrl", "shift"], key: "M" },
  home: { mods: ["ctrl", "shift"], key: "H" },
  launcher: { mods: ["ctrl", "shift"], key: "P" },
};

/** Held, not pressed: shown as "Hold Ctrl+Shift+Space". */
export const DICTATION_SHORTCUT: Shortcut = { mods: ["ctrl", "shift"], key: "Space" };

/**
 * The dictation key's name. `inline` fits the help sentence ("Hold the right Ctrl key and
 * speak."); otherwise the name as the settings list it.
 */
export function dictationKeyName(key: DictationKey, t: Translate, inline = false): string {
  if (key === "controlRight") {
    return t(inline ? "dictation.keyRightCtrlInline" : "dictation.keyRightCtrl");
  }
  if (key === "ctrlShiftSpace") return formatShortcut(DICTATION_SHORTCUT, t);
  return FUNCTION_KEYS[key] ?? key;
}

/** Keys of the shortcut in display order, modifiers in the UI language. */
export function shortcutKeys(shortcut: Shortcut, t: Translate): string[] {
  const names: Record<Modifier, string> = { ctrl: t("key.ctrl"), shift: t("key.shift") };
  const key = shortcut.key === "Space" ? t("key.space") : shortcut.key;
  return [...shortcut.mods.map((mod) => names[mod]), key];
}

export function formatShortcut(shortcut: Shortcut, t: Translate): string {
  return shortcutKeys(shortcut, t).join("+");
}
