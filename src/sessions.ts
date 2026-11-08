// Sidebar state: which sessions exist and which one is visible.
// A pure reducer, so the logic can be tested without UI and without backend.

import type { SessionInfo, Worktree } from "./api";
import { hasMarker, type Marker } from "./marker";

/** A session as the UI shows it: backend data plus flags of our own. */
export interface SessionView extends SessionInfo {
  /** The user chose "End process"; the exit that follows is not a crash. */
  stoppedByUser?: boolean;
  /** Slot in the saved workspace; only set when restoring. */
  restoreOrder?: number;
  /** Color and emoji chosen by the user; kept across rename and restart. */
  marker?: Marker;
  /** The session's own git worktree; removed when the session is closed. */
  worktree?: Worktree;
}

export interface SessionsState {
  sessions: SessionView[];
  activeId: number | null;
}

export type SessionsAction =
  | { type: "added"; session: SessionInfo; worktree?: Worktree }
  /**
   * A restored session at its saved slot, even if it finishes sooner or later than the others.
   * Becomes active only if no session is active yet.
   */
  | { type: "restored"; session: SessionInfo; order: number; marker?: Marker; worktree?: Worktree }
  | { type: "stopRequested"; id: number }
  | { type: "stopFailed"; id: number }
  | { type: "updated"; session: SessionInfo }
  /** The same session with a new ID at the same spot (placeholder after a successful start). */
  | { type: "replaced"; oldId: number; session: SessionInfo }
  | { type: "exited"; id: number; code: number | null }
  /**
   * Resuming failed and the backend started the session normally (`resumeFallback`): the
   * conversation id and the sub-agent flag it sent replace the old ones.
   */
  | { type: "fellBack"; id: number; cliSessionId: string | null; subAgents: boolean }
  | { type: "removed"; id: number }
  | { type: "activated"; id: number }
  /** New marker; no color and no emoji removes it. */
  | { type: "marked"; id: number; marker: Marker }
  | { type: "cycled"; step: 1 | -1 }
  /** Dragged or moved by keyboard in the sidebar: the session now sits at `index`. */
  | { type: "moved"; id: number; index: number };

export const initialSessions: SessionsState = { sessions: [], activeId: null };

export function sessionsReducer(state: SessionsState, action: SessionsAction): SessionsState {
  switch (action.type) {
    case "added": {
      if (state.sessions.some((s) => s.id === action.session.id)) return state;
      const view = withWorktree({ ...action.session }, action.worktree);
      return { sessions: [...state.sessions, view], activeId: action.session.id };
    }
    case "restored": {
      if (state.sessions.some((s) => s.id === action.session.id)) return state;
      const view: SessionView = withWorktree(
        withMarker({ ...action.session, restoreOrder: action.order }, action.marker),
        action.worktree,
      );
      // In front of the first session with a later slot; newly started ones (no slot) go last.
      const index = state.sessions.findIndex(
        (s) => (s.restoreOrder ?? Number.POSITIVE_INFINITY) > action.order,
      );
      const sessions =
        index < 0
          ? [...state.sessions, view]
          : [...state.sessions.slice(0, index), view, ...state.sessions.slice(index)];
      return { sessions, activeId: state.activeId ?? view.id };
    }
    case "stopRequested":
      return mark(state, action.id, true);
    case "stopFailed":
      return mark(state, action.id, false);
    case "updated":
      return {
        ...state,
        sessions: state.sessions.map((s) =>
          s.id === action.session.id ? keep(s, action.session) : s,
        ),
      };
    case "replaced": {
      if (!state.sessions.some((s) => s.id === action.oldId)) return state;
      if (state.sessions.some((s) => s.id === action.session.id)) return state;
      return {
        // The slot stays, or a session restored later would sort in wrongly; "ended by you"
        // no longer applies to the new process.
        sessions: state.sessions.map((s) =>
          s.id === action.oldId
            ? withWorktree(
                withMarker(withOrder({ ...action.session }, s.restoreOrder), s.marker),
                s.worktree,
              )
            : s,
        ),
        activeId: state.activeId === action.oldId ? action.session.id : state.activeId,
      };
    }
    case "exited":
      return {
        ...state,
        sessions: state.sessions.map((s) =>
          s.id === action.id ? { ...s, status: "exited", exitCode: action.code } : s,
        ),
      };
    case "fellBack": {
      const session = state.sessions.find((s) => s.id === action.id);
      if (!session) return state;
      if (
        session.cliSessionId === action.cliSessionId &&
        session.subAgents === action.subAgents &&
        session.resumed !== true
      ) {
        return state;
      }
      return {
        ...state,
        sessions: state.sessions.map((s) =>
          s.id === action.id
            ? {
                ...s,
                cliSessionId: action.cliSessionId,
                subAgents: action.subAgents,
                resumed: false,
              }
            : s,
        ),
      };
    }
    case "removed": {
      const index = state.sessions.findIndex((s) => s.id === action.id);
      if (index < 0) return state;
      const sessions = state.sessions.filter((s) => s.id !== action.id);
      if (state.activeId !== action.id) return { ...state, sessions };
      // The neighbouring session takes over: the next one, otherwise the previous one.
      const neighbour = sessions[index] ?? sessions[index - 1];
      return { sessions, activeId: neighbour?.id ?? null };
    }
    case "marked": {
      const session = state.sessions.find((s) => s.id === action.id);
      if (!session) return state;
      return {
        ...state,
        sessions: state.sessions.map((s) =>
          s.id === action.id ? withMarker(s, action.marker) : s,
        ),
      };
    }
    case "activated":
      if (!state.sessions.some((s) => s.id === action.id)) return state;
      return { ...state, activeId: action.id };
    case "cycled": {
      const count = state.sessions.length;
      if (count === 0) return state;
      const current = state.sessions.findIndex((s) => s.id === state.activeId);
      const next = (((current < 0 ? 0 : current + action.step) % count) + count) % count;
      return { ...state, activeId: state.sessions[next]?.id ?? null };
    }
    case "moved": {
      const sessions = moveItem(state.sessions, action.id, action.index);
      return sessions === null ? state : { ...state, sessions };
    }
  }
}

/**
 * Moves the item with this ID so that it ends up at `index` (clamped to the list). `null` when
 * nothing moves (unknown ID, same place).
 */
export function moveItem<T extends { id: number }>(
  items: readonly T[],
  id: number,
  index: number,
): T[] | null {
  const from = items.findIndex((item) => item.id === id);
  const item = items[from];
  if (item === undefined) return null;
  const to = Math.min(Math.max(Math.trunc(index), 0), items.length - 1);
  if (to === from) return null;
  const rest = items.filter((other) => other.id !== id);
  return [...rest.slice(0, to), item, ...rest.slice(to)];
}

function mark(state: SessionsState, id: number, stoppedByUser: boolean): SessionsState {
  const session = state.sessions.find((s) => s.id === id);
  if (!session || (session.stoppedByUser ?? false) === stoppedByUser) return state;
  return {
    ...state,
    sessions: state.sessions.map((s) => (s.id === id ? { ...s, stoppedByUser } : s)),
  };
}

/**
 * New backend data, our own flags stay: the slot always, "ended by you" only while the session
 * stays exited (rename yes, restart no).
 */
function keep(previous: SessionView, next: SessionInfo): SessionView {
  const view = withWorktree(
    withMarker(withOrder({ ...next }, previous.restoreOrder), previous.marker),
    previous.worktree,
  );
  if (previous.stoppedByUser === true && next.status === "exited") view.stoppedByUser = true;
  return view;
}

/** Sets or removes the marker; an empty one is not stored. */
function withMarker(view: SessionView, marker: Marker | undefined): SessionView {
  const next = { ...view };
  if (hasMarker(marker)) next.marker = marker;
  else delete next.marker;
  return next;
}

function withWorktree(view: SessionView, worktree: Worktree | undefined): SessionView {
  if (worktree !== undefined) view.worktree = worktree;
  return view;
}

function withOrder(view: SessionView, order: number | undefined): SessionView {
  if (order !== undefined) view.restoreOrder = order;
  return view;
}

/**
 * Next waiting session after the active one, round robin in sidebar order. If the active one is
 * the only waiting session, it stays; with none waiting, `null`.
 */
export function nextWaitingId(
  sessions: readonly Pick<SessionInfo, "id">[],
  activeId: number | null,
  isWaiting: (id: number) => boolean,
): number | null {
  const count = sessions.length;
  const start = sessions.findIndex((s) => s.id === activeId);
  for (let offset = 1; offset <= count; offset += 1) {
    const session = sessions[(((start + offset) % count) + count) % count];
    if (session && isWaiting(session.id)) return session.id;
  }
  return null;
}

/**
 * Name for a new session: the program name, or if that is taken, the name with the smallest free
 * number from 2 ("PowerShell 7 (2)"). Case does not matter, nor does surrounding whitespace.
 */
export function numberedTitle(base: string, existing: readonly string[]): string {
  const taken = new Set(existing.map((title) => title.trim().toLocaleLowerCase()));
  if (!taken.has(base.trim().toLocaleLowerCase())) return base;
  for (let number = 2; ; number += 1) {
    const title = `${base} (${String(number)})`;
    if (!taken.has(title.toLocaleLowerCase())) return title;
  }
}

/**
 * Running update session for this program, otherwise `null`. A second update next to it (double
 * click, sidebar and Settings) would work on the same files; the running one is shown instead.
 */
export function runningUpdateId(
  sessions: readonly Pick<SessionInfo, "id" | "profileId" | "kind" | "status">[],
  profileId: string,
): number | null {
  const found = sessions.find(
    (s) => s.kind === "update" && s.profileId === profileId && s.status !== "exited",
  );
  return found?.id ?? null;
}

/** Running backend sessions that the UI does not know about (after a reload). */
export function backgroundSessions(
  running: readonly SessionInfo[],
  knownIds: ReadonlySet<number>,
): SessionInfo[] {
  return running.filter((session) => session.status !== "exited" && !knownIds.has(session.id));
}
