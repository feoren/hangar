// The sessions' context use, outside React state: the backend sends a snapshot after every
// response of any agent, and only the meter of a session whose numbers changed re-renders, never
// the sidebar or the app. The listener starts with the first meter and asks once for the current
// snapshot, so a reloaded window does not wait for the next response.

import { listen } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";

import { api, CONTEXT_USAGE_CHANGED } from "./api";
import { parseContextSnapshot, type SessionContext } from "./contextUsage";

let sessions: ReadonlyMap<number, SessionContext> = new Map();
const listeners = new Set<() => void>();
let started = false;

function accept(value: unknown): void {
  const parsed = parseContextSnapshot(value);
  if (parsed === null) return;
  // Unchanged numbers keep their object, so their meter does not re-render.
  for (const [id, context] of parsed) {
    const before = sessions.get(id);
    if (before?.used === context.used && before.window === context.window) parsed.set(id, before);
  }
  sessions = parsed;
  for (const listener of listeners) listener();
}

function start(): void {
  if (started) return;
  started = true;
  try {
    listen(CONTEXT_USAGE_CHANGED, (event) => {
      accept(event.payload);
    }).catch(() => undefined);
    api
      .contextUsage()
      .then(accept)
      .catch(() => undefined);
  } catch {
    // Outside Tauri (browser preview) there are no backend events.
  }
}

function subscribe(listener: () => void): () => void {
  start();
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** Context use of one session; `undefined` until its agent answered once (or not an agent). */
export function useSessionContext(sessionId: number): SessionContext | undefined {
  return useSyncExternalStore(subscribe, () => sessions.get(sessionId));
}
