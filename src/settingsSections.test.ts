import { describe, expect, it } from "vitest";

import { nextTabIndex, SETTINGS_SECTIONS } from "./settingsSections";

describe("nextTabIndex", () => {
  it("moves with the arrow keys and wraps around at the end", () => {
    expect(nextTabIndex("ArrowDown", 0, 5)).toBe(1);
    expect(nextTabIndex("ArrowRight", 4, 5)).toBe(0);
    expect(nextTabIndex("ArrowUp", 0, 5)).toBe(4);
    expect(nextTabIndex("ArrowLeft", 3, 5)).toBe(2);
  });

  it("jumps to the edge with Home and End", () => {
    expect(nextTabIndex("Home", 3, 5)).toBe(0);
    expect(nextTabIndex("End", 1, 5)).toBe(4);
  });

  it("ignores other keys and empty lists", () => {
    expect(nextTabIndex("Enter", 1, 5)).toBeNull();
    expect(nextTabIndex("a", 1, 5)).toBeNull();
    expect(nextTabIndex("ArrowDown", 0, 0)).toBeNull();
  });
});

describe("SETTINGS_SECTIONS", () => {
  it("starts with General and contains every section once", () => {
    expect(SETTINGS_SECTIONS[0]).toBe("general");
    expect(new Set(SETTINGS_SECTIONS).size).toBe(SETTINGS_SECTIONS.length);
  });
});
