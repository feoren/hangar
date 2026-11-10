// Dragging a session from the sidebar: pure rules for when a press becomes a drag, where it would
// land in the list, where the line shows, how fast the list scrolls at its edges, and what a drop
// on the panes area means. The pointer plumbing lives in useSessionDrag.ts.

import type { LayoutId } from "./snapLayouts";

/** A press moves at least this far (px) before it counts as a drag; clicks stay clicks. */
export const DRAG_THRESHOLD = 5;

export function beyondThreshold(dx: number, dy: number, threshold = DRAG_THRESHOLD): boolean {
  return dx * dx + dy * dy >= threshold * threshold;
}

/** Where a drop would land. */
export type DragTarget =
  /** In the session list: the session's new place. */
  | { kind: "list"; index: number }
  /** A zone in the snap layout bar: switch to that layout, the session goes into the zone. */
  | { kind: "snap"; layout: LayoutId; zone: number }
  /** A pane of the current layout: the session takes that pane. */
  | { kind: "pane"; zone: number };

export function sameDragTarget(a: DragTarget | null, b: DragTarget | null): boolean {
  if (a === null || b === null) return a === b;
  switch (a.kind) {
    case "list":
      return b.kind === "list" && a.index === b.index;
    case "snap":
      return b.kind === "snap" && a.layout === b.layout && a.zone === b.zone;
    case "pane":
      return b.kind === "pane" && a.zone === b.zone;
  }
}

/** Row that shows the drop line, and on which edge; `null` when the drop would change nothing. */
export interface DropLine {
  id: number;
  edge: "before" | "after";
}

/**
 * `groupOf` names each session's sidebar group: the line only ever shows on a row of the dragged
 * session's own group (at the group's end it sits below its last row, not above the next group).
 */
export function dropLine(
  order: readonly number[],
  draggedId: number,
  index: number,
  groupOf: (id: number) => string = () => "",
): DropLine | null {
  const from = order.indexOf(draggedId);
  if (from < 0 || index === from) return null;
  const group = groupOf(draggedId);
  const others = order.filter((id) => id !== draggedId);
  const before = others[index];
  if (before !== undefined && groupOf(before) === group) return { id: before, edge: "before" };
  const previous = before === undefined ? others[others.length - 1] : others[index - 1];
  return previous !== undefined && groupOf(previous) === group
    ? { id: previous, edge: "after" }
    : null;
}

/** Within this distance (px) of the list's top or bottom edge, the list scrolls. */
export const AUTOSCROLL_EDGE = 32;
/** Fastest scroll step per frame (px), reached at the edge and beyond it. */
export const AUTOSCROLL_MAX = 14;

/**
 * Scroll step per frame while dragging near a list edge: negative upwards, positive downwards,
 * growing towards the edge; 0 in the middle.
 */
export function autoscrollStep(y: number, top: number, bottom: number): number {
  const edge = Math.min(AUTOSCROLL_EDGE, (bottom - top) / 3);
  if (!(edge > 0)) return 0;
  if (y < top + edge) {
    return -Math.ceil(AUTOSCROLL_MAX * Math.min(1, (top + edge - y) / edge));
  }
  if (y > bottom - edge) {
    return Math.ceil(AUTOSCROLL_MAX * Math.min(1, (y - (bottom - edge)) / edge));
  }
  return 0;
}

/** Where a drop on the panes area puts the session: layout and zone. */
export function dropPlacement(
  target: DragTarget,
  current: LayoutId,
): { layout: LayoutId; zone: number } | null {
  switch (target.kind) {
    case "list":
      return null;
    case "snap":
      return { layout: target.layout, zone: target.zone };
    case "pane":
      return { layout: current, zone: target.zone };
  }
}
