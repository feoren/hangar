// The sessions' CPU and memory, outside React state like the context meter (useContextUsage.ts):
// only the row of a session whose numbers changed re-renders. The store asks the backend every
// RESOURCE_POLL_MS, and only while the setting is on, the window is visible and a row listens.

import { useSyncExternalStore } from "react";

import { api } from "./api";
import {
  parseResources,
  RESOURCE_POLL_MS,
  sameResources,
  type SessionResources,
} from "./resources";

let sessions: ReadonlyMap<number, SessionResources> = new Map();
const listeners = new Set<() => void>();
let enabled = false;
let timer: number | null = null;
let watchingVisibility = false;

function publish(next: ReadonlyMap<number, SessionResources>): void {
  sessions = next;
  for (const listener of listeners) listener();
}

function accept(value: unknown): void {
  // Turned off while the answer was on its way.
  if (!enabled) return;
  const parsed = parseResources(value);
  if (parsed === null) return;
  for (const [id, resources] of parsed) {
    const before = sessions.get(id);
    if (before !== undefined && sameResources(before, resources)) parsed.set(id, before);
  }
  publish(parsed);
}

function poll(): void {
  api
    .sessionResources()
    .then(accept)
    .catch(() => undefined);
}

function visible(): boolean {
  return typeof document === "undefined" || document.visibilityState === "visible";
}

/** Starts or stops asking, after any change of the setting, the visibility or the listeners. */
function schedule(): void {
  const wanted = enabled && listeners.size > 0 && visible();
  if (wanted && timer === null) {
    poll();
    timer = window.setInterval(poll, RESOURCE_POLL_MS);
  } else if (!wanted && timer !== null) {
    window.clearInterval(timer);
    timer = null;
  }
}

function watchVisibility(): void {
  if (watchingVisibility || typeof document === "undefined") return;
  watchingVisibility = true;
  document.addEventListener("visibilitychange", schedule);
}

/** The setting: off stops asking and empties what the rows show. */
export function setResourcesEnabled(on: boolean): void {
  enabled = on;
  watchVisibility();
  if (!on && sessions.size > 0) publish(new Map());
  schedule();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  watchVisibility();
  schedule();
  return () => {
    listeners.delete(listener);
    schedule();
  };
}

/** One session's resources; `undefined` while off, hidden, not yet read or not readable. */
export function useSessionResources(sessionId: number): SessionResources | undefined {
  return useSyncExternalStore(subscribe, () => sessions.get(sessionId));
}
