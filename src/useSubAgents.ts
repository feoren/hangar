// Watching one session's sub-agents for the sub-agent view (contract:
// docs/wiki/pages/syntheses/agent-workflow-features.md, item 23). The backend sends at most four
// snapshots per second, so each one may become a render of the view. The snapshot lives only in
// this hook's state: it is never written to the workspace, an export, a notification or storage.

import { useEffect, useState } from "react";

import { api, Channel, errorText } from "./api";
import { applySnapshot, EMPTY_VIEW, parseSubAgentEvent, type SubAgentView } from "./subAgents";

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

interface State extends SubAgentWatch {
  key: string | null;
}

function targetKey(target: Target | null): string | null {
  if (target === null) return null;
  return [String(target.sessionId), target.cliSessionId ?? "", String(target.excerpts)].join(":");
}

/** `null`: nothing is watched (view closed or sub-agents not available for the session). */
export function useSubAgents(target: Target | null): SubAgentWatch {
  const key = targetKey(target);
  const [state, setState] = useState<State>({ key, view: EMPTY_VIEW, error: null });
  // Another session or conversation: start empty, so nothing of the previous one shows.
  if (state.key !== key) setState({ key, view: EMPTY_VIEW, error: null });

  const sessionId = target?.sessionId ?? null;
  const excerpts = target?.excerpts ?? true;
  useEffect(() => {
    if (sessionId === null || key === null) return;
    let live = true;
    const channel = new Channel();
    channel.onmessage = (message) => {
      if (!live) return;
      const snapshot = parseSubAgentEvent(message);
      if (snapshot === null) return;
      setState((current) =>
        current.key === key ? { ...current, view: applySnapshot(current.view, snapshot) } : current,
      );
    };
    serial(() => api.watchSubAgents(sessionId, channel, excerpts)).catch((e: unknown) => {
      if (!live) return;
      const text = errorText(e);
      setState((current) => (current.key === key ? { ...current, error: text } : current));
    });
    return () => {
      live = false;
      channel.onmessage = () => undefined;
      serial(() => api.unwatchSubAgents(sessionId)).catch(() => undefined);
    };
  }, [key, sessionId, excerpts]);

  return { view: state.view, error: state.error };
}
