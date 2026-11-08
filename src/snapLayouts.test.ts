import { describe, expect, it } from "vitest";

import {
  isLayoutId,
  LAYOUT_IDS,
  layoutForCount,
  migrateLayoutId,
  sameTarget,
  SNAP_TARGETS,
  stepTarget,
  targetIndex,
  zoneAt,
  zoneCount,
  zonePlace,
  zoneRect,
  zones,
} from "./snapLayouts";

describe("layouts", () => {
  it("offer the five presets in picker order", () => {
    expect(LAYOUT_IDS).toEqual(["single", "pair", "triple", "mainStack", "quad"]);
    expect(LAYOUT_IDS.map(zoneCount)).toEqual([1, 2, 3, 3, 4]);
  });

  it("cover the whole area without overlap", () => {
    for (const layout of LAYOUT_IDS) {
      const area = zones(layout).reduce((sum, zone) => sum + zone.width * zone.height, 0);
      expect(area, layout).toBeCloseTo(1, 10);
      for (const zone of zones(layout)) {
        expect(zone.x + zone.width, layout).toBeLessThanOrEqual(1 + 1e-9);
        expect(zone.y + zone.height, layout).toBeLessThanOrEqual(1 + 1e-9);
      }
      // Every zone's center lies in that zone and is found as that zone.
      zones(layout).forEach((zone, index) => {
        expect(zoneAt(layout, zone.x + zone.width / 2, zone.y + zone.height / 2)).toBe(index);
      });
    }
  });

  it("puts the main zone of one large plus two stacked on the left", () => {
    expect(zoneRect("mainStack", 0)).toEqual({ x: 0, y: 0, width: 0.6, height: 1 });
    expect(zoneRect("mainStack", 2)).toEqual({ x: 0.6, y: 0.5, width: 0.4, height: 0.5 });
    expect(zoneRect("quad", 9)).toEqual({ x: 0, y: 0, width: 1, height: 1 });
  });

  it("names zones by their place", () => {
    expect(zonePlace("single", 0)).toBe("full");
    expect(zonePlace("triple", 1)).toBe("center");
    expect(zonePlace("mainStack", 1)).toBe("topRight");
    expect(zonePlace("quad", 2)).toBe("bottomLeft");
  });

  it("finds the zone under a point and nothing outside the area", () => {
    expect(zoneAt("pair", 0.2, 0.9)).toBe(0);
    expect(zoneAt("pair", 0.8, 0.1)).toBe(1);
    expect(zoneAt("quad", 0.9, 0.9)).toBe(3);
    expect(zoneAt("quad", 1.2, 0.5)).toBeNull();
    expect(zoneAt("quad", Number.NaN, 0.5)).toBeNull();
  });
});

describe("migrateLayoutId", () => {
  it("keeps layout IDs and maps the split modes of 0.2", () => {
    expect(migrateLayoutId("mainStack")).toBe("mainStack");
    expect(migrateLayoutId("single")).toBe("single");
    expect(migrateLayoutId("columns")).toBe("pair");
    expect(migrateLayoutId("grid")).toBe("quad");
  });

  it("rejects anything else", () => {
    expect(migrateLayoutId("rows")).toBeNull();
    expect(migrateLayoutId(2)).toBeNull();
    expect(migrateLayoutId(undefined)).toBeNull();
    expect(isLayoutId("grid")).toBe(false);
  });
});

describe("layoutForCount", () => {
  it("takes the smallest layout that fits, at most four", () => {
    expect([0, 1, 2, 3, 4, 7].map(layoutForCount)).toEqual([
      "single",
      "single",
      "pair",
      "triple",
      "quad",
      "quad",
    ]);
  });
});

describe("picker targets", () => {
  it("list every zone of every layout once", () => {
    expect(SNAP_TARGETS).toHaveLength(13);
    expect(SNAP_TARGETS[0]).toEqual({ layout: "single", zone: 0 });
    expect(SNAP_TARGETS[12]).toEqual({ layout: "quad", zone: 3 });
    expect(targetIndex({ layout: "pair", zone: 1 })).toBe(2);
    expect(targetIndex({ layout: "pair", zone: 5 })).toBe(-1);
  });

  it("compare by layout and zone", () => {
    expect(sameTarget({ layout: "pair", zone: 1 }, { layout: "pair", zone: 1 })).toBe(true);
    expect(sameTarget({ layout: "pair", zone: 1 }, { layout: "quad", zone: 1 })).toBe(false);
    expect(sameTarget(null, null)).toBe(false);
  });

  it("move with the arrow keys without wrapping", () => {
    expect(stepTarget(0, "ArrowRight")).toBe(1);
    expect(stepTarget(1, "ArrowDown")).toBe(2);
    expect(stepTarget(2, "ArrowLeft")).toBe(1);
    expect(stepTarget(0, "ArrowUp")).toBe(0);
    expect(stepTarget(12, "ArrowRight")).toBe(12);
    expect(stepTarget(5, "Home")).toBe(0);
    expect(stepTarget(5, "End")).toBe(12);
    expect(stepTarget(5, "Enter")).toBeNull();
  });
});
