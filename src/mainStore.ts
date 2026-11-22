// What the main window keeps in the backend instead of localStorage: the security switches
// (claims, gate, command guard), the queued messages until 0.13 (since then in the workspace,
// migrateQueuedMessages), the protected settings (paste warning,
// automatic updates, HANGAR.md, snippets), the workspace templates and the start and notice
// preferences (private or team start, last program, folders, session restore, notices, the daily
// budget with its early warning and the day and level its notice last showed, the morning summary
// and the day it was last offered for, the dictation key). The
// panel and file viewer windows share the main window's origin and with it its localStorage; a
// compromised viewer could otherwise turn the gate or the paste warning off, queue a message or
// plant a snippet Hangar later pastes into an agent session, plant a template or a recent folder
// that starts a program in a folder of its choice, make the next start private (without the
// gate), silence the notices of an agent that waits for approval, hide the budget notice, or turn
// the dictation key on so that the next hold opens the microphone. The recently opened sessions
// (recentlyOpened.ts) live there too: each starts a program in a folder on one click. The backend
// commands answer the main window only (`main_store.rs`).
//
// Migration: as long as the backend has stored nothing, the values an older version kept in
// localStorage are taken over once; afterwards they are removed from localStorage, and nothing is
// read from there again.

import { api } from "./api";
import {
  BUDGET_WARNED_STORAGE_KEY,
  DEFAULT_EARLY_PERCENT,
  parseBudget,
  parseBudgetWarned,
  parseEarlyPercent,
  parseWarnedDay,
  type BudgetWarned,
} from "./costs";
import { legacyDictationEnabled } from "./dictation";
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
import { parseRecentSessions, type RecentSession } from "./recentlyOpened";
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
import { withQueuedMessages, type Workspace } from "./workspace";

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
  /** The local day and level the budget notice last showed; each level shows once a day. */
  budgetWarned: BudgetWarned | null;
  /** The local day ("YYYY-MM-DD") whose summary was last offered (morningReview.ts). */
  morningOfferedDay: string | null;
  /** Holding the dictation key dictates (useDictation.ts). */
  dictationEnabled: boolean;
}

/**
 * The preferences as the backend answered them. `dictationEnabled` is `null` in a file written
 * before dictation existed: the window then takes over its old localStorage value once.
 */
export type StoredMainPrefs = Omit<MainPrefs, "dictationEnabled"> & {
  dictationEnabled: boolean | null;
};

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
    // An older Hangar kept the dictation key inside the dictation settings; model and language
    // stay there.
    const dictation: unknown = kept.dictation;
    if (typeof dictation === "object" && dictation !== null && !Array.isArray(dictation)) {
      kept.dictation = Object.fromEntries(
        Object.entries(dictation).filter(([key]) => key !== "enabled"),
      );
    }
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
 * Moves the queued messages an older Hangar kept elsewhere into the workspace, where they live
 * now (`queuedMessage` of each session). Hangar 0.10 to 0.13 kept them in the backend's own
 * file; before that in localStorage, taken over only while that file was never written. A
 * session that has a message in the workspace already keeps it. The workspace with the moved
 * messages is saved before the old places are emptied, so a failed save loses nothing and the
 * next start tries again. The emptied backend file stays as the mark that the migration ran: a
 * text a viewer writes to the old localStorage key later is never taken over. Returns the
 * workspace with the moved messages.
 */
export async function migrateQueuedMessages(
  workspace: Workspace,
  saveWorkspace: (workspace: Workspace) => Promise<unknown>,
  backend: MainBackend = api,
  storage: LegacyStorage | null = browserStorage(),
): Promise<Workspace> {
  let answer: unknown;
  try {
    answer = await backend.loadMessageQueue();
  } catch {
    return workspace;
  }
  const backendQueue = parseStoredQueue(answer);
  const legacy =
    answer === null
      ? storage === null
        ? new Map<string, string>()
        : loadLegacyQueue(storage)
      : backendQueue;
  const migrated = withQueuedMessages(workspace, persistableQueue(Object.fromEntries(legacy)));
  if (migrated !== workspace) {
    try {
      await saveWorkspace(migrated);
    } catch {
      return migrated;
    }
  }
  if (answer === null || backendQueue.size > 0) {
    await backend.saveMessageQueue({}).catch(() => undefined);
  }
  try {
    storage?.removeItem(QUEUE_STORAGE_KEY);
  } catch {
    // Blocked storage: nothing there is read again anyway.
  }
  return migrated;
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

/** The preferences before anything was chosen: private start, home folder, every notice on. */
export function defaultMainPrefs(): MainPrefs {
  return {
    startMode: DEFAULT_START_MODE,
    lastProgram: null,
    favorites: [],
    recent: [],
    ...noticesOf(defaultSettings("en")),
    budgetWarned: null,
    morningOfferedDay: null,
    dictationEnabled: false,
  };
}

/**
 * The preferences the backend answered; `null` for "nothing stored yet" and for anything
 * malformed. Folders are read as the menu always reads them (`parseFolderPrefs`).
 */
export function parseMainPrefs(value: unknown): StoredMainPrefs | null {
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
  // The backend answers every field; missing ones are read as an older backend would mean them.
  const early: unknown = Reflect.get(value, "budgetEarlyPercent");
  const earlyPercent: unknown = early === undefined ? DEFAULT_EARLY_PERCENT : early;
  const budgetWarned: unknown = Reflect.get(value, "budgetWarned") ?? null;
  const morning: unknown = Reflect.get(value, "morningReview") ?? true;
  const morningOfferedDay: unknown = Reflect.get(value, "morningOfferedDay") ?? null;
  const dictation: unknown = Reflect.get(value, "dictationEnabled");
  if (startMode !== "private" && startMode !== "team") return null;
  if (dailyBudget !== null && parseBudget(dailyBudget) === null) return null;
  if (earlyPercent !== null && parseEarlyPercent(earlyPercent) === null) return null;
  if (budgetWarned !== null && parseBudgetWarned(budgetWarned) === null) return null;
  if (typeof morning !== "boolean") return null;
  if (morningOfferedDay !== null && parseWarnedDay(morningOfferedDay) === null) return null;
  if (typeof dictation !== "boolean" && dictation !== null && dictation !== undefined) return null;
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
    budgetEarlyPercent: parseEarlyPercent(earlyPercent),
    morningReview: morning,
    budgetWarned: parseBudgetWarned(budgetWarned),
    morningOfferedDay: parseWarnedDay(morningOfferedDay),
    dictationEnabled: typeof dictation === "boolean" ? dictation : null,
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
    budgetEarlyPercent: parseEarlyPercent(prefs.budgetEarlyPercent),
    morningReview: prefs.morningReview,
    budgetWarned: parseBudgetWarned(prefs.budgetWarned),
    morningOfferedDay: parseWarnedDay(prefs.morningOfferedDay),
    dictationEnabled: prefs.dictationEnabled,
  };
}

/**
 * What an older Hangar kept of the preferences in localStorage, for the one migration: the
 * start mode, the last program, the folders, the day the budget notice last showed and the
 * notice settings with the budget and the dictation key in the stored settings text
 * (`settingsText`, read at start before anything saves).
 */
export function legacyMainPrefs(
  storage: LegacyStorage | null,
  settingsText: string | null,
): MainPrefs {
  const notices = legacyNotices(settingsText);
  const dictationEnabled = legacyDictationEnabled(settingsText);
  if (storage === null) return { ...defaultMainPrefs(), ...notices, dictationEnabled };
  let warned: BudgetWarned | null = null;
  try {
    // A day alone: that version warned only when the budget was reached.
    warned = parseBudgetWarned(storage.getItem(BUDGET_WARNED_STORAGE_KEY));
  } catch {
    // Blocked storage: the notice may show once more today.
  }
  return {
    startMode: legacyStartMode(storage),
    lastProgram: legacyLastProgram(storage),
    ...legacyFolderPrefs(storage),
    ...notices,
    budgetWarned: warned,
    morningOfferedDay: null,
    dictationEnabled,
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
 * defaults, so a value a viewer writes to the old places later is never migrated. A file from
 * before dictation (`dictationEnabled` missing) takes over the old dictation key the same way,
 * once. The old copies are removed once the backend holds the preferences; a failed save keeps
 * them for the next start. Without a backend (browser preview) or on an error, the defaults apply.
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
  const answered = parseMainPrefs(answer);
  let prefs: MainPrefs | null = null;
  let stored = answer !== null;
  const store = (next: MainPrefs) =>
    backend.saveMainPrefs(next).then(
      () => true,
      () => false,
    );
  if (answer === null) {
    prefs = persistableMainPrefs(legacy);
    stored = await store(prefs);
  } else if (answered !== null && answered.dictationEnabled === null) {
    prefs = persistableMainPrefs({ ...answered, dictationEnabled: legacy.dictationEnabled });
    stored = await store(prefs);
  } else if (answered !== null) {
    prefs = { ...answered, dictationEnabled: answered.dictationEnabled === true };
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

/** The backend commands of the recently opened sessions; replaced in tests. */
export interface RecentBackend {
  loadRecentlyClosed: () => Promise<unknown>;
  saveRecentlyClosed: (entries: readonly RecentSession[]) => Promise<unknown>;
}

/**
 * The recently opened sessions, most recent first. Nothing to migrate: they never lived in
 * localStorage. Without a backend (browser preview), on an error or for anything malformed: none.
 */
export async function loadRecentSessions(backend: RecentBackend = api): Promise<RecentSession[]> {
  try {
    return parseRecentSessions(await backend.loadRecentlyClosed());
  } catch {
    return [];
  }
}

/** Stores the recently opened sessions as the backend takes them. */
export function saveRecentSessions(
  entries: readonly RecentSession[],
  backend: RecentBackend = api,
): Promise<unknown> {
  return backend.saveRecentlyClosed(parseRecentSessions(entries));
}
