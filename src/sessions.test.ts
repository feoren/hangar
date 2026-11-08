import { describe, expect, it } from "vitest";

import type { SessionInfo } from "./api";
import {
  backgroundSessions,
  initialSessions,
  nextWaitingId,
  numberedTitle,
  runningUpdateId,
  sessionsReducer,
  type SessionsState,
} from "./sessions";

function session(id: number): SessionInfo {
  return {
    id,
    profileId: "pwsh",
    title: `S${String(id)}`,
    cwd: "C:\\",
    status: "running",
    exitCode: null,
    createdAt: 0,
  };
}

function withSessions(ids: number[], activeId: number | null): SessionsState {
  return { sessions: ids.map(session), activeId };
}

describe("sessionsReducer", () => {
  it("replaces a session in the same place and keeps the selection", () => {
    const state = withSessions([1, -1, 2], -1);
    const next = sessionsReducer(state, { type: "replaced", oldId: -1, session: session(7) });
    expect(next.sessions.map((s) => s.id)).toEqual([1, 7, 2]);
    expect(next.activeId).toBe(7);
    const other = sessionsReducer(withSessions([1, -1], 1), {
      type: "replaced",
      oldId: -1,
      session: session(7),
    });
    expect(other.activeId).toBe(1);
    // Unknown old ID or new ID already taken: change nothing.
    expect(sessionsReducer(state, { type: "replaced", oldId: 9, session: session(7) })).toBe(state);
    expect(sessionsReducer(state, { type: "replaced", oldId: -1, session: session(2) })).toBe(
      state,
    );
  });

  it("a new session is appended and becomes active", () => {
    const state = sessionsReducer(initialSessions, { type: "added", session: session(1) });
    expect(state.sessions.map((s) => s.id)).toEqual([1]);
    expect(state.activeId).toBe(1);
  });

  it("adding twice changes nothing", () => {
    const state = withSessions([1], 1);
    expect(sessionsReducer(state, { type: "added", session: session(1) })).toBe(state);
  });

  it("exit sets status and code", () => {
    const state = sessionsReducer(withSessions([1, 2], 1), { type: "exited", id: 2, code: 3 });
    expect(state.sessions[1]).toMatchObject({ status: "exited", exitCode: 3 });
    expect(state.sessions[0]?.status).toBe("running");
  });

  it("an update replaces the session", () => {
    const renamed = { ...session(1), title: "Renamed" };
    const state = sessionsReducer(withSessions([1], 1), { type: "updated", session: renamed });
    expect(state.sessions[0]?.title).toBe("Renamed");
  });

  it("closing the active session activates the next one", () => {
    const state = sessionsReducer(withSessions([1, 2, 3], 2), { type: "removed", id: 2 });
    expect(state.sessions.map((s) => s.id)).toEqual([1, 3]);
    expect(state.activeId).toBe(3);
  });

  it("closing the active session at the end activates the previous one", () => {
    const state = sessionsReducer(withSessions([1, 2, 3], 3), { type: "removed", id: 3 });
    expect(state.activeId).toBe(2);
  });

  it("closing the only session leaves nothing active", () => {
    const state = sessionsReducer(withSessions([1], 1), { type: "removed", id: 1 });
    expect(state).toEqual({ sessions: [], activeId: null });
  });

  it("closing an inactive session keeps the active one", () => {
    const state = sessionsReducer(withSessions([1, 2], 1), { type: "removed", id: 2 });
    expect(state.activeId).toBe(1);
  });

  it("activating unknown IDs is ignored", () => {
    const state = withSessions([1], 1);
    expect(sessionsReducer(state, { type: "activated", id: 9 })).toBe(state);
  });

  it("cycling wraps around", () => {
    let state = withSessions([1, 2, 3], 3);
    state = sessionsReducer(state, { type: "cycled", step: 1 });
    expect(state.activeId).toBe(1);
    state = sessionsReducer(state, { type: "cycled", step: -1 });
    expect(state.activeId).toBe(3);
    expect(sessionsReducer(initialSessions, { type: "cycled", step: 1 })).toBe(initialSessions);
  });

  it("restores sessions to their remembered slot, whatever order they arrive in", () => {
    let state = initialSessions;
    state = sessionsReducer(state, { type: "restored", session: session(12), order: 2 });
    state = sessionsReducer(state, { type: "added", session: session(50) });
    state = sessionsReducer(state, { type: "restored", session: session(10), order: 0 });
    state = sessionsReducer(state, { type: "restored", session: session(11), order: 1 });
    expect(state.sessions.map((s) => s.id)).toEqual([10, 11, 12, 50]);
    // The newly launched session became active; restored ones do not take that away from it.
    expect(state.activeId).toBe(50);
    const first = sessionsReducer(initialSessions, {
      type: "restored",
      session: session(3),
      order: 4,
    });
    expect(first.activeId).toBe(3);
    expect(sessionsReducer(first, { type: "restored", session: session(3), order: 0 })).toBe(first);
  });

  it("keeps the remembered slot on restart for sessions restored later", () => {
    let state = sessionsReducer(initialSessions, {
      type: "restored",
      session: session(10),
      order: 0,
    });
    state = sessionsReducer(state, { type: "replaced", oldId: 10, session: session(20) });
    expect(state.sessions[0]?.restoreOrder).toBe(0);
    state = sessionsReducer(state, { type: "restored", session: session(11), order: 1 });
    expect(state.sessions.map((s) => s.id)).toEqual([20, 11]);
  });

  it('remembers "ended by you" until a restart, even across renaming', () => {
    let state = withSessions([1, 2], 1);
    state = sessionsReducer(state, { type: "stopRequested", id: 1 });
    state = sessionsReducer(state, { type: "exited", id: 1, code: 1 });
    expect(state.sessions[0]?.stoppedByUser).toBe(true);
    const renamed = { ...session(1), title: "renamed", status: "exited", exitCode: 1 } as const;
    state = sessionsReducer(state, { type: "updated", session: renamed });
    expect(state.sessions[0]?.stoppedByUser).toBe(true);
    state = sessionsReducer(state, { type: "updated", session: session(1) });
    expect(state.sessions[0]?.stoppedByUser).toBeUndefined();
    expect(state.sessions[1]?.stoppedByUser).toBeUndefined();
  });

  it("clears the flag when stopping fails", () => {
    let state = withSessions([1], 1);
    state = sessionsReducer(state, { type: "stopRequested", id: 1 });
    state = sessionsReducer(state, { type: "stopFailed", id: 1 });
    expect(state.sessions[0]?.stoppedByUser).toBe(false);
    expect(sessionsReducer(state, { type: "stopFailed", id: 1 })).toBe(state);
    expect(sessionsReducer(state, { type: "stopRequested", id: 9 })).toBe(state);
  });
});

describe("nextWaitingId", () => {
  const list = [session(1), session(2), session(3), session(4)];

  it("cycles to the next waiting session after the active one", () => {
    const waiting = new Set([1, 3]);
    const isWaiting = (id: number) => waiting.has(id);
    expect(nextWaitingId(list, 1, isWaiting)).toBe(3);
    expect(nextWaitingId(list, 3, isWaiting)).toBe(1);
    expect(nextWaitingId(list, 4, isWaiting)).toBe(1);
  });

  it("stays on the active one when only it is waiting, otherwise returns null", () => {
    expect(nextWaitingId(list, 2, (id) => id === 2)).toBe(2);
    expect(nextWaitingId(list, 2, () => false)).toBeNull();
    expect(nextWaitingId([], null, () => true)).toBeNull();
  });

  it("starts at the front without an active session", () => {
    expect(nextWaitingId(list, null, (id) => id >= 2)).toBe(2);
  });
});

describe("numberedTitle", () => {
  it("keeps free names unchanged", () => {
    expect(numberedTitle("PowerShell 7", [])).toBe("PowerShell 7");
    expect(numberedTitle("PowerShell 7", ["Claude Code"])).toBe("PowerShell 7");
  });

  it("numbers identical programs from 2 and fills gaps", () => {
    expect(numberedTitle("PowerShell 7", ["PowerShell 7"])).toBe("PowerShell 7 (2)");
    expect(numberedTitle("PowerShell 7", ["powershell 7 ", "PowerShell 7 (2)"])).toBe(
      "PowerShell 7 (3)",
    );
    expect(numberedTitle("Codex", ["Codex", "Codex (3)"])).toBe("Codex (2)");
  });

  it("reuses a base name that became free", () => {
    expect(numberedTitle("Codex", ["Codex (2)"])).toBe("Codex");
  });
});

describe("backgroundSessions", () => {
  it("counts only running sessions the UI does not know", () => {
    const running = [
      session(1),
      session(2),
      { ...session(3), status: "exited", exitCode: 0 } as const,
      { ...session(4), status: "starting" } as const,
    ];
    expect(backgroundSessions(running, new Set([2])).map((s) => s.id)).toEqual([1, 4]);
    expect(backgroundSessions(running, new Set([1, 2, 4]))).toEqual([]);
    expect(backgroundSessions([], new Set())).toEqual([]);
  });
});

describe("runningUpdateId", () => {
  it("finds only a running update session of the same program", () => {
    const agent = { ...session(1), profileId: "codex" };
    const done = {
      ...session(2),
      profileId: "codex",
      kind: "update" as const,
      status: "exited" as const,
    };
    const other = { ...session(3), profileId: "claude", kind: "update" as const };
    expect(runningUpdateId([agent, done, other], "codex")).toBeNull();
    expect(runningUpdateId([], "codex")).toBeNull();
    const running = {
      ...session(4),
      profileId: "codex",
      kind: "update" as const,
      status: "starting" as const,
    };
    expect(runningUpdateId([agent, done, other, running], "codex")).toBe(4);
    expect(runningUpdateId([agent, done, other, running], "claude")).toBe(3);
  });
});
