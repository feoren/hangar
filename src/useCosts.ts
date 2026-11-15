// The cost report outside React state, like the context meter (useContextUsage.ts): the backend
// signals only when a number changed, and only components that read it re-render. The signal
// carries nothing (any window may listen, and the report holds session titles); each one reads
// the report anew, and only the answer to the newest read counts.

import { listen } from "@tauri-apps/api/event";
import { useSyncExternalStore } from "react";

import { api, COST_USAGE_CHANGED } from "./api";
import { parseCostReport, type CostReport, type SessionCost } from "./costs";

let report: CostReport | null = null;
const listeners = new Set<() => void>();
let started = false;
let reads = 0;

function accept(value: unknown): void {
  const parsed = parseCostReport(value);
  if (parsed === null) return;
  report = parsed;
  for (const listener of listeners) listener();
}

/** Two reads may answer out of order: an older answer never replaces a newer one. */
function read(): void {
  reads += 1;
  const mine = reads;
  api
    .costReport()
    .then((value) => {
      if (mine === reads) accept(value);
    })
    .catch(() => undefined);
}

function start(): void {
  if (started) return;
  started = true;
  try {
    listen(COST_USAGE_CHANGED, read).catch(() => undefined);
    read();
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

/** The newest report; `null` until the backend sent one. */
export function useCostReport(): CostReport | null {
  return useSyncExternalStore(subscribe, () => report);
}

/** One session's tokens and cost so far; `undefined` without readable usage. */
export function useSessionCost(sessionId: number): SessionCost | undefined {
  return useSyncExternalStore(subscribe, () => report?.sessions.get(sessionId));
}
