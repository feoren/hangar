// Watching one session's sub-agents for the sub-agent view and the sidebar's tree (contract:
// docs/wiki/pages/syntheses/agent-workflow-features.md, item 23). The backend sends at most four
// snapshots per second; they go into an external store (subAgentStore.ts), so only the components
// that show sub-agents re-render, not the app. The snapshot lives only in that store's memory: it
// is never written to the workspace, an export, a notification or storage.

import { useEffect, useSyncExternalStore } from "react";

import { api, Channel, errorText } from "./api";
import { errorOf, SubAgentStore, viewOf, watchKey } from "./subAgentStore";
import type { SubAgentView } from "./subAgents";

/** The one watch's state; there is at most one watch at a time (watchedSession). */
const store = new SubAgentStore();

/**
 * Watch and unwatch run one after the other in call order. Otherwise the unwatch of a view that
 * closes could reach the backend after the watch of the next one (restart with a new conversation
 * id, excerpts switched) and end it.
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
  useEffect(() => {
    store.retarget(key, excerpts);
    if (sessionId === null || key === null) return;
    let live = true;
    const channel = new Channel();
    channel.onmessage = (message) => {
      if (live) store.receive(key, excerpts, message);
    };
    serial(() => api.watchSubAgents(sessionId, channel, excerpts)).catch((e: unknown) => {
      if (live) store.fail(key, errorText(e));
    });
    return () => {
      live = false;
      channel.onmessage = () => undefined;
      serial(() => api.unwatchSubAgents(sessionId)).catch(() => undefined);
    };
  }, [key, sessionId, excerpts]);
  return key;
}

/** Everything of the watch `key`; re-renders the caller with every snapshot. */
export function useSubAgentView(key: string | null): SubAgentWatch {
  const state = useSyncExternalStore(store.subscribe, store.current);
  return { view: viewOf(state, key), error: errorOf(state, key) };
}

export interface SubAgentSummary {
  count: number;
  /** `null` until the first snapshot arrived. */
  located: boolean | null;
  error: string | null;
}

/**
 * What the app needs for the view's layout and notice. Each part is a primitive, so a snapshot
 * that only changes times, tools or excerpts does not re-render the caller.
 */
export function useSubAgentSummary(key: string | null): SubAgentSummary {
  const count = useSyncExternalStore(
    store.subscribe,
    () => viewOf(store.current(), key).agents.length,
  );
  const located = useSyncExternalStore(store.subscribe, () => viewOf(store.current(), key).located);
  const error = useSyncExternalStore(store.subscribe, () => errorOf(store.current(), key));
  return { count, located, error };
}
