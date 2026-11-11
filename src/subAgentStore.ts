// The one sub-agent watch's state, outside React (contract:
// docs/wiki/pages/syntheses/agent-workflow-features.md, item 23). Snapshots and log lines arrive
// up to four times a second while a sub-agent works; keeping them here means only the components
// that show sub-agents re-render, never the whole app. Pure (no Tauri), so it is tested without
// the backend; useSubAgents.ts owns the instance and the watch. Nothing here is ever persisted.

import {
  applyLog,
  EMPTY_LOG,
  EMPTY_VIEW,
  logFor,
  parseSubAgentEvent,
  parseSubAgentLog,
  viewOfSnapshot,
  withoutExcerpts,
  withoutLogText,
  type SubAgentLog,
  type SubAgentView,
} from "./subAgents";

export interface SubAgentWatchState {
  /** Watched session and conversation (`watchKey`), `null` while nothing is watched. */
  key: string | null;
  /** Whether the running watch asked for excerpts. */
  excerpts: boolean;
  view: SubAgentView;
  /** Activity log of the sub-agent the view shows (`log.agentId`, the watch's focus). */
  log: SubAgentLog;
  /** The backend refused the watch (unknown session, no sub-agents for it). */
  error: string | null;
}

export const IDLE_WATCH: SubAgentWatchState = {
  key: null,
  excerpts: false,
  view: EMPTY_VIEW,
  log: EMPTY_LOG,
  error: null,
};

/** Session and conversation: a restart or resume fallback with a new id is another watch. */
export function watchKey(sessionId: number, cliSessionId: string | null): string {
  return `${String(sessionId)}:${cliSessionId ?? ""}`;
}

/** The view for `key`; a state of another (or no) watch reads as empty. */
export function viewOf(state: SubAgentWatchState, key: string | null): SubAgentView {
  return key !== null && state.key === key ? state.view : EMPTY_VIEW;
}

export function errorOf(state: SubAgentWatchState, key: string | null): string | null {
  return key !== null && state.key === key ? state.error : null;
}

/** The log for `key`; another (or no) watch reads as empty. */
export function logOf(state: SubAgentWatchState, key: string | null): SubAgentLog {
  return key !== null && state.key === key ? state.log : EMPTY_LOG;
}

export class SubAgentStore {
  private state: SubAgentWatchState = IDLE_WATCH;
  private readonly listeners = new Set<() => void>();

  /** For useSyncExternalStore. */
  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  /** For useSyncExternalStore: the same object until something changed. */
  readonly current = (): SubAgentWatchState => this.state;

  /**
   * A watch starts (or nothing is watched: `key` null). Another key starts empty, so nothing of
   * the previous session shows. The same key keeps its agents, so the sidebar's tree does not
   * blink while the watch restarts; texts no longer wanted are dropped at once.
   */
  retarget(key: string | null, excerpts: boolean): void {
    if (key === null) {
      this.set(IDLE_WATCH);
    } else if (this.state.key !== key) {
      // The log stays with its sub-agent, but empty: refocus follows with the watch's focus.
      this.set({
        key,
        excerpts,
        view: EMPTY_VIEW,
        log: logFor(EMPTY_LOG, this.state.log.agentId),
        error: null,
      });
    } else if (this.state.excerpts !== excerpts) {
      this.set({
        ...this.state,
        excerpts,
        view: excerpts ? this.state.view : withoutExcerpts(this.state.view),
        // The new watch sends the log again from the start, with or without texts.
        log: withoutLogText(this.state.log),
        // A new watch starts; a refusal of the previous one does not carry over.
        error: null,
      });
    }
  }

  /** The view shows `agentId` now (`null`: none); its log starts empty until the backend sends it. */
  refocus(agentId: string | null): void {
    const log = logFor(this.state.log, agentId);
    if (log !== this.state.log) this.set({ ...this.state, log });
  }

  /**
   * One channel message of the watch for `key` with `excerpts`. A message of a watch that is no
   * longer the current one (other session, other excerpt setting, still ending) is dropped, as
   * is a log batch of a sub-agent the view no longer shows.
   */
  receive(key: string, excerpts: boolean, message: unknown): void {
    if (this.state.key !== key || this.state.excerpts !== excerpts) return;
    const snapshot = parseSubAgentEvent(message);
    if (snapshot !== null) {
      this.set({ ...this.state, view: viewOfSnapshot(snapshot) });
      return;
    }
    const batch = parseSubAgentLog(message);
    if (batch === null) return;
    const log = applyLog(this.state.log, batch);
    if (log !== this.state.log) this.set({ ...this.state, log });
  }

  /** The backend refused the watch for `key`. */
  fail(key: string, error: string): void {
    if (this.state.key !== key || this.state.error === error) return;
    this.set({ ...this.state, error });
  }

  private set(next: SubAgentWatchState): void {
    if (next === this.state) return;
    this.state = next;
    for (const listener of this.listeners) listener();
  }
}

/** Quiet time before a focus switch goes to the backend. */
export const FOCUS_DEBOUNCE_MS = 100;

/**
 * Sends the watch's focus to the backend once it has settled for `delayMs`: clicking through
 * several sub-agents quickly costs one switch (one reset and one transcript read), not one per
 * click. The view clears the log at once anyway (`SubAgentStore.refocus`).
 */
export class FocusSender {
  /** Session and focus the running watch has; `null` while none runs. */
  private watch: { sessionId: number; focus: string | null } | null = null;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private readonly delayMs: number;
  private readonly send: (sessionId: number, focus: string | null) => void;

  constructor(delayMs: number, send: (sessionId: number, focus: string | null) => void) {
    this.delayMs = delayMs;
    this.send = send;
  }

  /** A watch started with `focus` already: nothing to send for it. */
  started(sessionId: number, focus: string | null): void {
    this.cancel();
    this.watch = { sessionId, focus };
  }

  /** The watch ended; a pending switch is dropped. */
  stopped(): void {
    this.cancel();
    this.watch = null;
  }

  /** The view shows `focus` now. */
  want(focus: string | null): void {
    this.cancel();
    if (this.watch === null || this.watch.focus === focus) return;
    this.timer = setTimeout(() => {
      this.timer = undefined;
      if (this.watch === null || this.watch.focus === focus) return;
      this.watch = { ...this.watch, focus };
      this.send(this.watch.sessionId, focus);
    }, this.delayMs);
  }

  private cancel(): void {
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = undefined;
  }
}
