import { describe, expect, it } from "vitest";

import {
  DEFAULT_DOT_ANIMATION,
  DOT_ANIMATIONS,
  dotAnimationClass,
  isDotAnimation,
  parseDotAnimation,
} from "./dotAnimation";

describe("dot animation", () => {
  it("bounces by default", () => {
    expect(DEFAULT_DOT_ANIMATION).toBe("bounce");
    expect(parseDotAnimation(undefined)).toBe("bounce");
  });

  it("keeps every known style", () => {
    for (const style of DOT_ANIMATIONS) {
      expect(isDotAnimation(style)).toBe(true);
      expect(parseDotAnimation(style)).toBe(style);
    }
  });

  it("falls back for unknown or wrong values", () => {
    for (const value of ["spin", "", "Bounce", 1, null, {}]) {
      expect(isDotAnimation(value)).toBe(false);
      expect(parseDotAnimation(value)).toBe(DEFAULT_DOT_ANIMATION);
    }
  });

  it("names the CSS class of a style", () => {
    expect(dotAnimationClass("wave")).toBe("dots-wave");
  });
});
