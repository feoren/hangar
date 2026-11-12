import { describe, expect, it } from "vitest";

import { dropLine } from "./sessionDrag";
import {
  agentGroupKey,
  agentGroupOrder,
  agentGroups,
  groupDropIndex,
  groupedOrder,
  groupIds,
  groupPosition,
  groupStepIndex,
  isSessionGrouping,
  sessionGroupKey,
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

describe("program groups", () => {
  it("key a session by its program only, worktree or not", () => {
    expect(agentGroupKey(plain(1, "codex"))).toBe("program:codex");
    expect(agentGroupKey(own(1, "codex"))).toBe("program:codex");
  });

  it("gather the sessions of one program, programs in order of first appearance", () => {
    const items = [plain(1, "claude"), plain(2, "pwsh"), plain(3, "claude"), plain(4, "codex")];
    expect(groupedOrder(items).map((s) => s.id)).toEqual([1, 3, 2, 4]);
    const groups = agentGroups(groupedOrder(items));
    expect(groups.map((g) => [g.key, g.sessions.map((s) => s.id)])).toEqual([
      ["program:claude", [1, 3]],
      ["program:pwsh", [2]],
      ["program:codex", [4]],
    ]);
  });

  it("put worktree sessions into their program's group, in list order", () => {
    const items = [own(1, "claude"), plain(2, "pwsh"), plain(3, "claude"), own(4, "pwsh")];
    expect(groupedOrder(items).map((s) => s.id)).toEqual([1, 3, 2, 4]);
    expect(agentGroups(items).map((g) => g.sessions.map((s) => s.id))).toEqual([
      [1, 3],
      [2, 4],
    ]);
  });

  it("is stable: an already grouped list stays the same array", () => {
    const items = [plain(1, "claude"), own(3, "claude"), plain(2, "pwsh")];
    expect(groupedOrder(items)).toBe(items);
    const none: Row[] = [];
    expect(groupedOrder(none)).toBe(none);
    expect(groupedOrder(groupedOrder([plain(1, "a"), plain(2, "b"), plain(3, "a")]))).toEqual([
      plain(1, "a"),
      plain(3, "a"),
      plain(2, "b"),
    ]);
  });

  it("list the ids of a session's group", () => {
    const items = [plain(1), own(2), plain(3, "pwsh"), own(4, "pwsh")];
    expect(groupIds(items, 4)).toEqual([3, 4]);
    expect(groupIds(items, 1)).toEqual([1, 2]);
    expect(groupIds(items, 9)).toEqual([]);
  });

  it("step, count and move only within the group, across worktree and plain rows", () => {
    const items = [plain(1, "a"), own(2, "a"), plain(3, "b"), plain(4, "b")];
    expect(groupStepIndex(items, 1, 1)).toBe(1);
    expect(groupStepIndex(items, 2, -1)).toBe(0);
    expect(groupStepIndex(items, 2, 1)).toBeNull();
    expect(groupStepIndex(items, 3, -1)).toBeNull();
    expect(groupStepIndex(items, 4, -1)).toBe(2);
    expect(groupStepIndex(items, 9, 1)).toBeNull();
    const moved = moveItem(items, 2, groupStepIndex(items, 2, -1) ?? -1);
    expect(moved?.map((s) => s.id)).toEqual([2, 1, 3, 4]);
    expect(groupPosition(items, 4)).toEqual({ position: 2, count: 2 });
    expect(groupPosition(items, 1)).toEqual({ position: 1, count: 2 });
    expect(groupPosition(items, 9)).toBeNull();
  });

  it("show no row of a folded group, also not the active one", () => {
    const sessions = [plain(1), plain(2), plain(3)];
    expect(shownSessions(sessions, true).map((s) => s.id)).toEqual([1, 2, 3]);
    // Folding the group of the session on screen must visibly fold it, too.
    expect(shownSessions(sessions, false)).toEqual([]);
    expect(shownSessions([plain(4)], false)).toEqual([]);
  });
});

describe("project groups", () => {
  interface Placed {
    id: number;
    profileId: string;
    cwd: string;
    kind?: "agent" | "update";
    worktree?: { repoRoot: string };
  }
  const at = (id: number, cwd: string, profileId = "claude"): Placed => ({ id, profileId, cwd });

  it("key a session by its project folder, Windows paths without case", () => {
    expect(sessionGroupKey(at(1, "C:\\Src\\Hangar\\"), "project")).toBe("project:c:\\src\\hangar");
    expect(sessionGroupKey(at(1, "/home/ada/x"), "project")).toBe("project:/home/ada/x");
    // By program, the folder plays no part.
    expect(sessionGroupKey(at(1, "/home/ada/x", "codex"))).toBe("program:codex");
  });

  it("put a worktree session into its repository's group", () => {
    const worktree: Placed = {
      ...at(2, "C:\\wt\\hangar-x", "codex"),
      worktree: { repoRoot: "C:\\src\\hangar" },
    };
    expect(sessionGroupKey(worktree, "project")).toBe("project:c:\\src\\hangar");
  });

  it("leave update sessions and sessions without a folder in their program's group", () => {
    expect(sessionGroupKey({ ...at(3, "C:\\Users\\ada"), kind: "update" }, "project")).toBe(
      "program:claude",
    );
    expect(sessionGroupKey(at(4, "  "), "project")).toBe("program:claude");
    expect(sessionGroupKey({ profileId: "pwsh" }, "project")).toBe("program:pwsh");
  });

  it("gather the sessions of one folder, mixed programs, in order of first appearance", () => {
    const items = [
      at(1, "C:\\a", "claude"),
      at(2, "C:\\b", "pwsh"),
      at(3, "c:\\A\\", "codex"),
      at(4, "C:\\b", "claude"),
    ];
    expect(groupedOrder(items, "project").map((s) => s.id)).toEqual([1, 3, 2, 4]);
    const groups = agentGroups(groupedOrder(items, "project"), "project");
    expect(groups.map((g) => [g.key, g.kind, g.path, g.sessions.map((s) => s.id)])).toEqual([
      ["project:c:\\a", "project", "C:\\a", [1, 3]],
      ["project:c:\\b", "project", "C:\\b", [2, 4]],
    ]);
    // Program groups carry no folder.
    expect(agentGroups(items).map((g) => [g.kind, g.path])).toEqual([
      ["program", null],
      ["program", null],
      ["program", null],
    ]);
  });

  it("step and count within the project group", () => {
    const items = [at(1, "/a", "x"), at(2, "/a", "y"), at(3, "/b", "x")];
    expect(groupStepIndex(items, 1, 1, "project")).toBe(1);
    expect(groupStepIndex(items, 2, 1, "project")).toBeNull();
    // By program, 1 and 3 share a group although 2 sits between them in this list.
    expect(groupIds(items, 1)).toEqual([1, 3]);
    expect(groupIds(items, 1, "project")).toEqual([1, 2]);
    expect(groupPosition(items, 2, "project")).toEqual({ position: 2, count: 2 });
    expect(agentGroupOrder(items, "project")).toEqual(["project:/a", "project:/b"]);
  });

  it("read the setting's values", () => {
    expect(isSessionGrouping("program")).toBe(true);
    expect(isSessionGrouping("project")).toBe(true);
    expect(isSessionGrouping("folder")).toBe(false);
    expect(isSessionGrouping(null)).toBe(false);
  });
});

describe("dragging within a group", () => {
  // Rows 20 px apart: 1, 2 (one program), then 3, 4 (another).
  const rows = [
    { id: 1, mid: 10, group: "program:claude" },
    { id: 2, mid: 30, group: "program:claude" },
    { id: 3, mid: 50, group: "program:pwsh" },
    { id: 4, mid: 70, group: "program:pwsh" },
  ];

  it("ignores the rows of the other group", () => {
    // Session 1 dragged far below: the end of its own group, never among the other program's.
    expect(groupDropIndex(rows, 1, 200)).toBe(1);
    expect(groupDropIndex(rows, 1, 0)).toBe(0);
    // A session of the second group dragged to the top lands at its group's start.
    expect(groupDropIndex(rows, 4, 0)).toBe(2);
    expect(groupDropIndex(rows, 3, 200)).toBe(3);
  });

  it("stays put when the group has no other row", () => {
    const alone = [
      { id: 1, mid: 10, group: "program:claude" },
      { id: 3, mid: 30, group: "program:pwsh" },
    ];
    expect(groupDropIndex(alone, 3, 0)).toBe(1);
  });

  it("counts in the full order when a folded group hides rows", () => {
    // Order 1, 2, 3 (one program, folded: no rows shown), then 4, 5, 6 (another, open).
    const order = [1, 2, 3, 4, 5, 6];
    const shown = [
      { id: 4, mid: 10, group: "program:codex" },
      { id: 5, mid: 30, group: "program:codex" },
      { id: 6, mid: 50, group: "program:codex" },
    ];
    // Session 6 dropped just above 5: before 5 in the full order, not before the hidden 2.
    expect(groupDropIndex(shown, 6, 25, order)).toBe(4);
    // Dropped above everything: its own group's start, after the hidden rows.
    expect(groupDropIndex(shown, 6, 0, order)).toBe(3);
    // Dropped below everything: its own group's end.
    expect(groupDropIndex(shown, 4, 200, order)).toBe(5);
  });

  it("leaves a lone shown row where it is", () => {
    // Only 4 of 3, 4, 5 on screen: no other row of its group to move past.
    const order = [1, 2, 3, 4, 5];
    const shown = [
      { id: 1, mid: 10, group: "program:claude" },
      { id: 2, mid: 30, group: "program:claude" },
      { id: 4, mid: 50, group: "program:codex" },
    ];
    expect(groupDropIndex(shown, 4, 0, order)).toBe(3);
    expect(groupDropIndex(shown, 4, 200, order)).toBe(3);
  });

  it("shows the drop line only on rows of the same group", () => {
    const order = [1, 2, 3, 4];
    const groupOf = (id: number) => (id <= 2 ? "program:claude" : "program:pwsh");
    // Session 1 to the end of its group: below 2, not above 3 of the next group.
    expect(dropLine(order, 1, 1, groupOf)).toEqual({ id: 2, edge: "after" });
    expect(dropLine(order, 4, 2, groupOf)).toEqual({ id: 3, edge: "before" });
    expect(dropLine(order, 3, 3, groupOf)).toEqual({ id: 4, edge: "after" });
    // Without groups the old rule holds.
    expect(dropLine(order, 1, 1)).toEqual({ id: 3, edge: "before" });
  });
});
