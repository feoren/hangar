import { describe, expect, it } from "vitest";

import { isOneLine, ONE_LINE_LAYOUT, setOneLine, toggleOneLine } from "./oneLineRows";
import { DEFAULT_ROW_LAYOUT, planRow, presetLayout } from "./rowLayout";
import { defaultSettings, parseSettings } from "./settings";

describe("one-line rows", () => {
  it("is one compact line", () => {
    expect(planRow(ONE_LINE_LAYOUT).lines).toHaveLength(1);
    expect(ONE_LINE_LAYOUT.density).toBe("compact");
    expect(isOneLine(ONE_LINE_LAYOUT)).toBe(true);
    expect(isOneLine(DEFAULT_ROW_LAYOUT)).toBe(false);
    // The preset of the same name counts as on, also at normal density.
    expect(isOneLine(presetLayout("accordion"))).toBe(true);
  });

  it("turns on and brings the previous layout back when turned off", () => {
    const own = { ...presetLayout("ring"), frame: "card" as const };
    const on = toggleOneLine({ rowLayout: own, rowLayoutBefore: null });
    expect(on).toEqual({ rowLayout: ONE_LINE_LAYOUT, rowLayoutBefore: own });
    const off = toggleOneLine(on);
    expect(off).toEqual({ rowLayout: own, rowLayoutBefore: null });
  });

  it("changes nothing when it already is as asked", () => {
    const state = { rowLayout: DEFAULT_ROW_LAYOUT, rowLayoutBefore: null };
    expect(setOneLine(state, false)).toBe(state);
    const on = setOneLine(state, true);
    expect(setOneLine(on, true)).toBe(on);
  });

  it("falls back to the standard row without a layout from before", () => {
    const built = presetLayout("accordion");
    expect(toggleOneLine({ rowLayout: built, rowLayoutBefore: null })).toEqual({
      rowLayout: DEFAULT_ROW_LAYOUT,
      rowLayoutBefore: null,
    });
    expect(toggleOneLine({ rowLayout: built, rowLayoutBefore: ONE_LINE_LAYOUT }).rowLayout).toEqual(
      DEFAULT_ROW_LAYOUT,
    );
  });

  it("keeps the layout from before across a restart", () => {
    const on = setOneLine({ rowLayout: presetLayout("tiles"), rowLayoutBefore: null }, true);
    const stored = JSON.stringify({ ...defaultSettings("de"), ...on });
    const parsed = parseSettings(stored, "de");
    expect(parsed.rowLayout).toEqual(ONE_LINE_LAYOUT);
    expect(parsed.rowLayoutBefore).toEqual(presetLayout("tiles"));
    expect(setOneLine(parsed, false).rowLayout).toEqual(presetLayout("tiles"));
    expect(parseSettings(JSON.stringify({ rowLayoutBefore: "x" }), "de").rowLayoutBefore).toBe(
      null,
    );
  });
});
