// The sidebar shows sessions in two groups: ordinary sessions first, then the ones that work in
// their own git worktree. The session list keeps that order itself (a stable partition), so cycling,
// jumping and the collapsed rail follow what the sidebar shows; moving stays within a group.

/** Sidebar groups that hold sessions, top to bottom. */
export const SESSION_GROUPS = ["sessions", "worktrees"] as const;
export type SessionGroup = (typeof SESSION_GROUPS)[number];

export function sessionGroup(session: { worktree?: unknown }): SessionGroup {
  return session.worktree === undefined ? "sessions" : "worktrees";
}

export function isSessionGroup(value: unknown): value is SessionGroup {
  return SESSION_GROUPS.some((group) => group === value);
}

/**
 * Ordinary sessions first, then worktree sessions; within each group the order stays. Returns the
 * same array when it is already grouped, so the reducer keeps its state untouched.
 */
export function groupedOrder<T extends { worktree?: unknown }>(items: T[]): T[] {
  const ordinary = items.filter((item) => sessionGroup(item) === "sessions");
  const own = items.filter((item) => sessionGroup(item) === "worktrees");
  const grouped = [...ordinary, ...own];
  return grouped.every((item, index) => item === items[index]) ? items : grouped;
}

/** Ids of the group a session belongs to, in list order; empty for an unknown id. */
export function groupIds(
  items: readonly { id: number; worktree?: unknown }[],
  id: number,
): number[] {
  const self = items.find((item) => item.id === id);
  if (self === undefined) return [];
  const group = sessionGroup(self);
  return items.filter((item) => sessionGroup(item) === group).map((item) => item.id);
}

/**
 * Alt+Up / Alt+Down: the index (as `moveItem` expects it) that swaps the session with its
 * neighbour in the same group; `null` at the group's edge or for an unknown id.
 */
export function groupStepIndex(
  items: readonly { id: number; worktree?: unknown }[],
  id: number,
  step: 1 | -1,
): number | null {
  const ids = groupIds(items, id);
  const neighbour = ids[ids.indexOf(id) + step];
  if (ids.length === 0 || neighbour === undefined) return null;
  const index = items.findIndex((item) => item.id === neighbour);
  return index < 0 ? null : index;
}

/** Place of a session within its group, counted from 1, for the "moved" announcement. */
export function groupPosition(
  items: readonly { id: number; worktree?: unknown }[],
  id: number,
): { position: number; count: number } | null {
  const ids = groupIds(items, id);
  const index = ids.indexOf(id);
  return index < 0 ? null : { position: index + 1, count: ids.length };
}

/** A row of a session list while dragging: its id, vertical middle and group. */
export interface GroupRow {
  id: number;
  mid: number;
  group: string;
}

/**
 * New place of a dragged session: among the other rows of its own group, the ones whose middle
 * lies above the pointer decide; the result is the index in the whole list (`moveItem`). Rows of
 * the other group never count, so a session cannot land in the wrong group. `rows` are in list
 * order.
 */
export function groupDropIndex(rows: readonly GroupRow[], draggedId: number, y: number): number {
  const dragged = rows.find((row) => row.id === draggedId);
  const others = rows.filter((row) => row.id !== draggedId);
  if (dragged === undefined) return others.filter((row) => row.mid < y).length;
  const mates = others.filter((row) => row.group === dragged.group);
  if (mates.length === 0) return rows.indexOf(dragged);
  const above = mates.filter((row) => row.mid < y).length;
  const before = mates[above];
  if (before !== undefined) return others.indexOf(before);
  const last = mates[mates.length - 1];
  return last === undefined ? rows.indexOf(dragged) : others.indexOf(last) + 1;
}
