// Snap layouts: the arrangements of the panes area, like the snap layouts of Windows 11. Each
// layout is a list of zones (fractions of the panes area); one zone holds one session. Pure
// definitions and geometry, so the picker, the panes and the drop targets share one source.

import type { MessageKey, Translate } from "./i18n";

export const LAYOUT_IDS = ["single", "pair", "triple", "mainStack", "quad"] as const;
export type LayoutId = (typeof LAYOUT_IDS)[number];

/** Position of a zone as fractions of the panes area (left, top, width, height). */
export interface Zone {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** Where a zone sits, for its name in the UI ("left", "top right"). */
export type ZonePlace =
  "full" | "left" | "center" | "right" | "topLeft" | "topRight" | "bottomLeft" | "bottomRight";

interface PlacedZone extends Zone {
  place: ZonePlace;
}

const THIRD = 1 / 3;

/** Zones in reading order (left to right, top to bottom); the first one is the main zone. */
const ZONES: Record<LayoutId, readonly PlacedZone[]> = {
  single: [{ place: "full", x: 0, y: 0, width: 1, height: 1 }],
  pair: [
    { place: "left", x: 0, y: 0, width: 0.5, height: 1 },
    { place: "right", x: 0.5, y: 0, width: 0.5, height: 1 },
  ],
  triple: [
    { place: "left", x: 0, y: 0, width: THIRD, height: 1 },
    { place: "center", x: THIRD, y: 0, width: THIRD, height: 1 },
    { place: "right", x: 2 * THIRD, y: 0, width: 1 - 2 * THIRD, height: 1 },
  ],
  mainStack: [
    { place: "left", x: 0, y: 0, width: 0.6, height: 1 },
    { place: "topRight", x: 0.6, y: 0, width: 0.4, height: 0.5 },
    { place: "bottomRight", x: 0.6, y: 0.5, width: 0.4, height: 0.5 },
  ],
  quad: [
    { place: "topLeft", x: 0, y: 0, width: 0.5, height: 0.5 },
    { place: "topRight", x: 0.5, y: 0, width: 0.5, height: 0.5 },
    { place: "bottomLeft", x: 0, y: 0.5, width: 0.5, height: 0.5 },
    { place: "bottomRight", x: 0.5, y: 0.5, width: 0.5, height: 0.5 },
  ],
};

export function isLayoutId(value: unknown): value is LayoutId {
  return LAYOUT_IDS.some((id) => id === value);
}

/**
 * Stored value to layout: a layout ID as it is, the split modes of 0.2 ("columns", "grid")
 * migrate to their layout, anything else is `null`.
 */
export function migrateLayoutId(value: unknown): LayoutId | null {
  if (isLayoutId(value)) return value;
  if (value === "columns") return "pair";
  if (value === "grid") return "quad";
  return null;
}

export function zones(layout: LayoutId): readonly Zone[] {
  return ZONES[layout];
}

export function zoneCount(layout: LayoutId): number {
  return ZONES[layout].length;
}

/** Zone of a layout; out of range falls back to the whole area. */
export function zoneRect(layout: LayoutId, zone: number): Zone {
  const found = ZONES[layout][zone];
  if (!found) return { x: 0, y: 0, width: 1, height: 1 };
  return { x: found.x, y: found.y, width: found.width, height: found.height };
}

export function zonePlace(layout: LayoutId, zone: number): ZonePlace {
  return ZONES[layout][zone]?.place ?? "full";
}

/** Zone under a point given as fractions of the panes area; `null` outside of it. */
export function zoneAt(layout: LayoutId, fx: number, fy: number): number | null {
  if (!(fx >= 0 && fx <= 1 && fy >= 0 && fy <= 1)) return null;
  const index = ZONES[layout].findIndex(
    (zone) =>
      fx >= zone.x && fx <= zone.x + zone.width && fy >= zone.y && fy <= zone.y + zone.height,
  );
  return index < 0 ? null : index;
}

/** Smallest layout for this many sessions (after "Send to several"), at most four. */
export function layoutForCount(count: number): LayoutId {
  if (count <= 1) return "single";
  if (count === 2) return "pair";
  if (count === 3) return "triple";
  return "quad";
}

/** One zone of one layout: a target in the layout picker. */
export interface SnapTarget {
  layout: LayoutId;
  zone: number;
}

/** Every zone of every layout in picker order: layouts left to right, zones in reading order. */
export const SNAP_TARGETS: readonly SnapTarget[] = LAYOUT_IDS.flatMap((layout) =>
  ZONES[layout].map((_, zone) => ({ layout, zone })),
);

export function sameTarget(a: SnapTarget | null, b: SnapTarget | null): boolean {
  return a !== null && b !== null && a.layout === b.layout && a.zone === b.zone;
}

/**
 * Keyboard in the picker: Right and Down go to the next zone, Left and Up to the previous one,
 * across layouts and without wrapping; Home and End jump to the ends. Other keys: `null`.
 */
export function stepTarget(index: number, key: string, count = SNAP_TARGETS.length): number | null {
  const last = count - 1;
  const clampIndex = (value: number) => Math.min(Math.max(value, 0), last);
  switch (key) {
    case "ArrowRight":
    case "ArrowDown":
      return clampIndex(index + 1);
    case "ArrowLeft":
    case "ArrowUp":
      return clampIndex(index - 1);
    case "Home":
      return 0;
    case "End":
      return last;
    default:
      return null;
  }
}

/** Index in SNAP_TARGETS, `-1` when unknown. */
export function targetIndex(target: SnapTarget): number {
  return SNAP_TARGETS.findIndex((other) => sameTarget(other, target));
}

export const LAYOUT_NAMES: Record<LayoutId, MessageKey> = {
  single: "layout.single",
  pair: "layout.pair",
  triple: "layout.triple",
  mainStack: "layout.mainStack",
  quad: "layout.quad",
};

const PLACE_NAMES: Record<ZonePlace, MessageKey> = {
  full: "zone.full",
  left: "zone.left",
  center: "zone.center",
  right: "zone.right",
  topLeft: "zone.topLeft",
  topRight: "zone.topRight",
  bottomLeft: "zone.bottomLeft",
  bottomRight: "zone.bottomRight",
};

/** Name of a zone for screen readers and tooltips: "Two side by side, right". */
export function zoneLabel(t: Translate, layout: LayoutId, zone: number): string {
  return t("snap.zoneLabel", {
    layout: t(LAYOUT_NAMES[layout]),
    zone: t(PLACE_NAMES[zonePlace(layout, zone)]),
  });
}
