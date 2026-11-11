// The small layout glyph of a sidebar row: the current snap layout as a few rectangles on a
// pixel grid, with the session's own zone marked. Pure geometry, so the sizes stay crisp and
// testable; LayoutGlyph.tsx draws it.

import type { Translate } from "./i18n";
import { placeName, zones, type LayoutId, type Ratios } from "./snapLayouts";

/** Where a session is shown: its zone in the current layout at the current splitter positions. */
export interface PanePlacement {
  layout: Exclude<LayoutId, "single">;
  zone: number;
  ratios: Ratios;
}

export interface GlyphRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export const GLYPH_SIZE = { width: 14, height: 10 } as const;

/**
 * Zones as whole pixels inside a glyph of `width` x `height`: zone edges snap to the pixel grid
 * and a 1 px gap separates neighbors. Every zone keeps at least 1 px on each axis.
 */
export function glyphRects(
  layout: LayoutId,
  ratios: Ratios,
  width: number = GLYPH_SIZE.width,
  height: number = GLYPH_SIZE.height,
): GlyphRect[] {
  const gap = 1;
  return zones(layout, ratios).map((zone) => {
    const left = Math.round(zone.x * width);
    const top = Math.round(zone.y * height);
    const right = Math.round((zone.x + zone.width) * width);
    const bottom = Math.round((zone.y + zone.height) * height);
    // The gap comes off the zone's right and bottom edge where a neighbor follows.
    const endX = right >= width ? width : right - gap;
    const endY = bottom >= height ? height : bottom - gap;
    return {
      x: left,
      y: top,
      width: Math.max(1, endX - left),
      height: Math.max(1, endY - top),
    };
  });
}

/** Tooltip and screen reader text: "In the split view: top left". */
export function placementText(t: Translate, placement: PanePlacement): string {
  return t("session.paneAt", { zone: placeName(t, placement.layout, placement.zone) });
}
