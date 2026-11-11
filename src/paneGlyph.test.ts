import { describe, expect, it } from "vitest";

import { translate } from "./i18n";
import { GLYPH_SIZE, glyphRects, placementText } from "./paneGlyph";
import { DEFAULT_RATIOS } from "./snapLayouts";

describe("glyphRects", () => {
  it("draws two halves with a one pixel gap", () => {
    expect(glyphRects("pair", [0.5])).toEqual([
      { x: 0, y: 0, width: 6, height: 10 },
      { x: 7, y: 0, width: 7, height: 10 },
    ]);
  });

  it("snaps every zone to whole pixels inside the glyph", () => {
    for (const [layout, ratios] of [
      ["triple", DEFAULT_RATIOS.triple],
      ["mainStack", [0.43, 0.37]],
      ["quad", [0.3, 0.8]],
    ] as const) {
      for (const rect of glyphRects(layout, ratios)) {
        for (const value of [rect.x, rect.y, rect.width, rect.height]) {
          expect(Number.isInteger(value), layout).toBe(true);
        }
        expect(rect.width).toBeGreaterThanOrEqual(1);
        expect(rect.height).toBeGreaterThanOrEqual(1);
        expect(rect.x + rect.width).toBeLessThanOrEqual(GLYPH_SIZE.width);
        expect(rect.y + rect.height).toBeLessThanOrEqual(GLYPH_SIZE.height);
      }
    }
  });

  it("follows the splitters roughly", () => {
    const [main, top, bottom] = glyphRects("mainStack", [0.6, 0.3]);
    expect(main).toEqual({ x: 0, y: 0, width: 7, height: 10 });
    expect(top).toEqual({ x: 8, y: 0, width: 6, height: 2 });
    expect(bottom).toEqual({ x: 8, y: 3, width: 6, height: 7 });
  });

  it("keeps a zone visible even when it is very narrow", () => {
    const rects = glyphRects("triple", [0.15, 0.2], 14, 10);
    expect(rects[1]?.width).toBeGreaterThanOrEqual(1);
  });
});

describe("placementText", () => {
  it("names the zone in the split view", () => {
    const t = (key: Parameters<typeof translate>[1], params?: Parameters<typeof translate>[2]) =>
      translate("en", key, params);
    expect(placementText(t, { layout: "quad", zone: 0, ratios: [0.5, 0.5] })).toBe(
      "In the split view: top left",
    );
    expect(placementText(t, { layout: "pair", zone: 1, ratios: [0.5] })).toBe(
      "In the split view: right",
    );
  });
});
