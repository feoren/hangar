import { describe, expect, it } from "vitest";

import { sessionsReducer, type SessionsState } from "./sessions";
import {
  fillSlots,
  isSplitMode,
  reconcileSlots,
  showSessions,
  slotOf,
  slotRect,
  type SplitSlots,
} from "./split";

describe("fillSlots", () => {
  it("puts the active session first, then the others in sidebar order", () => {
    expect(fillSlots("grid", [1, 2, 3, 4, 5], 3)).toEqual({ slots: [3, 1, 2, 4], focus: 0 });
    expect(fillSlots("columns", [1, 2, 3], 2)).toEqual({ slots: [2, 1], focus: 0 });
    expect(fillSlots("single", [1, 2], 2)).toEqual({ slots: [2], focus: 0 });
  });

  it("leaves panes empty when there are fewer sessions", () => {
    expect(fillSlots("grid", [7], 7)).toEqual({ slots: [7, null, null, null], focus: 0 });
    expect(fillSlots("columns", [], null)).toEqual({ slots: [null, null], focus: 0 });
  });
});

describe("reconcileSlots", () => {
  const grid: SplitSlots = { slots: [1, 2, 3, 4], focus: 0 };

  it("focuses a visible session without moving anything", () => {
    expect(reconcileSlots(grid, "grid", [1, 2, 3, 4, 5], 3)).toEqual({
      slots: [1, 2, 3, 4],
      focus: 2,
    });
  });

  it("shows a hidden session in the focused pane", () => {
    const focused: SplitSlots = { slots: [1, 2, 3, 4], focus: 1 };
    expect(reconcileSlots(focused, "grid", [1, 2, 3, 4, 5], 5)).toEqual({
      slots: [1, 5, 3, 4],
      focus: 1,
    });
  });

  it("puts a new session into an empty pane first", () => {
    const half: SplitSlots = { slots: [1, 2, null, null], focus: 0 };
    expect(reconcileSlots(half, "grid", [1, 2, 3], 3)).toEqual({
      slots: [1, 2, 3, null],
      focus: 2,
    });
  });

  it("returns the same object when nothing changes", () => {
    expect(reconcileSlots(grid, "grid", [1, 2, 3, 4], 1)).toBe(grid);
  });

  it("refills a closed session's pane with the next hidden one", () => {
    // Session 2 (focused) closes; the reducer activates its neighbour 3, which is visible.
    const focused: SplitSlots = { slots: [1, 2, 3, 4], focus: 1 };
    expect(reconcileSlots(focused, "grid", [1, 3, 4, 5], 3)).toEqual({
      slots: [1, 5, 3, 4],
      focus: 2,
    });
  });

  it("empties a closed session's pane when no session is hidden", () => {
    const focused: SplitSlots = { slots: [1, 2], focus: 1 };
    expect(reconcileSlots(focused, "columns", [1], 1)).toEqual({ slots: [1, null], focus: 0 });
  });

  it("clears everything when the last session closes", () => {
    expect(reconcileSlots({ slots: [1, 2], focus: 0 }, "columns", [], null)).toEqual({
      slots: [null, null],
      focus: 0,
    });
  });

  it("fills again when the mode changes", () => {
    expect(reconcileSlots(grid, "columns", [1, 2, 3, 4], 4)).toEqual({
      slots: [4, 1],
      focus: 0,
    });
    expect(reconcileSlots({ slots: [2], focus: 0 }, "grid", [1, 2, 3], 2)).toEqual({
      slots: [2, 1, 3, null],
      focus: 0,
    });
  });

  it("keeps single mode on the active session", () => {
    expect(reconcileSlots({ slots: [1], focus: 0 }, "single", [1, 2], 2)).toEqual({
      slots: [2],
      focus: 0,
    });
  });

  it("cycles with Ctrl+Tab like clicking the next session in the sidebar", () => {
    const sessions = [1, 2, 3].map((id) => ({
      id,
      profileId: "pwsh",
      title: String(id),
      cwd: "C:\\",
      status: "running" as const,
      exitCode: null,
      createdAt: 0,
    }));
    let state: SessionsState = { sessions, activeId: 1 };
    let split = fillSlots("columns", [1, 2, 3], 1);
    expect(split).toEqual({ slots: [1, 2], focus: 0 });
    const ids = [1, 2, 3];
    const step = () => {
      state = sessionsReducer(state, { type: "cycled", step: 1 });
      split = reconcileSlots(split, "columns", ids, state.activeId);
    };
    step(); // 2 is visible: focus moves to it
    expect(split).toEqual({ slots: [1, 2], focus: 1 });
    step(); // 3 is hidden: it replaces the focused pane
    expect(split).toEqual({ slots: [1, 3], focus: 1 });
    step(); // 1 is visible again
    expect(split).toEqual({ slots: [1, 3], focus: 0 });
  });
});

describe("showSessions", () => {
  it("picks two side by side or the grid", () => {
    expect(showSessions([4, 5], "single")).toEqual({
      mode: "columns",
      split: { slots: [4, 5], focus: 0 },
    });
    expect(showSessions([4, 5, 6], "single")).toEqual({
      mode: "grid",
      split: { slots: [4, 5, 6, null], focus: 0 },
    });
  });

  it("shows at most four and ignores duplicates", () => {
    expect(showSessions([1, 2, 2, 3, 4, 5], "columns").split.slots).toEqual([1, 2, 3, 4]);
  });

  it("keeps the mode for a single session", () => {
    expect(showSessions([9], "grid").mode).toBe("grid");
  });
});

describe("helpers", () => {
  it("finds the pane of a session", () => {
    expect(slotOf({ slots: [1, null, 3], focus: 0 }, 3)).toBe(2);
    expect(slotOf({ slots: [1], focus: 0 }, 3)).toBeNull();
  });

  it("lays out panes as fractions", () => {
    expect(slotRect("single", 0)).toEqual({ x: 0, y: 0, width: 1, height: 1 });
    expect(slotRect("columns", 1)).toEqual({ x: 0.5, y: 0, width: 0.5, height: 1 });
    expect(slotRect("grid", 2)).toEqual({ x: 0, y: 0.5, width: 0.5, height: 0.5 });
    expect(slotRect("grid", 3)).toEqual({ x: 0.5, y: 0.5, width: 0.5, height: 0.5 });
  });

  it("recognizes modes", () => {
    expect(isSplitMode("grid")).toBe(true);
    expect(isSplitMode("quad")).toBe(false);
    expect(isSplitMode(undefined)).toBe(false);
  });
});
