// "1 waiting" at the edge of the sidebar's session list: a waiting session scrolled out of view
// should not go unnoticed. The rows report where they are (IntersectionObserver); this module
// decides what the hint above and below the list says and where a click scrolls to.

export type RowPlace = "above" | "visible" | "below";

/**
 * Where a row is relative to the visible part of the list. A row counts as visible while at
 * least half of it shows; a row cut in half at the edge is easy to miss.
 */
export function rowPlace(
  row: { top: number; bottom: number },
  view: { top: number; bottom: number },
): RowPlace {
  const height = Math.max(1, row.bottom - row.top);
  const shown = Math.min(row.bottom, view.bottom) - Math.max(row.top, view.top);
  if (shown >= height / 2) return "visible";
  return row.top + height / 2 < view.top ? "above" : "below";
}

export interface EdgeHint {
  count: number;
  /** The waiting session nearest to the visible part: a click scrolls to it. */
  targetId: number;
}

export interface WaitingHints {
  above: EdgeHint | null;
  below: EdgeHint | null;
}

/**
 * Waiting sessions out of view, per edge. `order`: the rows top to bottom. Rows without a known
 * place (not measured yet) count as visible, so the hint never flashes up on first render.
 */
export function waitingHints(
  order: readonly number[],
  waiting: ReadonlySet<number>,
  places: ReadonlyMap<number, RowPlace>,
): WaitingHints {
  const above = order.filter((id) => waiting.has(id) && places.get(id) === "above");
  const below = order.filter((id) => waiting.has(id) && places.get(id) === "below");
  const nearestAbove = above[above.length - 1];
  const nearestBelow = below[0];
  return {
    above: nearestAbove === undefined ? null : { count: above.length, targetId: nearestAbove },
    below: nearestBelow === undefined ? null : { count: below.length, targetId: nearestBelow },
  };
}
