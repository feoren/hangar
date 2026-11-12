// The event log of the Files tab's activity map (fileActivity.ts) for the whole window, kept in
// localStorage. Only the main window records (`useActivityRecorder`): it hears every claims and
// team update anyway. The detached panel window only reads and follows the main window's writes
// through the `storage` event. Writes are batched: at most one every SAVE_DELAY_MS, and once more
// on leaving.

import { useEffect, useSyncExternalStore } from "react";

import type { ClaimsSnapshot } from "./claims";
import {
  EMPTY_LOG,
  appendEvents,
  localObservations,
  observe,
  parseLog,
  serializeLog,
  teamObservations,
  type EventLog,
  type LocalSession,
} from "./fileActivity";
import type { TeamActivity } from "./team";

const STORAGE_KEY = "hangar.fileActivity";
const SAVE_DELAY_MS = 5000;

let log: EventLog = EMPTY_LOG;
let loaded = false;
let saveTimer: number | null = null;
const listeners = new Set<() => void>();

function read(): EventLog {
  try {
    const text = window.localStorage.getItem(STORAGE_KEY);
    return text === null ? EMPTY_LOG : (parseLog(text, Date.now()) ?? EMPTY_LOG);
  } catch {
    // Storage blocked: the map starts empty and lives as long as the window.
    return EMPTY_LOG;
  }
}

function ensureLoaded(): void {
  if (loaded) return;
  loaded = true;
  log = read();
  window.addEventListener("storage", (event) => {
    if (event.key !== STORAGE_KEY) return;
    log = read();
    notify();
  });
  window.addEventListener("pagehide", save);
}

function notify(): void {
  for (const listener of listeners) listener();
}

function save(): void {
  if (saveTimer !== null) window.clearTimeout(saveTimer);
  saveTimer = null;
  try {
    window.localStorage.setItem(STORAGE_KEY, serializeLog(log));
  } catch {
    // Full or blocked: the map still works for this window.
  }
}

function scheduleSave(): void {
  if (saveTimer !== null) return;
  saveTimer = window.setTimeout(save, SAVE_DELAY_MS);
}

function apply(next: EventLog): void {
  if (next === log) return;
  log = next;
  notify();
  scheduleSave();
}

function recordLocal(snapshot: ClaimsSnapshot, sessions: readonly LocalSession[]): void {
  ensureLoaded();
  // With claims off nothing is observed; what was held stays until they are on again. The empty
  // snapshot of a starting window must not forget the stored holds either: the sessions' claims
  // would count as new changes once they arrive.
  if (!snapshot.enabled || snapshot.sessions.length === 0) return;
  const now = Date.now();
  const { observed, keep } = localObservations(snapshot, sessions);
  const result = observe(log.seen, "w:", observed, keep, now);
  apply(appendEvents(log, result.events, result.seen, now));
}

function recordTeam(
  activities: ReadonlyMap<string, TeamActivity>,
  snapshot: ClaimsSnapshot,
  sessions: readonly LocalSession[],
): void {
  ensureLoaded();
  // Not signed in, or the first team event still to come: nothing to compare yet.
  if (activities.size === 0) return;
  const now = Date.now();
  const observed = teamObservations(activities, snapshot, sessions);
  const result = observe(log.seen, "t:", observed, new Set(), now);
  apply(appendEvents(log, result.events, result.seen, now));
}

function subscribe(listener: () => void): () => void {
  ensureLoaded();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function current(): EventLog {
  return log;
}

/** The log, live: a component renders again when events arrive. */
export function useEventLog(): EventLog {
  return useSyncExternalStore(subscribe, current);
}

/**
 * Records change events from the claims and the team's presence. Main window only; the claims and
 * team updates are the triggers, there is no timer of its own.
 */
export function useActivityRecorder(
  claims: ClaimsSnapshot,
  activities: ReadonlyMap<string, TeamActivity>,
  sessions: readonly LocalSession[],
): void {
  useEffect(() => {
    recordLocal(claims, sessions);
    // A new session alone holds nothing new; the claims follow when it changes files.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [claims]);
  useEffect(() => {
    recordTeam(activities, claims, sessions);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activities]);
}
