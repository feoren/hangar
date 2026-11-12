// The sidebar gathers sessions in groups, in the order the groups first appear. By default one
// group per program (profile); the person can switch to one group per project folder instead
// (`SessionGrouping`). Sessions in their own git worktree sit in their program's group, and in
// their repository's project group, like any other; their row names the worktree. The session
// list keeps that order itself (a stable partition), so cycling, jumping and the collapsed rail
// follow what the sidebar shows; moving stays within a group.

import { folderKey } from "./folders";
import { projectFolder } from "./startTarget";

/** How the sidebar groups the sessions: one group per program or per project folder. */
export type SessionGrouping = "program" | "project";

export const SESSION_GROUPINGS: readonly SessionGrouping[] = ["program", "project"];

export function isSessionGrouping(value: unknown): value is SessionGrouping {
  return value === "program" || value === "project";
}

/** What decides a session's place in the sidebar: its program, or its project folder. */
export interface Groupable {
  profileId: string;
  cwd?: string;
  kind?: "agent" | "update" | undefined;
  worktree?: { repoRoot: string } | undefined;
}

/** Prefix of a program group's key; it sets keys apart from the older "sessions:" ones. */
export const AGENT_GROUP_PREFIX = "program:";
/** Prefix of a project group's key; the rest is the folder's `folderKey`. */
export const PROJECT_GROUP_PREFIX = "project:";

/** Key of a session's program group, the same for every session of one program. */
export function agentGroupKey(item: Groupable): string {
  return `${AGENT_GROUP_PREFIX}${item.profileId}`;
}

/**
 * The project folder a session belongs to in project grouping, `null` for none: update sessions
 * run in the home folder on purpose and keep their program's group.
 */
export function groupProject(item: Groupable): string | null {
  if (item.cwd === undefined) return null;
  return projectFolder({ cwd: item.cwd, kind: item.kind, worktree: item.worktree });
}

/** Key of a session's group under `by`. */
export function sessionGroupKey(item: Groupable, by: SessionGrouping = "program"): string {
  if (by === "program") return agentGroupKey(item);
  const project = groupProject(item);
  return project === null ? agentGroupKey(item) : `${PROJECT_GROUP_PREFIX}${folderKey(project)}`;
}

/** The sessions of one group, in list order. */
export interface AgentGroup<T> {
  key: string;
  /** A program's sessions, or a project folder's. */
  kind: "program" | "project";
  /** Program of the group's first session (the program itself for a program group). */
  profileId: string;
  /** The project folder as its first session names it; `null` for a program group. */
  path: string | null;
  sessions: T[];
}

/** The groups of one list, in the order they first appear. */
export function agentGroups<T extends Groupable>(
  items: readonly T[],
  by: SessionGrouping = "program",
): AgentGroup<T>[] {
  const groups = new Map<string, AgentGroup<T>>();
  for (const item of items) {
    const key = sessionGroupKey(item, by);
    const group = groups.get(key);
    if (group !== undefined) {
      group.sessions.push(item);
      continue;
    }
    const project = key.startsWith(PROJECT_GROUP_PREFIX) ? groupProject(item) : null;
    groups.set(key, {
      key,
      kind: project === null ? "program" : "project",
      profileId: item.profileId,
      path: project,
      sessions: [item],
    });
  }
  return [...groups.values()];
}

/**
 * Rows a group shows: all of them when open, none when folded. The active session folds away,
 * too; its group's header marks that it holds it, so the fold always does what it says.
 */
export function shownSessions<T>(sessions: readonly T[], open: boolean): T[] {
  return open ? [...sessions] : [];
}

/** Group keys of a list in order of appearance, one each. */
export function agentGroupOrder(
  items: readonly Groupable[],
  by: SessionGrouping = "program",
): string[] {
  return [...new Set(items.map((item) => sessionGroupKey(item, by)))];
}

/**
 * The sessions of one group together, groups in order of first appearance, sessions in list
 * order. Returns the same array when it is already grouped, so the reducer keeps its state
 * untouched.
 */
export function groupedOrder<T extends Groupable>(
  items: T[],
  by: SessionGrouping = "program",
): T[] {
  const grouped = agentGroups(items, by).flatMap((group) => group.sessions);
  return grouped.every((item, index) => item === items[index]) ? items : grouped;
}

/** Ids of the group a session belongs to, in list order; empty for an unknown id. */
export function groupIds(
  items: readonly ({ id: number } & Groupable)[],
  id: number,
  by: SessionGrouping = "program",
): number[] {
  const self = items.find((item) => item.id === id);
  if (self === undefined) return [];
  const key = sessionGroupKey(self, by);
  return items.filter((item) => sessionGroupKey(item, by) === key).map((item) => item.id);
}

/**
 * Alt+Up / Alt+Down: the index (as `moveItem` expects it) that swaps the session with its
 * neighbour in the same group; `null` at the group's edge or for an unknown id.
 */
export function groupStepIndex(
  items: readonly ({ id: number } & Groupable)[],
  id: number,
  step: 1 | -1,
  by: SessionGrouping = "program",
): number | null {
  const ids = groupIds(items, id, by);
  const neighbour = ids[ids.indexOf(id) + step];
  if (ids.length === 0 || neighbour === undefined) return null;
  const index = items.findIndex((item) => item.id === neighbour);
  return index < 0 ? null : index;
}

/** Place of a session within its group, counted from 1, for the "moved" announcement. */
export function groupPosition(
  items: readonly ({ id: number } & Groupable)[],
  id: number,
  by: SessionGrouping = "program",
): { position: number; count: number } | null {
  const ids = groupIds(items, id, by);
  const index = ids.indexOf(id);
  return index < 0 ? null : { position: index + 1, count: ids.length };
}

/** A row of a session list while dragging: its id, vertical middle and group key. */
export interface GroupRow {
  id: number;
  mid: number;
  group: string;
}

/**
 * New place of a dragged session: among the other rows of its own group, the ones whose middle
 * lies above the pointer decide; the result is the index in the whole list (`moveItem`). Rows of
 * other groups never count, so a session cannot land in the wrong group. `rows` are in list
 * order.
 */
export function groupDropIndex(
  rows: readonly GroupRow[],
  draggedId: number,
  y: number,
  // The whole list's ids: a folded group shows fewer rows than the list holds, so the
  // index is counted here, not among the rows on screen.
  order: readonly number[] = rows.map((row) => row.id),
): number {
  const dragged = rows.find((row) => row.id === draggedId);
  const others = rows.filter((row) => row.id !== draggedId);
  const rest = order.filter((id) => id !== draggedId);
  if (dragged === undefined) return others.filter((row) => row.mid < y).length;
  const stay = order.indexOf(draggedId);
  const mates = others.filter((row) => row.group === dragged.group);
  if (mates.length === 0) return stay;
  const above = mates.filter((row) => row.mid < y).length;
  const before = mates[above];
  if (before !== undefined) return rest.indexOf(before.id);
  const last = mates[mates.length - 1];
  return last === undefined ? stay : rest.indexOf(last.id) + 1;
}
