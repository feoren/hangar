import { describe, expect, it } from "vitest";

import { attentionTint } from "./attentionLook";
import {
  chosenSwatch,
  clampPulseSeconds,
  CREAM,
  DEFAULT_LOOKS,
  defaultColor,
  formatPulseSeconds,
  isDefaultLook,
  lookClass,
  LOOK_PRESETS,
  LOOK_STATES,
  normalizeColor,
  parseStateLooks,
  presetsFor,
  pulseLevels,
  PULSE_SECONDS,
  resolvedColor,
  withLook,
} from "./stateLook";

describe("defaults", () => {
  it("keep the look of earlier versions", () => {
    expect(DEFAULT_LOOKS.done).toEqual({ color: null, pulse: "medium", seconds: 2.8 });
    expect(DEFAULT_LOOKS.asks).toEqual(DEFAULT_LOOKS.done);
    expect(DEFAULT_LOOKS.working.pulse).toBe("off");
    expect(DEFAULT_LOOKS.idle.pulse).toBe("off");
    for (const state of LOOK_STATES) expect(isDefaultLook(state, DEFAULT_LOOKS[state])).toBe(true);
  });

  it("color waiting states cream and working and pause in the theme's green", () => {
    expect(defaultColor("done", "dark")).toBe(CREAM);
    expect(defaultColor("asks", "light")).toBe(CREAM);
    expect(defaultColor("working", "dark")).toBe("#57ab5a");
    expect(defaultColor("idle", "light")).toBe("#1a7f37");
    const looks = { ...DEFAULT_LOOKS, done: { ...DEFAULT_LOOKS.done, color: "#123456" } };
    expect(resolvedColor(looks, "done", "dark")).toBe("#123456");
  });
});

describe("presetsFor", () => {
  it("leaves out the preset the default already shows", () => {
    expect(presetsFor("done").some((preset) => preset.color === CREAM)).toBe(false);
    expect(presetsFor("working")).toEqual(LOOK_PRESETS);
  });
});

describe("clampPulseSeconds", () => {
  it("snaps to the step and stays in range", () => {
    expect(clampPulseSeconds(2.8)).toBe(2.8);
    expect(clampPulseSeconds(1.27)).toBe(1.2);
    expect(clampPulseSeconds(0)).toBe(PULSE_SECONDS.min);
    expect(clampPulseSeconds(99)).toBe(PULSE_SECONDS.max);
  });
});

describe("parseStateLooks", () => {
  it("falls back per field", () => {
    const looks = parseStateLooks(
      { working: { color: "#ABC", pulse: "loud", seconds: 9 }, done: "x" },
      undefined,
    );
    expect(looks.working).toEqual({ color: "#aabbcc", pulse: "off", seconds: 4 });
    expect(looks.done).toEqual(DEFAULT_LOOKS.done);
    expect(looks.idle).toEqual(DEFAULT_LOOKS.idle);
  });

  it("keeps an explicit default color", () => {
    const looks = parseStateLooks(
      { asks: { color: null, pulse: "strong", seconds: 1 } },
      "#123456",
    );
    expect(looks.asks).toEqual({ color: null, pulse: "strong", seconds: 1 });
    // Stored looks without "done" still take the older color for it.
    expect(looks.done.color).toBe("#123456");
  });
});

describe("normalizeColor", () => {
  it("normalizes to #rrggbb", () => {
    expect(normalizeColor("#ABC")).toBe("#aabbcc");
    expect(normalizeColor("#F2E3C6")).toBe("#f2e3c6");
    expect(normalizeColor("f2e3c6")).toBeNull();
    expect(normalizeColor(null)).toBeNull();
  });
});

describe("pulseLevels", () => {
  it("medium is the breath of earlier versions", () => {
    const peak = attentionTint(CREAM, "light");
    expect(pulseLevels(peak.high, "medium", true)).toEqual({
      low: peak.low,
      high: peak.high,
      still: peak.high,
      animated: true,
    });
  });

  it("grows from subtle to strong", () => {
    const subtle = pulseLevels(60, "subtle", true);
    const medium = pulseLevels(60, "medium", true);
    const strong = pulseLevels(60, "strong", true);
    expect(subtle.high - subtle.low).toBeLessThan(medium.high - medium.low);
    expect(medium.high - medium.low).toBeLessThan(strong.high - strong.low);
    for (const levels of [subtle, medium, strong]) expect(levels.high).toBeLessThanOrEqual(60);
  });

  it("without a pulse, keeps a still tint only for waiting rows", () => {
    expect(pulseLevels(60, "off", true)).toEqual({ low: 39, high: 39, still: 39, animated: false });
    expect(pulseLevels(60, "off", false)).toEqual({ low: 0, high: 0, still: 0, animated: false });
  });

  it("gives a pulsing working row a still tint for reduced motion", () => {
    expect(pulseLevels(60, "medium", false).still).toBe(39);
  });
});

describe("withLook", () => {
  it("changes one state and keeps the others", () => {
    const looks = withLook(DEFAULT_LOOKS, "asks", { pulse: "strong" });
    expect(looks.asks).toEqual({ ...DEFAULT_LOOKS.asks, pulse: "strong" });
    expect(looks.done).toBe(DEFAULT_LOOKS.done);
    expect(looks.working).toBe(DEFAULT_LOOKS.working);
  });
});

describe("chosenSwatch", () => {
  it("tells the default, a preset and a custom color apart", () => {
    expect(chosenSwatch("done", DEFAULT_LOOKS.done)).toBe("default");
    expect(chosenSwatch("done", { ...DEFAULT_LOOKS.done, color: "#9ee8dc" })).toBe("mint");
    expect(chosenSwatch("working", { ...DEFAULT_LOOKS.working, color: CREAM })).toBe("cream");
    expect(chosenSwatch("done", { ...DEFAULT_LOOKS.done, color: "#123456" })).toBe("custom");
  });
});

describe("lookClass", () => {
  it("names the state's classes, nothing for a plain row", () => {
    expect(lookClass("asks")).toBe("look look-asks");
    expect(lookClass(null)).toBe("");
  });
});

describe("formatPulseSeconds", () => {
  it("uses the language's decimal mark", () => {
    expect(formatPulseSeconds(2.8, "de")).toBe("2,8");
    expect(formatPulseSeconds(1, "en")).toBe("1.0");
  });
});
