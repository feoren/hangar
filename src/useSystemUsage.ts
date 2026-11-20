// The machine's load for the system bar, outside React state like the session resources
// (useSessionResources.ts): one poll for all gauges every SYSTEM_POLL_MS, only while the setting
// is on, the window is visible and the bar is on screen. A reading that changes nothing shown is
// not published, so the bar does not render for it. While saving energy on battery the poll
// slows down (power.ts) and the sparklines give way to still bars, so their history starts over.

import { useSyncExternalStore } from "react";

import { api } from "./api";
import {
  advance,
  EMPTY_STATE,
  keepsGpuColumn,
  parseSystemUsage,
  type SystemState,
} from "./systemUsage";
import { currentPlatform } from "./platform";
import { pollInterval, PollTimer } from "./power";
import { isPowerSaving, onPowerSavingChange } from "./usePower";

/** The platform does not change while the window lives. */
const platform = currentPlatform();

let state: SystemState = EMPTY_STATE;
/** What the bar renders; lags `state` while readings change nothing. */
let published: SystemState = EMPTY_STATE;
const listeners = new Set<() => void>();
let enabled = false;
let lastSaving = false;
let watchingVisibility = false;

function publish(): void {
  published = state;
  for (const listener of listeners) listener();
}

function accept(value: unknown): void {
  // Turned off while the answer was on its way.
  if (!enabled) return;
  const usage = parseSystemUsage(value);
  if (usage === null) return;
  const next = advance(state, usage);
  state = next.state;
  if (next.changed) publish();
}

function poll(): void {
  api
    .systemUsage()
    .then(accept)
    .catch(() => undefined);
}

const timer = new PollTimer(poll);

function visible(): boolean {
  return typeof document === "undefined" || document.visibilityState === "visible";
}

/** Starts or stops asking, after any change of the setting, the visibility or the listeners. */
function schedule(): void {
  const wanted = enabled && listeners.size > 0 && visible();
  timer.set(wanted, pollInterval("system", isPowerSaving()));
}

/**
 * Saving turned on or off: a history from before would mix two time scales in one sparkline, so
 * it starts over, and the interval follows.
 */
function powerChanged(): void {
  const saving = isPowerSaving();
  if (saving !== lastSaving) {
    lastSaving = saving;
    if (state !== EMPTY_STATE) {
      state = { ...state, history: EMPTY_STATE.history };
      publish();
    }
  }
  schedule();
}

function watchVisibility(): void {
  if (watchingVisibility || typeof document === "undefined") return;
  watchingVisibility = true;
  document.addEventListener("visibilitychange", schedule);
  // On battery the interval grows; back on AC it shrinks again at once.
  lastSaving = isPowerSaving();
  onPowerSavingChange(powerChanged);
}

/** The setting (the same one as the sessions' resources): off stops asking and hides the bar. */
export function setSystemUsageEnabled(on: boolean): void {
  enabled = on;
  watchVisibility();
  if (!on && state !== EMPTY_STATE) {
    state = EMPTY_STATE;
    publish();
  }
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

/** The last published reading and the history of each gauge. */
export function useSystemUsage(): SystemState {
  return useSyncExternalStore(subscribe, () => published);
}

/**
 * Whether the session rows keep a GPU column. A boolean snapshot: a row re-renders only when the
 * answer changes, not with every reading.
 */
export function useGpuColumn(): boolean {
  return useSyncExternalStore(subscribe, () => keepsGpuColumn(published.usage, platform));
}
