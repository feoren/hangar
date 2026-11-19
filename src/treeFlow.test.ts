import { describe, expect, it } from "vitest";

import {
  DEFAULT_TREE_FLOW,
  flowEnd,
  highestLevel,
  LEVEL_UP,
  nextLevel,
  parseTreeFlow,
  RateMeter,
  rateParts,
} from "./treeFlow";

describe("parseTreeFlow", () => {
  it("is off by default, with the stream and the rate", () => {
    expect(DEFAULT_TREE_FLOW).toEqual({ enabled: false, style: "stream", showRate: true });
    expect(parseTreeFlow(undefined)).toEqual(DEFAULT_TREE_FLOW);
  });

  it("keeps valid stored values", () => {
    const stored = { enabled: true, style: "dots", showRate: false };
    expect(parseTreeFlow(stored)).toEqual(stored);
  });

  it("replaces each invalid part with its default", () => {
    expect(parseTreeFlow({ enabled: "yes", style: "sparkle", showRate: 0 })).toEqual(
      DEFAULT_TREE_FLOW,
    );
    expect(parseTreeFlow({ enabled: true, style: 3 })).toEqual({
      ...DEFAULT_TREE_FLOW,
      enabled: true,
    });
    for (const value of [null, "on", 1, ["stream"]]) {
      expect(parseTreeFlow(value)).toEqual(DEFAULT_TREE_FLOW);
    }
  });
});

describe("nextLevel", () => {
  it("goes up as soon as the rate reaches a level", () => {
    expect(nextLevel(1, 0)).toBe(1);
    expect(nextLevel(1, LEVEL_UP[2])).toBe(2);
    expect(nextLevel(1, LEVEL_UP[3])).toBe(3);
  });

  it("goes down only when the rate falls clearly below the level", () => {
    // Just below the threshold keeps the pace: no restart for every quieter second.
    expect(nextLevel(3, LEVEL_UP[3] * 0.8)).toBe(3);
    expect(nextLevel(3, LEVEL_UP[3] * 0.4)).toBe(2);
    expect(nextLevel(2, LEVEL_UP[2] * 0.6)).toBe(2);
    expect(nextLevel(2, LEVEL_UP[2] * 0.4)).toBe(1);
    expect(nextLevel(3, 0)).toBe(1);
  });

  it("takes the busiest of several sessions for a trunk", () => {
    expect(highestLevel([])).toBe(1);
    expect(highestLevel([1, 3, 2])).toBe(3);
  });
});

describe("RateMeter", () => {
  it("turns counted bytes into smoothed rates per second", () => {
    const meter = new RateMeter();
    meter.output(1, 10_000);
    meter.input(1, 200);
    expect(meter.sample(1000)).toBe(true);
    // Half of the first second, as the smoothing starts from zero.
    expect(meter.rate(1)).toEqual({ down: 5_000, up: 100, level: 2 });
    meter.output(1, 10_000);
    meter.sample(2000);
    expect(meter.rate(1)?.down).toBe(7_500);
  });

  it("measures by the time that passed", () => {
    const meter = new RateMeter();
    meter.sample(0);
    meter.output(7, 4_000);
    meter.sample(2000);
    // 4000 bytes in two seconds are 2000 per second, half of it after one smoothing step.
    expect(meter.rate(7)?.down).toBe(1_000);
  });

  it("keeps the same rate object while the shown numbers stay", () => {
    const meter = new RateMeter();
    meter.output(1, 10_000);
    meter.sample(1000);
    const first = meter.rate(1);
    for (let second = 2; second < 30; second += 1) {
      meter.output(1, 32_000);
      meter.sample(second * 1000);
    }
    const settled = meter.rate(1);
    meter.output(1, 32_000);
    expect(meter.sample(30_000)).toBe(false);
    expect(meter.rate(1)).toBe(settled);
    expect(settled).not.toBe(first);
  });

  it("calms down and forgets a quiet session", () => {
    const meter = new RateMeter();
    meter.output(1, 100_000);
    meter.sample(1000);
    expect(meter.rate(1)?.level).toBe(3);
    let now = 1000;
    while (meter.rate(1) !== undefined && now < 60_000) {
      now += 1000;
      meter.sample(now);
    }
    expect(meter.rate(1)).toBeUndefined();
  });

  it("ignores a sample without time passing and starts over after clear", () => {
    const meter = new RateMeter();
    meter.output(1, 5_000);
    meter.sample(1000);
    meter.output(1, 5_000);
    expect(meter.sample(1000)).toBe(false);
    meter.clear();
    expect(meter.rate(1)).toBeUndefined();
  });
});

describe("flowEnd", () => {
  it("ends the trunk at the last live row", () => {
    expect(flowEnd(null, [1, 2, 3, 4], [3, 1])).toBe(3);
  });

  it("keeps the former end while nothing is live, so the trunk fades at its length", () => {
    expect(flowEnd(3, [1, 2, 3, 4], [])).toBe(3);
    // A row that is gone ends nothing.
    expect(flowEnd(5, [1, 2, 3, 4], [])).toBeNull();
    expect(flowEnd(null, [1, 2], [])).toBeNull();
  });
});

describe("rateParts", () => {
  it("shows kilobytes with one decimal below ten, whole numbers above", () => {
    expect(rateParts(100, "de")).toEqual({ unit: "kb", value: "0,1" });
    expect(rateParts(12_000, "de")).toEqual({ unit: "kb", value: "12" });
    expect(rateParts(9_960, "en")).toEqual({ unit: "kb", value: "10" });
    expect(rateParts(640_000, "en")).toEqual({ unit: "kb", value: "640" });
  });

  it("switches to megabytes from a thousand kilobytes", () => {
    expect(rateParts(1_200_000, "de")).toEqual({ unit: "mb", value: "1,2" });
    expect(rateParts(999_600, "en")).toEqual({ unit: "mb", value: "1.0" });
  });
});
