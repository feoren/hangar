// What the main window keeps in the backend instead of localStorage: the security switches
// (claims, gate, command guard), the queued messages, the protected settings (paste warning,
// automatic updates, HANGAR.md, snippets), the workspace templates and the start and notice
// preferences (private or team start, last program, folders, session restore, notices, the daily
// budget and the day its notice last showed). The
// panel and file viewer windows share the main window's origin and with it its localStorage; a
// compromised viewer could otherwise turn the gate or the paste warning off, queue a message or
// plant a snippet Hangar later pastes into an agent session, plant a template or a recent folder
// that starts a program in a folder of its choice, make the next start private (without the
// gate), silence the notices of an agent that waits for approval, or hide the budget notice. The
// backend commands answer
// the main window only (`main_store.rs`).
//
// Migration: as long as the backend has stored nothing, the values an older version kept in
// localStorage are taken over once; afterwards they are removed from localStorage, and nothing is
// read from there again.

import { api } from "./api";
import { BUDGET_WARNED_STORAGE_KEY, parseBudget, parseWarnedDay } from "./costs";
import {
  FOLDERS_STORAGE_KEY,
  LAST_CWD_STORAGE_KEY,
  legacyFolderPrefs,
  parseFolderPrefs,
  type FolderPrefs,
} from "./folders";
import { LAST_PROGRAM_STORAGE_KEY, legacyLastProgram, parseLastProgram } from "./launch";
import {
  loadLegacyQueue,
  parseStoredQueue,
  persistableQueue,
  QUEUE_STORAGE_KEY,
} from "./messageQueue";
import type { Language } from "./i18n";
import { parseQuietHours } from "./quietHours";
import {
  defaultSettings,
  legacyNotices,
  NOTICE_KEYS,
  noticesOf,
  PROTECTED_KEYS,
  protectedOf,
  SECURITY_SWITCH_KEYS,
  SETTINGS_STORAGE_KEY,
  switchesOf,
  type NoticeSettings,
  type ProtectedSettings,
  type SecuritySwitches,
} from "./settings";
import { parseSnippets } from "./snippets";
import {
  DEFAULT_START_MODE,
  legacyStartMode,
  START_MODE_STORAGE_KEY,
  type StartMode,
} from "./startMode";
import { legacyTemplates, parseTemplates, TEMPLATES_STORAGE_KEY } from "./templates";
import type { WorkspaceTemplate } from "./templates";

/** The backend commands this module needs; replaced in tests. */
export interface MainBackend {
  securitySwitches: () => Promise<unknown>;
  saveSecuritySwitches: (switches: SecuritySwitches) => Promise<unknown>;
  loadMessageQueue: () => Promise<unknown>;
  saveMessageQueue: (entries: Record<string, string>) => Promise<unknown>;
  protectedSettings: () => Promise<unknown>;
  saveProtectedSettings: (settings: ProtectedSettings) => Promise<unknown>;
  loadTemplates: () => Promise<unknown>;
  saveTemplates: (templates: readonly WorkspaceTemplate[]) => Promise<unknown>;
  mainPrefs: () => Promise<unknown>;
  saveMainPrefs: (prefs: MainPrefs) => Promise<unknown>;
}

/**
 * Where and how the next session starts, and whether a waiting session shows itself, as the
 * backend stores them (`main_store.rs`, `MainPrefs`).
 */
export interface MainPrefs extends FolderPrefs, NoticeSettings {
  startMode: StartMode;
  /** The profile Ctrl+Shift+T starts; `null`: the first program in the list. */
  lastProgram: string | null;
  /** The local day ("YYYY-MM-DD") the budget notice last showed; it shows once a day. */
  budgetWarnedDay: string | null;
}

type LegacyStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

/** The switches the backend answered; `null` for "nothing stored yet" and for anything malformed. */
export function parseSwitches(value: unknown): SecuritySwitches | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const read = (key: keyof SecuritySwitches): boolean | null => {
    const flag: unknown = Reflect.get(value, key);
    return typeof flag === "boolean" ? flag : null;
  };
  const claimsEnabled = read("claimsEnabled");
  const gateEnabled = read("gateEnabled");
  const commandGuardEnabled = read("commandGuardEnabled");
  if (claimsEnabled === null || gateEnabled === null || commandGuardEnabled === null) return null;
  return { claimsEnabled, gateEnabled, commandGuardEnabled };
}

function browserStorage(): LegacyStorage | null {
  try {
    return localStorage;
  } catch {
    return null;
  }
}

/** Takes `keys` out of the stored settings text; the rest of the settings stays. */
function dropLegacyKeys(storage: LegacyStorage, keys: readonly string[]): void {
  try {
    const text = storage.getItem(SETTINGS_STORAGE_KEY);
    if (text === null) return;
    const raw: unknown = JSON.parse(text);
    if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return;
    const kept = Object.fromEntries(Object.entries(raw).filter(([key]) => !keys.includes(key)));
    storage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify(kept));
  } catch {
    // Blocked or broken storage: nothing there is read again anyway.
  }
}

/**
 * The switches for this run. `legacy`: what the stored settings text held at start
 * (`legacySwitches`), taken over only while the backend has nothing yet. Without a backend
 * (browser preview) or on an error, everything is on.
 */
export async function loadSecuritySwitches(
  legacy: SecuritySwitches,
  backend: MainBackend = api,
  storage: LegacyStorage | null = browserStorage(),
): Promise<SecuritySwitches> {
  let answer: unknown;
  try {
    answer = await backend.securitySwitches();
  } catch {
    return switchesOf(defaultSettings("en"));
  }
  let switches = parseSwitches(answer);
  if (answer === null) {
    switches = legacy;
    await backend.saveSecuritySwitches(legacy).catch(() => undefined);
  }
  if (storage !== null) dropLegacyKeys(storage, SECURITY_SWITCH_KEYS);
  return switches ?? switchesOf(defaultSettings("en"));
}

/**
 * The stored queued messages per session key. While the backend has none stored yet, the texts
 * an older version kept in localStorage are taken over once; the localStorage copy is removed
 * either way.
 */
export async function loadQueuedMessages(
  backend: MainBackend = api,
  storage: LegacyStorage | null = browserStorage(),
): Promise<Map<string, string>> {
  let answer: unknown;
  try {
    answer = await backend.loadMessageQueue();
  } catch {
    return new Map<string, string>();
  }
  let stored = parseStoredQueue(answer);
  if (answer === null && storage !== null) {
    const legacy = loadLegacyQueue(storage);
    const entries = persistableQueue(Object.fromEntries(legacy));
    // Saved even when empty: from now on the backend has a queue stored, so a viewer that
    // writes the old key later never gets a message migrated.
    await backend.saveMessageQueue(entries).catch(() => undefined);
    stored = new Map(Object.entries(entries));
  }
  try {
    storage?.removeItem(QUEUE_STORAGE_KEY);
  } catch {
    // Blocked storage: nothing there is read again anyway.
  }
  return stored;
}

/**
 * The protected settings the backend answered; `null` for "nothing stored yet" and for anything
 * malformed. Snippets are read as the settings always read them (`parseSnippets`).
 */
export function parseProtected(value: unknown, language: Language): ProtectedSettings | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const read = (key: "warnMultilinePaste" | "autoUpdate" | "hangarMd"): boolean | null => {
    const flag: unknown = Reflect.get(value, key);
    return typeof flag === "boolean" ? flag : null;
  };
  const warnMultilinePaste = read("warnMultilinePaste");
  const autoUpdate = read("autoUpdate");
  const hangarMd = read("hangarMd");
  const snippets: unknown = Reflect.get(value, "snippets");
  if (warnMultilinePaste === null || autoUpdate === null || hangarMd === null) return null;
  if (!Array.isArray(snippets)) return null;
  return { warnMultilinePaste, autoUpdate, hangarMd, snippets: parseSnippets(snippets, language) };
}

/** What the backend takes: snippets without blank ones (a snippet being written), bounded. */
export function persistableProtected(settings: ProtectedSettings): ProtectedSettings {
  // An array is never replaced by the defaults, so the language plays no part here.
  return { ...protectedOf(settings), snippets: parseSnippets(settings.snippets, "en") };
}

/**
 * The protected settings for this run. `legacy`: what the stored settings text held at start
 * (`legacyProtected`), taken over only while the backend has nothing yet, and stored then even
 * when it is all defaults, so a value a viewer writes to the old place later is never migrated.
 * Without a backend (browser preview) or on an error, the defaults apply (warning and updates
 * on).
 */
export async function loadProtectedSettings(
  legacy: ProtectedSettings,
  language: Language,
  backend: MainBackend = api,
  storage: LegacyStorage | null = browserStorage(),
): Promise<ProtectedSettings> {
  const defaults = protectedOf(defaultSettings(language));
  let answer: unknown;
  try {
    answer = await backend.protectedSettings();
  } catch {
    return defaults;
  }
  let settings = parseProtected(answer, language);
  if (answer === null) {
    settings = persistableProtected(legacy);
    await backend.saveProtectedSettings(settings).catch(() => undefined);
  }
  if (storage !== null) dropLegacyKeys(storage, PROTECTED_KEYS);
  return settings ?? defaults;
}

/** Stores the protected settings; blank snippets are left out. */
export function saveProtectedSettings(
  settings: ProtectedSettings,
  backend: MainBackend = api,
): Promise<unknown> {
  return backend.saveProtectedSettings(persistableProtected(settings));
}

/**
 * The stored workspace templates. While the backend has none stored yet, the list an older
 * version kept in localStorage is taken over once and stored, even when empty; the localStorage
 * copy is removed either way.
 */
export async function loadStoredTemplates(
  backend: MainBackend = api,
  storage: LegacyStorage | null = browserStorage(),
): Promise<WorkspaceTemplate[]> {
  let answer: unknown;
  try {
    answer = await backend.loadTemplates();
  } catch {
    return [];
  }
  let templates = parseTemplates(answer);
  if (answer === null) {
    templates = storage === null ? [] : legacyTemplates(storage);
    await backend.saveTemplates(templates).catch(() => undefined);
  }
  try {
    storage?.removeItem(TEMPLATES_STORAGE_KEY);
  } catch {
    // Blocked storage: nothing there is read again anyway.
  }
  return templates;
}

/** Stores the workspace templates; what the backend would refuse is left out. */
export function saveStoredTemplates(
  templates: readonly WorkspaceTemplate[],
  backend: MainBackend = api,
): Promise<unknown> {
  return backend.saveTemplates(parseTemplates(templates));
}

/** Stores the queued messages; what the backend would refuse is left out. */
export function saveQueuedMessages(
  entries: Record<string, string>,
  backend: MainBackend = api,
): Promise<unknown> {
  return backend.saveMessageQueue(persistableQueue(entries));
}

/** The preferences before anything was chosen: private start, home folder, every notice on. */
export function defaultMainPrefs(): MainPrefs {
  return {
    startMode: DEFAULT_START_MODE,
    lastProgram: null,
    favorites: [],
    recent: [],
    ...noticesOf(defaultSettings("en")),
    budgetWarnedDay: null,
  };
}

/**
 * The preferences the backend answered; `null` for "nothing stored yet" and for anything
 * malformed. Folders are read as the menu always reads them (`parseFolderPrefs`).
 */
export function parseMainPrefs(value: unknown): MainPrefs | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const read = (
    key: "restoreSessions" | "attentionEnabled" | "taskbarFlash" | "notifyDesktop" | "chime",
  ): boolean | null => {
    const flag: unknown = Reflect.get(value, key);
    return typeof flag === "boolean" ? flag : null;
  };
  const startMode: unknown = Reflect.get(value, "startMode");
  const lastProgram: unknown = Reflect.get(value, "lastProgram");
  const restoreSessions = read("restoreSessions");
  const attentionEnabled = read("attentionEnabled");
  const taskbarFlash = read("taskbarFlash");
  const notifyDesktop = read("notifyDesktop");
  const chime = read("chime");
  const dailyBudget: unknown = Reflect.get(value, "dailyBudget") ?? null;
  const budgetWarnedDay: unknown = Reflect.get(value, "budgetWarnedDay") ?? null;
  if (startMode !== "private" && startMode !== "team") return null;
  if (dailyBudget !== null && parseBudget(dailyBudget) === null) return null;
  if (budgetWarnedDay !== null && parseWarnedDay(budgetWarnedDay) === null) return null;
  if (lastProgram !== null && parseLastProgram(lastProgram) === null) return null;
  if (
    restoreSessions === null ||
    attentionEnabled === null ||
    taskbarFlash === null ||
    notifyDesktop === null ||
    chime === null
  ) {
    return null;
  }
  const folders = parseFolderPrefs(value);
  return {
    startMode,
    lastProgram: parseLastProgram(lastProgram),
    favorites: folders.favorites,
    recent: folders.recent,
    restoreSessions,
    attentionEnabled,
    taskbarFlash,
    notifyDesktop,
    chime,
    quietHours: parseQuietHours(Reflect.get(value, "quietHours")),
    dailyBudget: parseBudget(dailyBudget),
    budgetWarnedDay: parseWarnedDay(budgetWarnedDay),
  };
}

/** What the backend takes: the fields it knows, folders and the program checked like it does. */
export function persistableMainPrefs(prefs: MainPrefs): MainPrefs {
  const folders = parseFolderPrefs(prefs);
  return {
    startMode: prefs.startMode === "team" ? "team" : "private",
    lastProgram: parseLastProgram(prefs.lastProgram),
    favorites: folders.favorites,
    recent: folders.recent,
    ...noticesOf(prefs),
    quietHours: parseQuietHours(prefs.quietHours),
    dailyBudget: parseBudget(prefs.dailyBudget),
    budgetWarnedDay: parseWarnedDay(prefs.budgetWarnedDay),
  };
}

/**
 * What an older Hangar kept of the preferences in localStorage, for the one migration: the
 * start mode, the last program, the folders, the day the budget notice last showed and the
 * notice settings with the budget in the stored settings text (`settingsText`, read at start
 * before anything saves).
 */
export function legacyMainPrefs(
  storage: LegacyStorage | null,
  settingsText: string | null,
): MainPrefs {
  const notices = legacyNotices(settingsText);
  if (storage === null) return { ...defaultMainPrefs(), ...notices };
  let warnedDay: string | null = null;
  try {
    warnedDay = parseWarnedDay(storage.getItem(BUDGET_WARNED_STORAGE_KEY));
  } catch {
    // Blocked storage: the notice may show once more today.
  }
  return {
    startMode: legacyStartMode(storage),
    lastProgram: legacyLastProgram(storage),
    ...legacyFolderPrefs(storage),
    ...notices,
    budgetWarnedDay: warnedDay,
  };
}

/** Keys an older Hangar kept the preferences under, besides the stored settings text. */
const LEGACY_PREF_KEYS = [
  START_MODE_STORAGE_KEY,
  LAST_PROGRAM_STORAGE_KEY,
  FOLDERS_STORAGE_KEY,
  LAST_CWD_STORAGE_KEY,
  BUDGET_WARNED_STORAGE_KEY,
] as const;

/**
 * The preferences for this run. `legacy`: what localStorage held at start (`legacyMainPrefs`),
 * taken over only while the backend has nothing yet, and stored then even when it is all
 * defaults, so a value a viewer writes to the old places later is never migrated. The old copies
 * are removed once the backend holds the preferences; a failed save keeps them for the next start.
 * Without a backend (browser preview) or on an error, the defaults apply.
 */
export async function loadMainPrefs(
  legacy: MainPrefs,
  backend: MainBackend = api,
  storage: LegacyStorage | null = browserStorage(),
): Promise<MainPrefs> {
  let answer: unknown;
  try {
    answer = await backend.mainPrefs();
  } catch {
    return defaultMainPrefs();
  }
  let prefs = parseMainPrefs(answer);
  let stored = answer !== null;
  if (answer === null) {
    prefs = persistableMainPrefs(legacy);
    stored = await backend.saveMainPrefs(prefs).then(
      () => true,
      () => false,
    );
  }
  // Only once the backend holds them: after a failed save the old copies stay for the next try.
  if (stored && storage !== null) {
    dropLegacyKeys(storage, NOTICE_KEYS);
    for (const key of LEGACY_PREF_KEYS) {
      try {
        storage.removeItem(key);
      } catch {
        // Blocked storage: nothing there is read again anyway.
      }
    }
  }
  return prefs ?? defaultMainPrefs();
}

/** Stores the preferences; what the backend would refuse is left out. */
export function saveMainPrefs(prefs: MainPrefs, backend: MainBackend = api): Promise<unknown> {
  return backend.saveMainPrefs(persistableMainPrefs(prefs));
}
