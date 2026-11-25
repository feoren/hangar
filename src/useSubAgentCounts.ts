// The sessions' running sub-agents, outside React state: the backend sends a snapshot only when a
// count changed, and only the row whose count changed re-renders, never the sidebar or the app.
// The listener starts with the first row and asks once for the current snapshot, so a reloaded
// window does not wait for the next change.

import { listen } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";

import { api, SUB_AGENT_COUNTS_CHANGED } from "./api";
import { parseSubAgentCounts } from "./subAgentCounts";

let sessions: ReadonlyMap<number, number> = new Map();
const listeners = new Set<() => void>();
let started = false;

function accept(value: unknown): void {
  const parsed = parseSubAgentCounts(value);
  if (parsed === null) return;
  sessions = parsed;
  for (const listener of listeners) listener();
}

function start(): void {
  if (started) return;
  started = true;
  try {
    listen(SUB_AGENT_COUNTS_CHANGED, (event) => {
      accept(event.payload);
    }).catch(() => undefined);
    api
      .subAgentCounts()
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

/** Running sub-agents of one session as the backend counted them; 0 for none (or no agent). */
export function useSubAgentCount(sessionId: number): number {
  return useSyncExternalStore(subscribe, () => sessions.get(sessionId) ?? 0);
}
