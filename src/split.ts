// Split view: which session sits in which pane (slot) of the snap layout (snapLayouts.ts; one
// pane per zone). Pure logic; the sessions reducer still decides which session is active, and the
// active session is always the focused pane.

import { layoutForCount, zoneCount, type LayoutId } from "./snapLayouts";

/** Session ID per pane, `null` for an empty pane; `focus` is the pane with the active session. */
export interface SplitSlots {
  slots: readonly (number | null)[];
  focus: number;
}

export const EMPTY_SLOTS: SplitSlots = { slots: [null], focus: 0 };

/**
 * Fills every pane from scratch: the active session first, then the others in sidebar order.
 * Used when the number of zones changes.
 */
export function fillSlots(
  layout: LayoutId,
  sessionIds: readonly number[],
  activeId: number | null,
): SplitSlots {
  const count = zoneCount(layout);
  const order =
    activeId !== null && sessionIds.includes(activeId)
      ? [activeId, ...sessionIds.filter((id) => id !== activeId)]
      : [...sessionIds];
  const slots = Array.from({ length: count }, (_, index) => order[index] ?? null);
  return { slots, focus: 0 };
}

/**
 * Brings the panes in line with the sessions after any change:
 * - a layout with another number of zones: fill again (`fillSlots`); one with the same number
 *   keeps every session in its pane;
 * - closed sessions leave their pane;
 * - the active session, if visible, gets the focus; if hidden, it takes the first empty pane or,
 *   without one, replaces the session in the focused pane (like clicking it in the sidebar);
 * - empty panes take hidden sessions in sidebar order, so a pane stays empty only while there are
 *   fewer sessions than panes.
 * Returns the same object when nothing changes, so it can run on every render.
 */
export function reconcileSlots(
  current: SplitSlots,
  layout: LayoutId,
  sessionIds: readonly number[],
  activeId: number | null,
): SplitSlots {
  if (current.slots.length !== zoneCount(layout)) return fillSlots(layout, sessionIds, activeId);
  const alive = new Set(sessionIds);
  const slots = current.slots.map((id) => (id !== null && alive.has(id) ? id : null));
  let focus = Math.min(Math.max(current.focus, 0), slots.length - 1);
  if (activeId !== null && alive.has(activeId)) {
    const visible = slots.indexOf(activeId);
    if (visible >= 0) {
      focus = visible;
    } else {
      const empty = slots.indexOf(null);
      focus = empty >= 0 ? empty : focus;
      slots[focus] = activeId;
    }
  }
  const shown = new Set(slots);
  const hidden = sessionIds.filter((id) => !shown.has(id));
  for (let index = 0; index < slots.length && hidden.length > 0; index += 1) {
    if (slots[index] === null) slots[index] = hidden.shift() ?? null;
  }
  return sameSlots(current, { slots, focus }) ? current : { slots, focus };
}

export function sameSlots(a: SplitSlots, b: SplitSlots): boolean {
  return (
    a.focus === b.focus &&
    a.slots.length === b.slots.length &&
    a.slots.every((id, index) => id === b.slots[index])
  );
}

/** Pane of this session, `null` when it is not visible. */
export function slotOf(split: SplitSlots, id: number): number | null {
  const index = split.slots.indexOf(id);
  return index < 0 ? null : index;
}

/**
 * Shows exactly these sessions (after "Send to several") in the smallest layout that fits them,
 * at most four. One session keeps the current layout. The first one gets the focus.
 */
export function showSessions(
  ids: readonly number[],
  layout: LayoutId,
): { layout: LayoutId; split: SplitSlots } {
  const unique = ids.filter((id, index) => ids.indexOf(id) === index).slice(0, 4);
  const next = unique.length <= 1 ? layout : layoutForCount(unique.length);
  const count = zoneCount(next);
  const slots = Array.from({ length: count }, (_, index) => unique[index] ?? null);
  return { layout: next, split: { slots, focus: 0 } };
}

/**
 * Puts a session into one zone of a layout (drop from the sidebar, layout picker) and focuses it.
 * - Same number of zones as now: every other session stays in its pane; if the session was
 *   already visible, it swaps places with the zone's session.
 * - Another number: the zone gets the session, the other zones take the sessions visible so far
 *   in their order.
 * Zones still empty then take hidden sessions in sidebar order, as in `reconcileSlots`.
 */
export function placeSession(
  current: SplitSlots,
  layout: LayoutId,
  zone: number,
  id: number,
  sessionIds: readonly number[],
): SplitSlots {
  const count = zoneCount(layout);
  const target = Math.min(Math.max(Math.trunc(zone), 0), count - 1);
  const alive = new Set(sessionIds);
  const kept = current.slots.map((other) => (other !== null && alive.has(other) ? other : null));
  let slots: (number | null)[];
  if (kept.length === count) {
    slots = [...kept];
    const from = slots.indexOf(id);
    if (from >= 0) slots[from] = slots[target] ?? null;
    slots[target] = id;
  } else {
    const visible = kept.filter((other): other is number => other !== null && other !== id);
    slots = Array.from({ length: count }, (_, index) =>
      index === target ? id : (visible.shift() ?? null),
    );
  }
  const shown = new Set(slots);
  const hidden = sessionIds.filter((other) => !shown.has(other));
  slots = slots.map((other) => other ?? hidden.shift() ?? null);
  return { slots, focus: target };
}
