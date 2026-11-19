// The tree flow's state outside React, like the session resources (useSessionResources.ts): the
// terminals and the input path count bytes here, a timer turns them into rates once per second,
// and only the parts of a row that show a level or a rate re-render when it changes. While the
// setting is off (or the WebView cannot draw the flow), counting returns at once and no timer
// runs.

import { useSyncExternalStore } from "react";

import {
  DEFAULT_TREE_FLOW,
  highestLevel,
  RateMeter,
  treeFlowSupported,
  type FlowLevel,
  type FlowRate,
  type TreeFlowPrefs,
} from "./treeFlow";

/** How often the counted bytes become rates: at most once per second, as the rows show them. */
export const SAMPLE_MS = 1000;

const meter = new RateMeter();
let prefs: TreeFlowPrefs = DEFAULT_TREE_FLOW;
let active = false;
let timer: ReturnType<typeof setInterval> | null = null;
const prefListeners = new Set<() => void>();
const rateListeners = new Set<() => void>();
const encoder = typeof TextEncoder === "undefined" ? null : new TextEncoder();

function notify(listeners: ReadonlySet<() => void>): void {
  for (const listener of listeners) listener();
}

function tick(): void {
  if (meter.sample(Date.now())) notify(rateListeners);
}

/**
 * The setting, from the app. `supported` is whether the WebView can draw the flow; off or
 * unsupported stops the timer and forgets every count.
 */
export function setTreeFlow(next: TreeFlowPrefs, supported: boolean = treeFlowSupported()): void {
  const nextActive = next.enabled && supported;
  const changed = nextActive !== active || next !== prefs;
  prefs = next;
  active = nextActive;
  if (active && timer === null) {
    timer = setInterval(tick, SAMPLE_MS);
  } else if (!active && timer !== null) {
    clearInterval(timer);
    timer = null;
  }
  if (!active) meter.clear();
  if (changed) {
    notify(prefListeners);
    notify(rateListeners);
  }
}

/** Output bytes a session's terminal received. Free while the flow is off. */
export function noteOutput(id: number | null, bytes: number): void {
  if (!active || id === null) return;
  meter.output(id, bytes);
}

/** Input Hangar writes to a session. Free while the flow is off. */
export function noteInput(id: number, data: string): void {
  if (!active) return;
  meter.input(id, encoder === null ? data.length : encoder.encode(data).length);
}

/** For tests: samples right away instead of waiting for the timer. */
export function sampleNow(now: number): void {
  if (meter.sample(now)) notify(rateListeners);
}

function subscribePrefs(listener: () => void): () => void {
  prefListeners.add(listener);
  return () => {
    prefListeners.delete(listener);
  };
}

function subscribeRates(listener: () => void): () => void {
  rateListeners.add(listener);
  return () => {
    rateListeners.delete(listener);
  };
}

function activePrefs(): TreeFlowPrefs | null {
  return active ? prefs : null;
}

/** The settings while the flow shows; `null` while it is off or cannot be drawn. */
export function useTreeFlow(): TreeFlowPrefs | null {
  return useSyncExternalStore(subscribePrefs, activePrefs, activePrefs);
}

/** One session's speed level; re-renders only when the level changes. */
export function useFlowLevel(id: number): FlowLevel {
  const read = () => meter.rate(id)?.level ?? 1;
  return useSyncExternalStore(subscribeRates, read, read);
}

/** The highest level of these sessions: a group's trunk runs as fast as its busiest session. */
export function useHighestLevel(ids: readonly number[]): FlowLevel {
  const read = () => highestLevel(ids.map((id) => meter.rate(id)?.level ?? 1));
  return useSyncExternalStore(subscribeRates, read, read);
}

/** One session's rates, the same object while they do not change; `undefined`: nothing yet. */
export function useFlowRate(id: number): FlowRate | undefined {
  const read = () => meter.rate(id);
  return useSyncExternalStore(subscribeRates, read, read);
}
