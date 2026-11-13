import { describe, expect, it } from "vitest";

import {
  splitters,
  KEY_STEP_SHARE,
  MIN_PANE_SHARE,
  minShare,
  parsePaneRatios,
  parseSubAgentRatios,
  ratioBounds,
  ratiosFor,
  sameRatios,
  setRatio,
  stepRatio,
  SUB_AGENT_LAYOUT,
  subAgentRatios,
  withoutRatios,
  withRatios,
} from "./paneRatios";
import { DEFAULT_RATIOS, LAYOUT_IDS, zoneAt, zoneCount, zones, type LayoutId } from "./snapLayouts";

const WIDE = { width: 2000, height: 1000 };

describe("ratiosFor", () => {
  it("uses the stored positions or the preset", () => {
    expect(ratiosFor({}, "pair")).toEqual([0.5]);
    expect(ratiosFor({ pair: [0.7] }, "pair")).toEqual([0.7]);
    expect(ratiosFor({ pair: [0.7, 0.2] }, "pair")).toEqual([0.5]);
    expect(ratiosFor({}, "mainStack")).toEqual([0.6, 0.5]);
  });

  it("stores and forgets per layout", () => {
    const stored = withRatios({ quad: [0.4, 0.6] }, "pair", [0.3]);
    expect(stored).toEqual({ quad: [0.4, 0.6], pair: [0.3] });
    expect(withoutRatios(stored, "pair")).toEqual({ quad: [0.4, 0.6] });
    expect(sameRatios([0.5], [0.5 + 1e-12])).toBe(true);
    expect(sameRatios([0.5], [0.6])).toBe(false);
  });
});

describe("splitters", () => {
  it("has one splitter per ratio, and none for a single zone", () => {
    for (const layout of LAYOUT_IDS) {
      expect(splitters(layout, DEFAULT_RATIOS[layout]), layout).toHaveLength(
        DEFAULT_RATIOS[layout].length,
      );
    }
    expect(splitters("single", [])).toEqual([]);
  });

  it("puts the stack's row splitter only beside the main zone", () => {
    expect(splitters("mainStack", [0.6, 0.4])).toEqual([
      { index: 0, orientation: "vertical", at: 0.6, from: 0, to: 1 },
      { index: 1, orientation: "horizontal", at: 0.4, from: 0.6, to: 1 },
    ]);
  });

  it("keeps the grid aligned with one line per axis", () => {
    expect(splitters("quad", [0.3, 0.7]).map((d) => [d.orientation, d.at, d.from, d.to])).toEqual([
      ["vertical", 0.3, 0, 1],
      ["horizontal", 0.7, 0, 1],
    ]);
  });
});

describe("zones follow the ratios", () => {
  it("cover the whole area without overlap at any position", () => {
    const samples: [LayoutId, number[]][] = [
      ["pair", [0.27]],
      ["triple", [0.2, 0.71]],
      ["mainStack", [0.42, 0.33]],
      ["quad", [0.61, 0.24]],
    ];
    for (const [layout, ratios] of samples) {
      const found = zones(layout, ratios);
      expect(found).toHaveLength(zoneCount(layout));
      const area = found.reduce((sum, zone) => sum + zone.width * zone.height, 0);
      expect(area, layout).toBeCloseTo(1, 10);
      found.forEach((zone, index) => {
        expect(zoneAt(layout, zone.x + zone.width / 2, zone.y + zone.height / 2, ratios)).toBe(
          index,
        );
      });
    }
  });

  it("drop targets move with the splitter", () => {
    // At 0.3 the default split would say left; with the splitter at 0.2 it is the right zone.
    expect(zoneAt("pair", 0.3, 0.5)).toBe(0);
    expect(zoneAt("pair", 0.3, 0.5, [0.2])).toBe(1);
  });

  it("falls back to the preset for unusable ratios", () => {
    expect(zones("pair", [Number.NaN])).toEqual(zones("pair"));
    expect(zones("triple", [0.5])).toEqual(zones("triple"));
  });
});

describe("clamping", () => {
  it("keeps 15 % or the pixel minimum, whichever is larger", () => {
    expect(minShare("vertical")).toBe(MIN_PANE_SHARE);
    expect(minShare("vertical", WIDE)).toBe(MIN_PANE_SHARE);
    // 240 px of 1000 px is more than 15 %.
    expect(minShare("vertical", { width: 1000, height: 600 })).toBeCloseTo(0.24, 10);
    // 120 px of 500 px.
    expect(minShare("horizontal", { width: 1000, height: 500 })).toBeCloseTo(0.24, 10);
  });

  it("stops a splitter before a pane gets too small", () => {
    expect(setRatio("pair", [0.5], 0, 0.02, WIDE)).toEqual([MIN_PANE_SHARE]);
    expect(setRatio("pair", [0.5], 0, 0.99, WIDE)).toEqual([1 - MIN_PANE_SHARE]);
    expect(setRatio("pair", [0.5], 0, 0.4, WIDE)).toEqual([0.4]);
    expect(setRatio("pair", [0.5], 0, 0.1, { width: 1000, height: 600 })[0]).toBeCloseTo(0.24, 10);
  });

  it("keeps the middle column of three wide enough", () => {
    const bounds = ratioBounds("triple", [0.3, 0.6], 0, WIDE);
    expect(bounds.min).toBeCloseTo(0.15, 10);
    expect(bounds.max).toBeCloseTo(0.45, 10);
    expect(setRatio("triple", [0.3, 0.6], 0, 0.58, WIDE)[0]).toBeCloseTo(0.45, 10);
    expect(setRatio("triple", [0.3, 0.6], 1, 0.31, WIDE)[1]).toBeCloseTo(0.45, 10);
  });

  it("stays in the middle when the area is too small for both minimums", () => {
    expect(ratioBounds("pair", [0.5], 0, { width: 300, height: 300 })).toEqual({
      min: 0.5,
      max: 0.5,
    });
    expect(setRatio("pair", [0.3], 0, 0.1, { width: 300, height: 300 })).toEqual([0.5]);
  });

  it("only moves the splitter it was asked to", () => {
    expect(setRatio("quad", [0.5, 0.5], 1, 0.3, WIDE)).toEqual([0.5, 0.3]);
    expect(setRatio("quad", [0.5, 0.5], 5, 0.3, WIDE)).toEqual([0.5, 0.5]);
    expect(setRatio("quad", [0.5, 0.5], 0, Number.NaN, WIDE)).toEqual([0.5, 0.5]);
  });
});

describe("stepRatio", () => {
  it("steps along the splitter's axis", () => {
    expect(stepRatio("pair", [0.5], 0, "ArrowRight", WIDE)?.[0]).toBeCloseTo(
      0.5 + KEY_STEP_SHARE,
      10,
    );
    expect(stepRatio("pair", [0.5], 0, "ArrowLeft", WIDE)?.[0]).toBeCloseTo(
      0.5 - KEY_STEP_SHARE,
      10,
    );
    expect(stepRatio("quad", [0.5, 0.5], 1, "ArrowDown", WIDE)?.[1]).toBeCloseTo(
      0.5 + KEY_STEP_SHARE,
      10,
    );
    expect(stepRatio("mainStack", [0.6, 0.5], 1, "ArrowUp", WIDE)?.[1]).toBeCloseTo(
      0.5 - KEY_STEP_SHARE,
      10,
    );
  });

  it("ignores the other axis and unknown keys", () => {
    expect(stepRatio("pair", [0.5], 0, "ArrowUp", WIDE)).toBeNull();
    expect(stepRatio("quad", [0.5, 0.5], 1, "ArrowLeft", WIDE)).toBeNull();
    expect(stepRatio("pair", [0.5], 0, "a", WIDE)).toBeNull();
    expect(stepRatio("pair", [0.5], 3, "ArrowLeft", WIDE)).toBeNull();
  });

  it("jumps to the limits with Home and End", () => {
    expect(stepRatio("pair", [0.5], 0, "Home", WIDE)).toEqual([MIN_PANE_SHARE]);
    expect(stepRatio("pair", [0.5], 0, "End", WIDE)).toEqual([1 - MIN_PANE_SHARE]);
  });

  it("never steps past a limit", () => {
    expect(stepRatio("pair", [MIN_PANE_SHARE], 0, "ArrowLeft", WIDE)).toEqual([MIN_PANE_SHARE]);
  });
});

describe("parsePaneRatios", () => {
  it("keeps valid positions per layout", () => {
    expect(parsePaneRatios({ pair: [0.7], triple: [0.25, 0.6], quad: [0.4, 0.3] })).toEqual({
      pair: [0.7],
      triple: [0.25, 0.6],
      quad: [0.4, 0.3],
    });
  });

  it("drops wrong lengths, types, out of range and unknown layouts", () => {
    expect(
      parsePaneRatios({
        single: [0.5],
        pair: [0.05],
        triple: [0.6, 0.5],
        mainStack: [0.5],
        quad: [0.5, "x"],
        columns: [0.5],
      }),
    ).toEqual({});
    expect(parsePaneRatios(null)).toEqual({});
    expect(parsePaneRatios("wide")).toEqual({});
    expect(parsePaneRatios([0.5])).toEqual({});
  });
});

describe("sub-agent view ratios", () => {
  it("uses the two-column preset until a position is stored", () => {
    expect(subAgentRatios(null)).toEqual(DEFAULT_RATIOS[SUB_AGENT_LAYOUT]);
    expect(subAgentRatios([0.65])).toEqual([0.65]);
  });

  it("keeps the terminal and the view at least 240 px wide", () => {
    const area = { width: 1200, height: 800 };
    expect(setRatio(SUB_AGENT_LAYOUT, [0.5], 0, 0.95, area)).toEqual([1 - 240 / 1200]);
    expect(setRatio(SUB_AGENT_LAYOUT, [0.5], 0, 0.05, area)).toEqual([240 / 1200]);
    expect(setRatio(SUB_AGENT_LAYOUT, [0.5], 0, 0.6, area)).toEqual([0.6]);
  });

  it("parses a stored position and drops unusable ones", () => {
    expect(parseSubAgentRatios([0.7])).toEqual([0.7]);
    expect(parseSubAgentRatios([0.99])).toBeNull();
    expect(parseSubAgentRatios([0.4, 0.6])).toBeNull();
    expect(parseSubAgentRatios(["wide"])).toBeNull();
    expect(parseSubAgentRatios(undefined)).toBeNull();
  });
});
