// Between the main window and the detached side panel window (ADR 0007, contract:
// docs/wiki/pages/syntheses/notes.md). The main window stays the single source of truth: it pushes
// the active session's context as event `panel-context` and commands as event `panel-command`
// (`emitTo("panel", …)`). The panel window may only listen; what it asks of the main window goes
// through the backend (`panel_to_main`, event `panel-message`). Everything read here arrives as
// `unknown` and is checked before use.

import { EMPTY_CLAIMS, parseClaimsSnapshot, type ClaimsSnapshot } from "./claims";
import type { SideTab } from "./layout";
import { MAX_NOTE_BYTES } from "./notes";
import { EMPTY_ACTIVITY, parseTeamActivity, type TeamActivity } from "./team";

/** Event from the backend to the main window; payload checked by `panel_to_main`. */
export const PANEL_MESSAGE = "panel-message";
/** Event from the main window to the panel window: something to do. */
export const PANEL_COMMAND = "panel-command";
/** Event from the main window to the panel window: what the tabs show. */
export const PANEL_CONTEXT = "panel-context";
/** Label of the panel window (src-tauri/src/panel_window.rs). */
export const PANEL_WINDOW_LABEL = "panel";

const SIDE_TABS: readonly SideTab[] = ["notes", "changes", "files"];
/** Longer attention states are not ours; the value only has to change when it changes. */
const MAX_TRIGGER = 64;

export type PanelMessage =
  | { kind: "sendToTerminal"; text: string; sessionId: number | null }
  | { kind: "notesChanged"; folderKey: string }
  | { kind: "showMain" }
  | { kind: "showSession"; sessionId: number }
  | { kind: "ready" };

export type PanelCommand =
  /** Ctrl+Shift+N in the main window: start a new quick note. */
  | { kind: "quickNote" }
  /** "Add selection to note" in the terminal. */
  | { kind: "capture"; text: string; source: string }
  /** Notes changed in the main window: reload the list. */
  | { kind: "refresh" }
  /** Save pending input now (the app is quitting). */
  | { kind: "flush" }
  /** A toggle in the main window asked for this tab. */
  | { kind: "show"; tab: SideTab };

function record(value: unknown): object | null {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? value : null;
}

function field(value: object, name: string): unknown {
  return name in value ? Reflect.get(value, name) : undefined;
}

function isId(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isFolderKey(value: unknown): value is string {
  return typeof value === "string" && (value === "general" || /^[0-9a-f]{32}$/i.test(value));
}

export function isSideTab(value: unknown): value is SideTab {
  return SIDE_TABS.some((tab) => tab === value);
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

/** `panel-message` payload in the main window; `null` for anything unexpected. */
export function parsePanelMessage(payload: unknown): PanelMessage | null {
  const value = record(payload);
  if (!value) return null;
  switch (field(value, "kind")) {
    case "sendToTerminal": {
      const text = field(value, "text");
      const id = field(value, "sessionId");
      if (typeof text !== "string" || !isSendable(text)) return null;
      if (id !== undefined && id !== null && !isId(id)) return null;
      return { kind: "sendToTerminal", text, sessionId: isId(id) ? id : null };
    }
    case "notesChanged": {
      const folderKey = field(value, "folderKey");
      return isFolderKey(folderKey) ? { kind: "notesChanged", folderKey } : null;
    }
    case "showSession": {
      const sessionId = field(value, "sessionId");
      return isId(sessionId) ? { kind: "showSession", sessionId } : null;
    }
    case "showMain":
      return { kind: "showMain" };
    case "ready":
      return { kind: "ready" };
    default:
      return null;
  }
}

/** `panel-command` payload in the panel window; `null` for anything unexpected. */
export function parsePanelCommand(payload: unknown): PanelCommand | null {
  const value = record(payload);
  if (!value) return null;
  const kind = field(value, "kind");
  switch (kind) {
    case "quickNote":
    case "refresh":
    case "flush":
      return { kind };
    case "capture": {
      const text = field(value, "text");
      const source = field(value, "source");
      if (typeof text !== "string" || text === "" || typeof source !== "string") return null;
      return { kind, text, source };
    }
    case "show": {
      const tab = field(value, "tab");
      return isSideTab(tab) ? { kind, tab } : null;
    }
    default:
      return null;
  }
}

/** A session as the panel needs it: for names in the claims texts and as a "Send to" target. */
export interface PanelSession {
  id: number;
  title: string;
  /** Exited sessions keep their name but take no text. */
  running: boolean;
  /** Runs an agent, not a plain shell (sessionChip.ts, `runsAgent`): diff comments go to it. */
  agent: boolean;
}

/** What the tabs of the detached panel show; the main window computes it. */
export interface PanelContext {
  activeId: number | null;
  /** Working folder of the active session. */
  cwd: string | null;
  /** The active session's attention state: a change reads the changes again. */
  trigger: string;
  sessions: readonly PanelSession[];
  claims: ClaimsSnapshot;
  /** Shared projects by id, trimmed to what the tabs read (avatars, teammates' overlaps). */
  activities: ReadonlyMap<string, TeamActivity>;
}

export const EMPTY_PANEL_CONTEXT: PanelContext = {
  activeId: null,
  cwd: null,
  trigger: "none",
  sessions: [],
  claims: EMPTY_CLAIMS,
  activities: new Map(),
};

/** The `panel-context` event's payload: plain JSON, no maps. */
export interface PanelContextPayload {
  activeId: number | null;
  cwd: string | null;
  trigger: string;
  sessions: readonly PanelSession[];
  claims: ClaimsSnapshot;
  teams: { projectId: string; activity: TeamActivity }[];
}

/** The `panel-context` payload, team activities trimmed to what the tabs read. */
export function panelContextPayload(context: PanelContext): PanelContextPayload {
  return {
    activeId: context.activeId,
    cwd: context.cwd,
    trigger: context.trigger,
    sessions: context.sessions,
    claims: context.claims,
    teams: [...context.activities].map(([projectId, activity]) => ({
      projectId,
      activity: {
        ...EMPTY_ACTIVITY,
        peers: activity.peers,
        overlaps: activity.overlaps,
        sessionRepos: activity.sessionRepos,
      },
    })),
  };
}

function parseSessions(value: unknown): PanelSession[] | null {
  if (!Array.isArray(value)) return null;
  const sessions: PanelSession[] = [];
  for (const item of value) {
    const entry = record(item);
    if (!entry) return null;
    const id = field(entry, "id");
    const title = field(entry, "title");
    const running = field(entry, "running");
    const agent = field(entry, "agent");
    if (!isId(id) || typeof title !== "string" || typeof running !== "boolean") return null;
    if (typeof agent !== "boolean") return null;
    sessions.push({ id, title, running, agent });
  }
  return sessions;
}

function parseTeams(value: unknown): Map<string, TeamActivity> | null {
  if (!Array.isArray(value)) return null;
  const teams = new Map<string, TeamActivity>();
  for (const item of value) {
    const entry = record(item);
    if (!entry) return null;
    const projectId = field(entry, "projectId");
    const activity = parseTeamActivity(field(entry, "activity"));
    if (typeof projectId !== "string" || projectId === "" || activity === null) return null;
    if (teams.has(projectId)) return null;
    teams.set(projectId, activity);
  }
  return teams;
}

/** `panel-context` payload in the panel window; `null` if anything is malformed. */
export function parsePanelContext(payload: unknown): PanelContext | null {
  const value = record(payload);
  if (!value) return null;
  const activeId = field(value, "activeId");
  const cwd = field(value, "cwd");
  const trigger = field(value, "trigger");
  const sessions = parseSessions(field(value, "sessions"));
  const claims = parseClaimsSnapshot(field(value, "claims"));
  const activities = parseTeams(field(value, "teams"));
  if (activeId !== null && !isId(activeId)) return null;
  if (cwd !== null && (typeof cwd !== "string" || cwd === "")) return null;
  if (typeof trigger !== "string" || trigger.length > MAX_TRIGGER) return null;
  if (sessions === null || claims === null || activities === null) return null;
  return { activeId, cwd, trigger, sessions, claims, activities };
}

/** Name for the claims texts; a closed session keeps its number (as in the main window). */
export function panelSessionName(sessions: readonly PanelSession[], id: number): string {
  return sessions.find((session) => session.id === id)?.title ?? `#${String(id)}`;
}

/** Whether the session runs an agent; `false` for an unknown session. */
export function panelSessionIsAgent(sessions: readonly PanelSession[], id: number | null): boolean {
  return sessions.some((session) => session.id === id && session.agent);
}

/** Logical pixels, as `open_panel_window` takes them. */
export interface PanelBounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Detached, kept on top, the tab and the place of the window, remembered per machine. */
export interface PanelWindowPrefs {
  detached: boolean;
  onTop: boolean;
  tab: SideTab;
  bounds: PanelBounds | null;
}

export const DEFAULT_WINDOW_PREFS: PanelWindowPrefs = {
  detached: false,
  onTop: false,
  tab: "notes",
  bounds: null,
};
export const WINDOW_PREFS_KEY = "hangar.panelWindow";
/** Before ADR 0007 only the notes could be detached; their prefs carry over once. */
export const LEGACY_WINDOW_PREFS_KEY = "hangar.notesWindow";

function parseBounds(value: unknown): PanelBounds | null {
  const entry = record(value);
  if (!entry) return null;
  const [x, y, width, height] = ["x", "y", "width", "height"].map((name) => field(entry, name));
  const finite = (n: unknown): n is number => typeof n === "number" && Number.isFinite(n);
  if (!finite(x) || !finite(y) || !finite(width) || !finite(height)) return null;
  if (width <= 0 || height <= 0) return null;
  return { x, y, width, height };
}

export function parseWindowPrefs(text: string | null): PanelWindowPrefs {
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
  const tab = field(value, "tab");
  return {
    detached: typeof detached === "boolean" ? detached : DEFAULT_WINDOW_PREFS.detached,
    onTop: typeof onTop === "boolean" ? onTop : DEFAULT_WINDOW_PREFS.onTop,
    tab: isSideTab(tab) ? tab : DEFAULT_WINDOW_PREFS.tab,
    bounds: parseBounds(field(value, "bounds")),
  };
}

export function loadWindowPrefs(): PanelWindowPrefs {
  try {
    const current = localStorage.getItem(WINDOW_PREFS_KEY);
    return parseWindowPrefs(current ?? localStorage.getItem(LEGACY_WINDOW_PREFS_KEY));
  } catch {
    return DEFAULT_WINDOW_PREFS;
  }
}

export function saveWindowPrefs(prefs: PanelWindowPrefs): void {
  try {
    localStorage.setItem(WINDOW_PREFS_KEY, JSON.stringify(prefs));
    localStorage.removeItem(LEGACY_WINDOW_PREFS_KEY);
    // The notes window read the sessions from here; the panel gets them as events.
    localStorage.removeItem("hangar.notesContext");
  } catch {
    // Without storage the panel starts docked next time.
  }
}

/** Changes some prefs, keeping what another window wrote meanwhile. */
export function updateWindowPrefs(patch: Partial<PanelWindowPrefs>): PanelWindowPrefs {
  const next = { ...loadWindowPrefs(), ...patch };
  saveWindowPrefs(next);
  return next;
}
