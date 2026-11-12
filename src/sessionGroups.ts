// The sidebar gathers sessions per program (profile): one group for each program, in the order
// the programs first appear. Sessions in their own git worktree sit in their program's group like
// any other; their row names the worktree. The session list keeps that order itself (a stable
// partition), so cycling, jumping and the collapsed rail follow what the sidebar shows; moving
// stays within a group.

/** What decides a session's place in the sidebar: its program. */
export interface Groupable {
  profileId: string;
}

/** Prefix of a group key; it sets keys apart from the older "sessions:" and "worktrees:" ones. */
export const AGENT_GROUP_PREFIX = "program:";

/** Key of a session's group, the same for every session of one program. */
export function agentGroupKey(item: Groupable): string {
  return `${AGENT_GROUP_PREFIX}${item.profileId}`;
}

/** The sessions of one program, in list order. */
export interface AgentGroup<T> {
  key: string;
  profileId: string;
  sessions: T[];
}

/** Agent groups of one list, in the order their programs first appear. */
export function agentGroups<T extends Groupable>(items: readonly T[]): AgentGroup<T>[] {
  const groups = new Map<string, AgentGroup<T>>();
  for (const item of items) {
    const key = agentGroupKey(item);
    const group = groups.get(key);
    if (group === undefined) groups.set(key, { key, profileId: item.profileId, sessions: [item] });
    else group.sessions.push(item);
  }
  return [...groups.values()];
}

/**
 * Rows an agent group shows: all of them when open, none when folded. The active session folds
 * away, too; its group's header marks that it holds it, so the fold always does what it says.
 */
export function shownSessions<T>(sessions: readonly T[], open: boolean): T[] {
  return open ? [...sessions] : [];
}

/** Agent group keys of a list in order of appearance, one each. */
export function agentGroupOrder(items: readonly Groupable[]): string[] {
  return [...new Set(items.map(agentGroupKey))];
}

/**
 * The sessions of one program together, programs in order of first appearance, sessions in list
 * order. Returns the same array when it is already grouped, so the reducer keeps its state
 * untouched.
 */
export function groupedOrder<T extends Groupable>(items: T[]): T[] {
  const grouped = agentGroups(items).flatMap((group) => group.sessions);
  return grouped.every((item, index) => item === items[index]) ? items : grouped;
}

/** Ids of the agent group a session belongs to, in list order; empty for an unknown id. */
export function groupIds(items: readonly ({ id: number } & Groupable)[], id: number): number[] {
  const self = items.find((item) => item.id === id);
  if (self === undefined) return [];
  const key = agentGroupKey(self);
  return items.filter((item) => agentGroupKey(item) === key).map((item) => item.id);
}

/**
 * Alt+Up / Alt+Down: the index (as `moveItem` expects it) that swaps the session with its
 * neighbour in the same agent group; `null` at the group's edge or for an unknown id.
 */
export function groupStepIndex(
  items: readonly ({ id: number } & Groupable)[],
  id: number,
  step: 1 | -1,
): number | null {
  const ids = groupIds(items, id);
  const neighbour = ids[ids.indexOf(id) + step];
  if (ids.length === 0 || neighbour === undefined) return null;
  const index = items.findIndex((item) => item.id === neighbour);
  return index < 0 ? null : index;
}

/** Place of a session within its agent group, counted from 1, for the "moved" announcement. */
export function groupPosition(
  items: readonly ({ id: number } & Groupable)[],
  id: number,
): { position: number; count: number } | null {
  const ids = groupIds(items, id);
  const index = ids.indexOf(id);
  return index < 0 ? null : { position: index + 1, count: ids.length };
}

/** A row of a session list while dragging: its id, vertical middle and agent group key. */
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
