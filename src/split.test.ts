import { describe, expect, it } from "vitest";

import { sessionsReducer, type SessionsState } from "./sessions";
import {
  fillSlots,
  placeSession,
  reconcileSlots,
  showSessions,
  slotOf,
  type SplitSlots,
} from "./split";

describe("fillSlots", () => {
  it("puts the active session first, then the others in sidebar order", () => {
    expect(fillSlots("quad", [1, 2, 3, 4, 5], 3)).toEqual({ slots: [3, 1, 2, 4], focus: 0 });
    expect(fillSlots("pair", [1, 2, 3], 2)).toEqual({ slots: [2, 1], focus: 0 });
    expect(fillSlots("single", [1, 2], 2)).toEqual({ slots: [2], focus: 0 });
  });

  it("leaves panes empty when there are fewer sessions", () => {
    expect(fillSlots("quad", [7], 7)).toEqual({ slots: [7, null, null, null], focus: 0 });
    expect(fillSlots("pair", [], null)).toEqual({ slots: [null, null], focus: 0 });
  });
});

describe("reconcileSlots", () => {
  const grid: SplitSlots = { slots: [1, 2, 3, 4], focus: 0 };

  it("focuses a visible session without moving anything", () => {
    expect(reconcileSlots(grid, "quad", [1, 2, 3, 4, 5], 3)).toEqual({
      slots: [1, 2, 3, 4],
      focus: 2,
    });
  });

  it("shows a hidden session in the focused pane", () => {
    const focused: SplitSlots = { slots: [1, 2, 3, 4], focus: 1 };
    expect(reconcileSlots(focused, "quad", [1, 2, 3, 4, 5], 5)).toEqual({
      slots: [1, 5, 3, 4],
      focus: 1,
    });
  });

  it("puts a new session into an empty pane first", () => {
    const half: SplitSlots = { slots: [1, 2, null, null], focus: 0 };
    expect(reconcileSlots(half, "quad", [1, 2, 3], 3)).toEqual({
      slots: [1, 2, 3, null],
      focus: 2,
    });
  });

  it("returns the same object when nothing changes", () => {
    expect(reconcileSlots(grid, "quad", [1, 2, 3, 4], 1)).toBe(grid);
  });

  it("refills a closed session's pane with the next hidden one", () => {
    // Session 2 (focused) closes; the reducer activates its neighbour 3, which is visible.
    const focused: SplitSlots = { slots: [1, 2, 3, 4], focus: 1 };
    expect(reconcileSlots(focused, "quad", [1, 3, 4, 5], 3)).toEqual({
      slots: [1, 5, 3, 4],
      focus: 2,
    });
  });

  it("empties a closed session's pane when no session is hidden", () => {
    const focused: SplitSlots = { slots: [1, 2], focus: 1 };
    expect(reconcileSlots(focused, "pair", [1], 1)).toEqual({ slots: [1, null], focus: 0 });
  });

  it("clears everything when the last session closes", () => {
    expect(reconcileSlots({ slots: [1, 2], focus: 0 }, "pair", [], null)).toEqual({
      slots: [null, null],
      focus: 0,
    });
  });

  it("fills again when the mode changes", () => {
    expect(reconcileSlots(grid, "pair", [1, 2, 3, 4], 4)).toEqual({
      slots: [4, 1],
      focus: 0,
    });
    expect(reconcileSlots({ slots: [2], focus: 0 }, "quad", [1, 2, 3], 2)).toEqual({
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
    let split = fillSlots("pair", [1, 2, 3], 1);
    expect(split).toEqual({ slots: [1, 2], focus: 0 });
    const ids = [1, 2, 3];
    const step = () => {
      state = sessionsReducer(state, { type: "cycled", step: 1 });
      split = reconcileSlots(split, "pair", ids, state.activeId);
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
  it("picks the smallest layout that fits", () => {
    expect(showSessions([4, 5], "single")).toEqual({
      layout: "pair",
      split: { slots: [4, 5], focus: 0 },
    });
    expect(showSessions([4, 5, 6], "single")).toEqual({
      layout: "triple",
      split: { slots: [4, 5, 6], focus: 0 },
    });
    expect(showSessions([4, 5, 6, 7], "pair").layout).toBe("quad");
  });

  it("shows at most four and ignores duplicates", () => {
    expect(showSessions([1, 2, 2, 3, 4, 5], "pair").split.slots).toEqual([1, 2, 3, 4]);
  });

  it("keeps the layout for a single session", () => {
    expect(showSessions([9], "mainStack").layout).toBe("mainStack");
  });
});

describe("layouts with the same number of zones", () => {
  it("keep every session in its pane", () => {
    const three: SplitSlots = { slots: [3, 1, 2], focus: 1 };
    expect(reconcileSlots(three, "mainStack", [1, 2, 3, 4], 1)).toBe(three);
  });
});

describe("placeSession", () => {
  const ids = [1, 2, 3, 4, 5];

  it("puts a hidden session into a zone of the current layout", () => {
    expect(placeSession({ slots: [1, 2], focus: 0 }, "pair", 1, 5, ids)).toEqual({
      slots: [1, 5],
      focus: 1,
    });
  });

  it("swaps a visible session with the zone's session", () => {
    expect(placeSession({ slots: [1, 2, 3, 4], focus: 0 }, "quad", 3, 1, ids)).toEqual({
      slots: [4, 2, 3, 1],
      focus: 3,
    });
  });

  it("keeps the panes when switching to a layout with as many zones", () => {
    expect(placeSession({ slots: [1, 2, 3], focus: 0 }, "mainStack", 0, 5, ids)).toEqual({
      slots: [5, 2, 3],
      focus: 0,
    });
  });

  it("fills the other zones with the visible sessions first, then hidden ones", () => {
    expect(placeSession({ slots: [4, 2], focus: 0 }, "quad", 2, 5, ids)).toEqual({
      slots: [4, 2, 5, 1],
      focus: 2,
    });
    expect(placeSession({ slots: [3], focus: 0 }, "pair", 1, 2, ids)).toEqual({
      slots: [3, 2],
      focus: 1,
    });
  });

  it("goes back to a single pane with just the session", () => {
    expect(placeSession({ slots: [1, 2, 3, 4], focus: 2 }, "single", 0, 3, ids)).toEqual({
      slots: [3],
      focus: 0,
    });
  });

  it("leaves zones empty without enough sessions and drops closed ones", () => {
    expect(placeSession({ slots: [1, 9], focus: 0 }, "triple", 2, 2, [1, 2])).toEqual({
      slots: [1, null, 2],
      focus: 2,
    });
    expect(placeSession({ slots: [9, 2], focus: 0 }, "pair", 1, 1, [1, 2])).toEqual({
      slots: [2, 1],
      focus: 1,
    });
  });

  it("clamps an unknown zone", () => {
    expect(placeSession({ slots: [1, 2], focus: 0 }, "pair", 7, 3, ids).focus).toBe(1);
  });

  it("survives the next reconcile unchanged once the session is active", () => {
    const placed = placeSession({ slots: [1, 2], focus: 0 }, "quad", 3, 5, ids);
    expect(reconcileSlots(placed, "quad", ids, 5)).toBe(placed);
  });
});

describe("helpers", () => {
  it("finds the pane of a session", () => {
    expect(slotOf({ slots: [1, null, 3], focus: 0 }, 3)).toBe(2);
    expect(slotOf({ slots: [1], focus: 0 }, 3)).toBeNull();
  });
});
