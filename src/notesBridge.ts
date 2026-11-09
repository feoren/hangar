// Between the main window and the detached notes window (contract:
// docs/wiki/pages/syntheses/notes.md). The notes window may only listen: what it asks of the
// main window goes through the backend (`notes_to_main`, event `notes-message`). The main window
// tells the notes window what it needs through the event `notes-command` and keeps the session
// context in localStorage, which both windows share (same origin). Everything read here arrives
// as `unknown` and is checked before use.

import { MAX_NOTE_BYTES } from "./notes";

/** Event from the backend to the main window; payload checked by `notes_to_main`. */
export const NOTES_MESSAGE = "notes-message";
/** Event from the main window to the notes window. */
export const NOTES_COMMAND = "notes-command";
/** Label of the notes window (src-tauri/src/notes_window.rs). */
export const NOTES_WINDOW_LABEL = "notes";

export type NotesMessage =
  | { kind: "sendToTerminal"; text: string; sessionId: number | null }
  | { kind: "notesChanged"; folderKey: string }
  | { kind: "showMain" };

export type NotesCommand =
  /** Ctrl+Shift+N in the main window: start a new quick note. */
  | { kind: "quickNote" }
  /** "Add selection to note" in the terminal. */
  | { kind: "capture"; text: string; source: string }
  /** Notes changed in the main window: reload the list. */
  | { kind: "refresh" }
  /** Save pending input now (the app is quitting). */
  | { kind: "flush" }
  /** "Bring back": save and close the window. */
  | { kind: "close" };

function record(value: unknown): object | null {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? value : null;
}

function field(value: object, name: string): unknown {
  return name in value ? Reflect.get(value, name) : undefined;
}

function isFolderKey(value: unknown): value is string {
  return typeof value === "string" && (value === "general" || /^[0-9a-f]{32}$/i.test(value));
}

/** Tab and line breaks only: an ESC could end bracketed paste early. Mirrors the backend. */
export function isSendable(text: string): boolean {
  if (text === "" || new TextEncoder().encode(text).length > MAX_NOTE_BYTES) return false;
  for (const char of text) {
    const code = char.codePointAt(0) ?? 0;
    const control = code < 0x20 || (code >= 0x7f && code < 0xa0);
    if (control && char !== "\t" && char !== "\n" && char !== "\r") return false;
  }
  return true;
}

/** Removes control characters the terminal must not receive from a note (tab and breaks stay). */
export function sendableText(text: string): string {
  let result = "";
  for (const char of text) {
    const code = char.codePointAt(0) ?? 0;
    const control = code < 0x20 || (code >= 0x7f && code < 0xa0);
    if (!control || char === "\t" || char === "\n" || char === "\r") result += char;
  }
  return result;
}

/** `notes-message` payload in the main window; `null` for anything unexpected. */
export function parseNotesMessage(payload: unknown): NotesMessage | null {
  const value = record(payload);
  if (!value) return null;
  switch (field(value, "kind")) {
    case "sendToTerminal": {
      const text = field(value, "text");
      const id = field(value, "sessionId");
      if (typeof text !== "string" || !isSendable(text)) return null;
      if (id !== undefined && id !== null && !(typeof id === "number" && Number.isInteger(id))) {
        return null;
      }
      return { kind: "sendToTerminal", text, sessionId: typeof id === "number" ? id : null };
    }
    case "notesChanged": {
      const folderKey = field(value, "folderKey");
      return isFolderKey(folderKey) ? { kind: "notesChanged", folderKey } : null;
    }
    case "showMain":
      return { kind: "showMain" };
    default:
      return null;
  }
}

/** `notes-command` payload in the notes window; `null` for anything unexpected. */
export function parseNotesCommand(payload: unknown): NotesCommand | null {
  const value = record(payload);
  if (!value) return null;
  const kind = field(value, "kind");
  switch (kind) {
    case "quickNote":
    case "refresh":
    case "flush":
    case "close":
      return { kind };
    case "capture": {
      const text = field(value, "text");
      const source = field(value, "source");
      if (typeof text !== "string" || text === "" || typeof source !== "string") return null;
      return { kind, text, source };
    }
    default:
      return null;
  }
}

/** What the notes window knows about the sessions: for "This project" and "Send to …". */
export interface NotesContext {
  /** Working folder of the active session. */
  sessionFolder: string | null;
  activeId: number | null;
  sessions: readonly { id: number; title: string }[];
}

export const EMPTY_CONTEXT: NotesContext = { sessionFolder: null, activeId: null, sessions: [] };
export const CONTEXT_KEY = "hangar.notesContext";

export function parseNotesContext(text: string | null): NotesContext {
  if (text === null) return EMPTY_CONTEXT;
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return EMPTY_CONTEXT;
  }
  const value = record(raw);
  if (!value) return EMPTY_CONTEXT;
  const folder = field(value, "sessionFolder");
  const active = field(value, "activeId");
  const list = field(value, "sessions");
  const sessions: { id: number; title: string }[] = [];
  if (Array.isArray(list)) {
    for (const item of list) {
      const entry = record(item);
      if (!entry) continue;
      const id = field(entry, "id");
      const title = field(entry, "title");
      if (typeof id === "number" && Number.isInteger(id) && typeof title === "string") {
        sessions.push({ id, title });
      }
    }
  }
  return {
    sessionFolder: typeof folder === "string" && folder !== "" ? folder : null,
    activeId: typeof active === "number" && Number.isInteger(active) ? active : null,
    sessions,
  };
}

/** Detached and kept on top, remembered per machine. */
export interface NotesWindowPrefs {
  detached: boolean;
  onTop: boolean;
}

export const DEFAULT_WINDOW_PREFS: NotesWindowPrefs = { detached: false, onTop: false };
export const WINDOW_PREFS_KEY = "hangar.notesWindow";

export function parseWindowPrefs(text: string | null): NotesWindowPrefs {
  if (text === null) return DEFAULT_WINDOW_PREFS;
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return DEFAULT_WINDOW_PREFS;
  }
  const value = record(raw);
  if (!value) return DEFAULT_WINDOW_PREFS;
  const detached = field(value, "detached");
  const onTop = field(value, "onTop");
  return {
    detached: typeof detached === "boolean" ? detached : DEFAULT_WINDOW_PREFS.detached,
    onTop: typeof onTop === "boolean" ? onTop : DEFAULT_WINDOW_PREFS.onTop,
  };
}

export function loadWindowPrefs(): NotesWindowPrefs {
  try {
    return parseWindowPrefs(localStorage.getItem(WINDOW_PREFS_KEY));
  } catch {
    return DEFAULT_WINDOW_PREFS;
  }
}

export function saveWindowPrefs(prefs: NotesWindowPrefs): void {
  try {
    localStorage.setItem(WINDOW_PREFS_KEY, JSON.stringify(prefs));
  } catch {
    // Without storage the notes start docked next time.
  }
}

export function loadContext(): NotesContext {
  try {
    return parseNotesContext(localStorage.getItem(CONTEXT_KEY));
  } catch {
    return EMPTY_CONTEXT;
  }
}

export function saveContext(context: NotesContext): void {
  try {
    localStorage.setItem(CONTEXT_KEY, JSON.stringify(context));
  } catch {
    // The notes window then offers General and no session to send to.
  }
}
