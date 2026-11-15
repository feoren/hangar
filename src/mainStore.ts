// What the main window keeps in the backend instead of localStorage: the security switches
// (claims, gate, command guard) and the queued messages. The panel and file viewer windows share
// the main window's origin and with it its localStorage; a compromised viewer could otherwise
// turn the gate off or queue a message Hangar later pastes into an agent session. The backend
// commands answer the main window only (`main_store.rs`).
//
// Migration: as long as the backend has stored nothing, the values an older version kept in
// localStorage are taken over once; afterwards they are removed from localStorage, and nothing is
// read from there again.

import { api } from "./api";
import {
  loadLegacyQueue,
  parseStoredQueue,
  persistableQueue,
  QUEUE_STORAGE_KEY,
} from "./messageQueue";
import {
  defaultSettings,
  SECURITY_SWITCH_KEYS,
  SETTINGS_STORAGE_KEY,
  switchesOf,
  type SecuritySwitches,
} from "./settings";

/** The backend commands this module needs; replaced in tests. */
export interface MainBackend {
  securitySwitches: () => Promise<unknown>;
  saveSecuritySwitches: (switches: SecuritySwitches) => Promise<unknown>;
  loadMessageQueue: () => Promise<unknown>;
  saveMessageQueue: (entries: Record<string, string>) => Promise<unknown>;
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

/** Takes the switches out of the stored settings text; the rest of the settings stays. */
function dropLegacySwitches(storage: LegacyStorage): void {
  try {
    const text = storage.getItem(SETTINGS_STORAGE_KEY);
    if (text === null) return;
    const raw: unknown = JSON.parse(text);
    if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return;
    const switches: readonly string[] = SECURITY_SWITCH_KEYS;
    const kept = Object.fromEntries(Object.entries(raw).filter(([key]) => !switches.includes(key)));
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
  if (storage !== null) dropLegacySwitches(storage);
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

/** Stores the queued messages; what the backend would refuse is left out. */
export function saveQueuedMessages(
  entries: Record<string, string>,
  backend: MainBackend = api,
): Promise<unknown> {
  return backend.saveMessageQueue(persistableQueue(entries));
}
