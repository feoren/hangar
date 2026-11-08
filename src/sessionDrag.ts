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

/** Vertical middle of a row in the list, in window coordinates. */
export interface RowMid {
  id: number;
  mid: number;
}

/**
 * New place of the dragged session: the number of other rows whose middle lies above the
 * pointer. That is the index `moveItem` expects.
 */
export function dropIndex(rows: readonly RowMid[], draggedId: number, y: number): number {
  return rows.filter((row) => row.id !== draggedId && row.mid < y).length;
}

/** Row that shows the drop line, and on which edge; `null` when the drop would change nothing. */
export interface DropLine {
  id: number;
  edge: "before" | "after";
}

export function dropLine(
  order: readonly number[],
  draggedId: number,
  index: number,
): DropLine | null {
  const from = order.indexOf(draggedId);
  if (from < 0 || index === from) return null;
  const others = order.filter((id) => id !== draggedId);
  const before = others[index];
  if (before !== undefined) return { id: before, edge: "before" };
  const last = others[others.length - 1];
  return last === undefined ? null : { id: last, edge: "after" };
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
