import { describe, expect, it } from "vitest";

import { dropLine } from "./sessionDrag";
import {
  agentGroupKey,
  agentGroups,
  groupDropIndex,
  groupedOrder,
  groupIds,
  groupPosition,
  groupStepIndex,
  isSessionGroup,
  sessionGroup,
  shownSessions,
} from "./sessionGroups";
import { moveItem } from "./sessions";

const tree = { path: "p", branch: "hangar/x", repoRoot: "r" };
interface Row {
  id: number;
  profileId: string;
  worktree?: typeof tree;
}
const plain = (id: number, profileId = "claude"): Row => ({ id, profileId });
const own = (id: number, profileId = "claude"): Row => ({ id, profileId, worktree: tree });

describe("session groups", () => {
  it("put a session with its own worktree into the worktree group", () => {
    expect(sessionGroup(plain(1))).toBe("sessions");
    expect(sessionGroup(own(2))).toBe("worktrees");
    expect(isSessionGroup("worktrees")).toBe(true);
    expect(isSessionGroup("team")).toBe(false);
  });

  it("order ordinary sessions first and keep the order within each group", () => {
    const items = [own(1), plain(2), own(3), plain(4)];
    expect(groupedOrder(items).map((s) => s.id)).toEqual([2, 4, 1, 3]);
  });

  it("return the same array when it is already grouped", () => {
    const items = [plain(1), plain(2), own(3)];
    expect(groupedOrder(items)).toBe(items);
    const none: Row[] = [];
    expect(groupedOrder(none)).toBe(none);
  });

  it("list the ids of a session's group", () => {
    const items = [plain(1), plain(2), own(3), own(4)];
    expect(groupIds(items, 4)).toEqual([3, 4]);
    expect(groupIds(items, 1)).toEqual([1, 2]);
    expect(groupIds(items, 9)).toEqual([]);
  });

  it("step by keyboard only within the group", () => {
    const items = [plain(1), plain(2), own(3), own(4)];
    expect(groupStepIndex(items, 1, 1)).toBe(1);
    expect(groupStepIndex(items, 2, 1)).toBeNull();
    expect(groupStepIndex(items, 3, -1)).toBeNull();
    expect(groupStepIndex(items, 4, -1)).toBe(2);
    expect(groupStepIndex(items, 9, 1)).toBeNull();
    const moved = moveItem(items, 4, groupStepIndex(items, 4, -1) ?? -1);
    expect(moved?.map((s) => s.id)).toEqual([1, 2, 4, 3]);
  });

  it("count the position within the group", () => {
    const items = [plain(1), plain(2), own(3), own(4)];
    expect(groupPosition(items, 4)).toEqual({ position: 2, count: 2 });
    expect(groupPosition(items, 1)).toEqual({ position: 1, count: 2 });
    expect(groupPosition(items, 9)).toBeNull();
  });
});

describe("agent groups", () => {
  it("key a session by its sidebar group and program", () => {
    expect(agentGroupKey(plain(1, "codex"))).toBe("sessions:codex");
    expect(agentGroupKey(own(1, "codex"))).toBe("worktrees:codex");
  });

  it("gather the sessions of one program, programs in order of first appearance", () => {
    const items = [plain(1, "claude"), plain(2, "pwsh"), plain(3, "claude"), plain(4, "codex")];
    expect(groupedOrder(items).map((s) => s.id)).toEqual([1, 3, 2, 4]);
    const groups = agentGroups(groupedOrder(items));
    expect(groups.map((g) => [g.key, g.sessions.map((s) => s.id)])).toEqual([
      ["sessions:claude", [1, 3]],
      ["sessions:pwsh", [2]],
      ["sessions:codex", [4]],
    ]);
  });

  it("keep the same program apart in the worktree group", () => {
    const items = [own(1, "claude"), plain(2, "claude"), own(3, "pwsh"), plain(4, "pwsh")];
    expect(groupedOrder(items).map((s) => s.id)).toEqual([2, 4, 1, 3]);
  });

  it("is stable: an already grouped list stays the same array", () => {
    const items = [plain(1, "claude"), plain(3, "claude"), plain(2, "pwsh")];
    expect(groupedOrder(items)).toBe(items);
    expect(groupedOrder(groupedOrder([plain(1, "a"), plain(2, "b"), plain(3, "a")]))).toEqual([
      plain(1, "a"),
      plain(3, "a"),
      plain(2, "b"),
    ]);
  });

  it("step, count and move only within the agent group", () => {
    const items = [plain(1, "a"), plain(2, "a"), plain(3, "b"), plain(4, "b")];
    expect(groupStepIndex(items, 2, 1)).toBeNull();
    expect(groupStepIndex(items, 3, -1)).toBeNull();
    expect(groupStepIndex(items, 4, -1)).toBe(2);
    expect(groupIds(items, 3)).toEqual([3, 4]);
    expect(groupPosition(items, 4)).toEqual({ position: 2, count: 2 });
  });

  it("show no row of a folded group, also not the active one", () => {
    const sessions = [plain(1), plain(2), plain(3)];
    expect(shownSessions(sessions, true).map((s) => s.id)).toEqual([1, 2, 3]);
    // Folding the group of the session on screen must visibly fold it, too.
    expect(shownSessions(sessions, false)).toEqual([]);
    expect(shownSessions([plain(4)], false)).toEqual([]);
  });
});

describe("dragging within a group", () => {
  // Rows 20 px apart: 1, 2 (sessions), then 3, 4 (worktrees).
  const rows = [
    { id: 1, mid: 10, group: "sessions" },
    { id: 2, mid: 30, group: "sessions" },
    { id: 3, mid: 50, group: "worktrees" },
    { id: 4, mid: 70, group: "worktrees" },
  ];

  it("ignores the rows of the other group", () => {
    // Session 1 dragged far below: the end of its own group, never among the worktrees.
    expect(groupDropIndex(rows, 1, 200)).toBe(1);
    expect(groupDropIndex(rows, 1, 0)).toBe(0);
    // A worktree session dragged to the top lands at its group's start.
    expect(groupDropIndex(rows, 4, 0)).toBe(2);
    expect(groupDropIndex(rows, 3, 200)).toBe(3);
  });

  it("stays put when the group has no other row", () => {
    const alone = [
      { id: 1, mid: 10, group: "sessions" },
      { id: 3, mid: 30, group: "worktrees" },
    ];
    expect(groupDropIndex(alone, 3, 0)).toBe(1);
  });

  it("counts in the full order when a folded agent group hides rows", () => {
    // Order 1, 2, 3 (one program, folded: no rows shown), then 4, 5, 6 (another, open).
    const order = [1, 2, 3, 4, 5, 6];
    const shown = [
      { id: 4, mid: 10, group: "sessions:codex" },
      { id: 5, mid: 30, group: "sessions:codex" },
      { id: 6, mid: 50, group: "sessions:codex" },
    ];
    // Session 6 dropped just above 5: before 5 in the full order, not before the hidden 2.
    expect(groupDropIndex(shown, 6, 25, order)).toBe(4);
    // Dropped above everything: its own group's start, after the hidden rows.
    expect(groupDropIndex(shown, 6, 0, order)).toBe(3);
    // Dropped below everything: its own group's end.
    expect(groupDropIndex(shown, 4, 200, order)).toBe(5);
  });

  it("leaves the active row of a folded agent group where it is", () => {
    // 1 and 2 open; 3, 4, 5 folded with only the active 4 shown.
    const order = [1, 2, 3, 4, 5];
    const shown = [
      { id: 1, mid: 10, group: "sessions:claude" },
      { id: 2, mid: 30, group: "sessions:claude" },
      { id: 4, mid: 50, group: "sessions:codex" },
    ];
    expect(groupDropIndex(shown, 4, 0, order)).toBe(3);
    expect(groupDropIndex(shown, 4, 200, order)).toBe(3);
  });

  it("shows the drop line only on rows of the same group", () => {
    const order = [1, 2, 3, 4];
    const groupOf = (id: number) => (id <= 2 ? "sessions" : "worktrees");
    // Session 1 to the end of its group: below 2, not above worktree 3.
    expect(dropLine(order, 1, 1, groupOf)).toEqual({ id: 2, edge: "after" });
    expect(dropLine(order, 4, 2, groupOf)).toEqual({ id: 3, edge: "before" });
    expect(dropLine(order, 3, 3, groupOf)).toEqual({ id: 4, edge: "after" });
    // Without groups the old rule holds.
    expect(dropLine(order, 1, 1)).toEqual({ id: 3, edge: "before" });
  });
});
