// Split view: one session, two side by side, or four in a grid. Pure logic for which session sits
// in which pane (slot); the sessions reducer still decides which session is active, and the active
// session is always the focused pane.

export const SPLIT_MODES = ["single", "columns", "grid"] as const;
export type SplitMode = (typeof SPLIT_MODES)[number];

export function isSplitMode(value: unknown): value is SplitMode {
  return SPLIT_MODES.some((mode) => mode === value);
}

export function slotCount(mode: SplitMode): number {
  switch (mode) {
    case "single":
      return 1;
    case "columns":
      return 2;
    case "grid":
      return 4;
  }
}

/** Session ID per pane, `null` for an empty pane; `focus` is the pane with the active session. */
export interface SplitSlots {
  slots: readonly (number | null)[];
  focus: number;
}

export const EMPTY_SLOTS: SplitSlots = { slots: [null], focus: 0 };

/**
 * Fills every pane from scratch: the active session first, then the others in sidebar order.
 * Used when the mode changes.
 */
export function fillSlots(
  mode: SplitMode,
  sessionIds: readonly number[],
  activeId: number | null,
): SplitSlots {
  const count = slotCount(mode);
  const order =
    activeId !== null && sessionIds.includes(activeId)
      ? [activeId, ...sessionIds.filter((id) => id !== activeId)]
      : [...sessionIds];
  const slots = Array.from({ length: count }, (_, index) => order[index] ?? null);
  return { slots, focus: 0 };
}

/**
 * Brings the panes in line with the sessions after any change:
 * - another mode: fill again (`fillSlots`);
 * - closed sessions leave their pane;
 * - the active session, if visible, gets the focus; if hidden, it takes the first empty pane or,
 *   without one, replaces the session in the focused pane (like clicking it in the sidebar);
 * - empty panes take hidden sessions in sidebar order, so a pane stays empty only while there are
 *   fewer sessions than panes.
 * Returns the same object when nothing changes, so it can run on every render.
 */
export function reconcileSlots(
  current: SplitSlots,
  mode: SplitMode,
  sessionIds: readonly number[],
  activeId: number | null,
): SplitSlots {
  if (current.slots.length !== slotCount(mode)) return fillSlots(mode, sessionIds, activeId);
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
 * Shows exactly these sessions (after "Send to several"): two side by side, three or four in the
 * grid, at most four. One session keeps the current mode. The first one gets the focus.
 */
export function showSessions(
  ids: readonly number[],
  mode: SplitMode,
): { mode: SplitMode; split: SplitSlots } {
  const unique = ids.filter((id, index) => ids.indexOf(id) === index).slice(0, 4);
  const next: SplitMode = unique.length <= 1 ? mode : unique.length === 2 ? "columns" : "grid";
  const count = slotCount(next);
  const slots = Array.from({ length: count }, (_, index) => unique[index] ?? null);
  return { mode: next, split: { slots, focus: 0 } };
}

/** Position of a pane as fractions of the panes area (left, top, width, height). */
export interface SlotRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export function slotRect(mode: SplitMode, slot: number): SlotRect {
  switch (mode) {
    case "single":
      return { x: 0, y: 0, width: 1, height: 1 };
    case "columns":
      return { x: slot === 1 ? 0.5 : 0, y: 0, width: 0.5, height: 1 };
    case "grid":
      return { x: slot % 2 === 1 ? 0.5 : 0, y: slot >= 2 ? 0.5 : 0, width: 0.5, height: 0.5 };
  }
}
