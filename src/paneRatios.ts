// Resizable zones: where the splitters of a snap layout sit (snapLayouts.ts turns them into zones),
// how far each may move, keyboard steps, reset and parsing of the stored values. Pure logic; the
// pointer plumbing lives in PaneSplitters.tsx.

import { DEFAULT_RATIOS, LAYOUT_IDS, type LayoutId, type Ratios } from "./snapLayouts";

/** Splitter positions per layout; a missing layout uses its preset. */
export type PaneRatios = Partial<Record<LayoutId, Ratios>>;

/** Every pane keeps at least this share of the panes area along the splitter's axis. */
export const MIN_PANE_SHARE = 0.15;
/** ... and at least this many pixels, so a terminal never shrinks to a handful of columns. */
export const MIN_PANE_PX = { width: 240, height: 120 } as const;
/** One arrow key press moves a splitter by this share of the panes area. */
export const KEY_STEP_SHARE = 0.025;

/** Size of the panes area in px; without one, only the share applies. */
export interface AreaSize {
  width: number;
  height: number;
}

/** A splitter between zones, in fractions of the panes area. */
export interface ZoneSplitter {
  /** Which ratio it moves. */
  index: number;
  /** `vertical`: the line stands upright and moves left and right. */
  orientation: "vertical" | "horizontal";
  /** Position on its axis (x for vertical, y for horizontal). */
  at: number;
  /** Start and end on the other axis. */
  from: number;
  to: number;
}

/** Stored ratios of a layout, or its preset when none (or no usable ones) are stored. */
export function ratiosFor(all: PaneRatios, layout: LayoutId): Ratios {
  const stored = all[layout];
  const fallback = DEFAULT_RATIOS[layout];
  if (stored === undefined || stored.length !== fallback.length) return fallback;
  return stored;
}

export function withRatios(all: PaneRatios, layout: LayoutId, ratios: Ratios): PaneRatios {
  return { ...all, [layout]: ratios };
}

/** The layout goes back to its preset (nothing stored for it). */
export function withoutRatios(all: PaneRatios, layout: LayoutId): PaneRatios {
  const next: PaneRatios = {};
  for (const id of LAYOUT_IDS) {
    const stored = all[id];
    if (id !== layout && stored !== undefined) next[id] = stored;
  }
  return next;
}

export function sameRatios(a: Ratios, b: Ratios): boolean {
  return (
    a.length === b.length && a.every((value, index) => Math.abs(value - (b[index] ?? NaN)) < 1e-9)
  );
}

/** The splitters of a layout at these positions; a single zone has none. */
export function splitters(layout: LayoutId, ratios: Ratios): ZoneSplitter[] {
  const [a = 0.5, b = 0.5] = ratios;
  switch (layout) {
    case "single":
      return [];
    case "pair":
      return [{ index: 0, orientation: "vertical", at: a, from: 0, to: 1 }];
    case "triple":
      return [
        { index: 0, orientation: "vertical", at: a, from: 0, to: 1 },
        { index: 1, orientation: "vertical", at: b, from: 0, to: 1 },
      ];
    case "mainStack":
      return [
        { index: 0, orientation: "vertical", at: a, from: 0, to: 1 },
        { index: 1, orientation: "horizontal", at: b, from: a, to: 1 },
      ];
    case "quad":
      return [
        { index: 0, orientation: "vertical", at: a, from: 0, to: 1 },
        { index: 1, orientation: "horizontal", at: b, from: 0, to: 1 },
      ];
  }
}

function orientationOf(layout: LayoutId, index: number): "vertical" | "horizontal" {
  if ((layout === "mainStack" || layout === "quad") && index === 1) return "horizontal";
  return "vertical";
}

/** Smallest share of a pane along this axis: 15 % or the pixel minimum, whichever is larger. */
export function minShare(orientation: "vertical" | "horizontal", area?: AreaSize): number {
  const size = orientation === "vertical" ? area?.width : area?.height;
  const px = orientation === "vertical" ? MIN_PANE_PX.width : MIN_PANE_PX.height;
  if (size === undefined || !(size > 0)) return MIN_PANE_SHARE;
  return Math.max(MIN_PANE_SHARE, px / size);
}

/**
 * Range a splitter may move in: the panes on both sides keep their minimum. In an area too small
 * for both minimums, the splitter stays in the middle of its neighbors.
 */
export function ratioBounds(
  layout: LayoutId,
  ratios: Ratios,
  index: number,
  area?: AreaSize,
): { min: number; max: number } {
  // Neighbors: the area's edges, or the other splitter of the three columns.
  let low = 0;
  let high = 1;
  if (layout === "triple") {
    if (index === 0) high = ratios[1] ?? 1;
    else low = ratios[0] ?? 0;
  }
  const share = minShare(orientationOf(layout, index), area);
  const min = low + share;
  const max = high - share;
  if (min > max) {
    const middle = (low + high) / 2;
    return { min: middle, max: middle };
  }
  return { min, max };
}

/** Moves one splitter to `value`, clamped to its range; other splitters stay. */
export function setRatio(
  layout: LayoutId,
  ratios: Ratios,
  index: number,
  value: number,
  area?: AreaSize,
): Ratios {
  if (index < 0 || index >= ratios.length) return ratios;
  const { min, max } = ratioBounds(layout, ratios, index, area);
  const next = Number.isFinite(value)
    ? Math.min(Math.max(value, min), max)
    : (ratios[index] ?? min);
  return ratios.map((current, other) => (other === index ? next : current));
}

/**
 * Keyboard on a splitter: the arrows along its axis move it by a step, Home and End to its ends.
 * Other keys: `null`.
 */
export function stepRatio(
  layout: LayoutId,
  ratios: Ratios,
  index: number,
  key: string,
  area?: AreaSize,
): Ratios | null {
  const current = ratios[index];
  if (current === undefined) return null;
  const vertical = orientationOf(layout, index) === "vertical";
  const less = vertical ? "ArrowLeft" : "ArrowUp";
  const more = vertical ? "ArrowRight" : "ArrowDown";
  switch (key) {
    case less:
      return setRatio(layout, ratios, index, current - KEY_STEP_SHARE, area);
    case more:
      return setRatio(layout, ratios, index, current + KEY_STEP_SHARE, area);
    case "Home":
      return setRatio(layout, ratios, index, ratioBounds(layout, ratios, index, area).min, area);
    case "End":
      return setRatio(layout, ratios, index, ratioBounds(layout, ratios, index, area).max, area);
    default:
      return null;
  }
}

/**
 * Stored value to splitter positions: per known layout, a list of the right length whose values
 * keep every pane at its minimum share; anything else is dropped (the preset applies).
 */
export function parsePaneRatios(raw: unknown): PaneRatios {
  if (typeof raw !== "object" || raw === null) return {};
  const parsed: PaneRatios = {};
  for (const layout of LAYOUT_IDS) {
    const value: unknown = Reflect.get(raw, layout);
    const fallback = DEFAULT_RATIOS[layout];
    if (fallback.length === 0 || !Array.isArray(value) || value.length !== fallback.length) {
      continue;
    }
    const numbers = value.filter(
      (entry): entry is number => typeof entry === "number" && Number.isFinite(entry),
    );
    if (numbers.length !== fallback.length) continue;
    // Clamped in order, so the columns of three stay in order and wide enough.
    let ratios: Ratios = numbers;
    for (let index = 0; index < numbers.length; index += 1) {
      ratios = setRatio(layout, ratios, index, numbers[index] ?? NaN);
    }
    const valid = ratios.every((entry, index) => Math.abs(entry - (numbers[index] ?? NaN)) < 1e-9);
    if (valid) parsed[layout] = ratios;
  }
  return parsed;
}
