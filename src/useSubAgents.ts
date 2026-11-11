// Watching one session's sub-agents for the sub-agent view and the sidebar's tree (contract:
// docs/wiki/pages/syntheses/agent-workflow-features.md, item 23). The backend sends at most four
// snapshots and log batches per second; they go into an external store (subAgentStore.ts), so
// only the components that show sub-agents re-render, not the app. Everything lives only in that
// store's memory: it is never written to the workspace, an export, a notification or storage.

import { useEffect, useRef, useSyncExternalStore } from "react";

import { api, Channel, errorText } from "./api";
import {
  errorOf,
  FOCUS_DEBOUNCE_MS,
  FocusSender,
  logOf,
  SubAgentStore,
  viewOf,
  watchKey,
} from "./subAgentStore";
import { visibleAgents, type SubAgent, type SubAgentLog, type SubAgentView } from "./subAgents";

/** The one watch's state; there is at most one watch at a time (watchedSession). */
const store = new SubAgentStore();

/** Focus switches of the one watch, sent once they settle. */
const focusSender = new FocusSender(FOCUS_DEBOUNCE_MS, (sessionId, focus) => {
  serial(() => api.focusSubAgent(sessionId, focus)).catch(() => undefined);
});

/**
 * Watch, focus and unwatch run one after the other in call order. Otherwise the unwatch of a
 * view that closes could reach the backend after the watch of the next one (restart with a new
 * conversation id, excerpts switched) and end it.
 */
let queue: Promise<unknown> = Promise.resolve();
function serial<T>(work: () => Promise<T>): Promise<T> {
  const next = queue.then(work, work);
  queue = next.catch(() => undefined);
  return next;
}

export interface SubAgentWatch {
  view: SubAgentView;
  /** The backend refused the watch (unknown session, no sub-agents for it). */
  error: string | null;
}

interface Target {
  sessionId: number;
  /** A new conversation (restart, resume fallback) starts a new watch. */
  cliSessionId: string | null;
  excerpts: boolean;
  /** The sub-agent whose activity log the view shows; a change does not restart the watch. */
  focus: string | null;
}

/**
 * Runs the watch for `target` (`null`: nothing is watched, because neither the view nor the
 * sidebar's tree needs it or sub-agents are not available for the session) and returns its key
 * for the readers below. Holds no React state: a snapshot never re-renders the caller.
 */
export function useSubAgentWatch(target: Target | null): string | null {
  const key = target === null ? null : watchKey(target.sessionId, target.cliSessionId);
  const sessionId = target?.sessionId ?? null;
  const excerpts = target?.excerpts ?? false;
  const focus = target?.focus ?? null;
  /** The focus as of this render, for a watch that (re)starts. */
  const focusNow = useRef(focus);
  // Runs before the watch effect below, so a (re)start sends the current focus.
  useEffect(() => {
    focusNow.current = focus;
  }, [focus]);
  useEffect(() => {
    store.retarget(key, excerpts);
    store.refocus(focusNow.current);
    if (sessionId === null || key === null) return;
    let live = true;
    const channel = new Channel();
    channel.onmessage = (message) => {
      if (live) store.receive(key, excerpts, message);
    };
    const initial = focusNow.current;
    focusSender.started(sessionId, initial);
    serial(() => api.watchSubAgents(sessionId, channel, excerpts, initial)).catch((e: unknown) => {
      if (live) store.fail(key, errorText(e));
    });
    return () => {
      live = false;
      focusSender.stopped();
      channel.onmessage = () => undefined;
      serial(() => api.unwatchSubAgents(sessionId)).catch(() => undefined);
    };
  }, [key, sessionId, excerpts]);
  useEffect(() => {
    store.refocus(focus);
    focusSender.want(focus);
  }, [focus]);
  return key;
}

/**
 * Snapshot and error of the watch `key`; re-renders the caller with every snapshot, but not
 * with log batches (the view object stays the same while only the log changes).
 */
export function useSubAgentView(key: string | null): SubAgentWatch {
  const view = useSyncExternalStore(store.subscribe, () => viewOf(store.current(), key));
  const error = useSyncExternalStore(store.subscribe, () => errorOf(store.current(), key));
  return { view, error };
}

/** The activity log of the watch `key`; re-renders the caller with every batch. */
export function useSubAgentLog(key: string | null): SubAgentLog {
  return useSyncExternalStore(store.subscribe, () => logOf(store.current(), key));
}

export interface SubAgentSummary {
  count: number;
  /** `null` until the first snapshot arrived. */
  located: boolean | null;
  error: string | null;
}

/** The sub-agents of the watch `key` right now, for event handlers (no re-render). */
export function subAgentsNow(key: string | null): readonly SubAgent[] {
  return viewOf(store.current(), key).agents;
}

/**
 * What the app needs for the view's notice and the tree. Each part is a primitive, so a snapshot
 * that only changes times, tools or excerpts does not re-render the caller. `count` leaves out
 * the `hidden` (removed) sub-agents.
 */
export function useSubAgentSummary(
  key: string | null,
  hidden: ReadonlySet<string>,
): SubAgentSummary {
  const count = useSyncExternalStore(
    store.subscribe,
    () => visibleAgents(viewOf(store.current(), key).agents, hidden).length,
  );
  const located = useSyncExternalStore(store.subscribe, () => viewOf(store.current(), key).located);
  const error = useSyncExternalStore(store.subscribe, () => errorOf(store.current(), key));
  return { count, located, error };
}
