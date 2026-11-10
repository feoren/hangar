// The one sub-agent watch's state, outside React (contract:
// docs/wiki/pages/syntheses/agent-workflow-features.md, item 23). Snapshots arrive up to four
// times a second while a sub-agent works; keeping them here means only the components that show
// sub-agents re-render, never the whole app. Pure (no Tauri), so it is tested without the backend;
// useSubAgents.ts owns the instance and the watch. Nothing here is ever persisted.

import {
  applySnapshot,
  EMPTY_VIEW,
  parseSubAgentEvent,
  withoutExcerpts,
  type SubAgentView,
} from "./subAgents";

export interface SubAgentWatchState {
  /** Watched session and conversation (`watchKey`), `null` while nothing is watched. */
  key: string | null;
  /** Whether the running watch asked for excerpts. */
  excerpts: boolean;
  view: SubAgentView;
  /** The backend refused the watch (unknown session, no sub-agents for it). */
  error: string | null;
}

export const IDLE_WATCH: SubAgentWatchState = {
  key: null,
  excerpts: false,
  view: EMPTY_VIEW,
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
   * blink while the watch restarts; excerpts no longer wanted are dropped at once.
   */
  retarget(key: string | null, excerpts: boolean): void {
    if (key === null) {
      this.set(IDLE_WATCH);
    } else if (this.state.key !== key) {
      this.set({ key, excerpts, view: EMPTY_VIEW, error: null });
    } else if (this.state.excerpts !== excerpts) {
      this.set({
        ...this.state,
        excerpts,
        view: excerpts ? this.state.view : withoutExcerpts(this.state.view),
        // A new watch starts; a refusal of the previous one does not carry over.
        error: null,
      });
    }
  }

  /**
   * One channel message of the watch for `key` with `excerpts`. A message of a watch that is no
   * longer the current one (other session, other excerpt setting, still ending) is dropped.
   */
  receive(key: string, excerpts: boolean, message: unknown): void {
    if (this.state.key !== key || this.state.excerpts !== excerpts) return;
    const snapshot = parseSubAgentEvent(message);
    if (snapshot === null) return;
    this.set({ ...this.state, view: applySnapshot(this.state.view, snapshot) });
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
