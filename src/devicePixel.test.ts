import { describe, expect, it } from "vitest";

import { devicePx, devicePxValue } from "./devicePixel";

describe("devicePx", () => {
  it("is one CSS pixel divided by the scale", () => {
    expect(devicePx(1)).toBe(1);
    expect(devicePx(1.25)).toBe(0.8);
    expect(devicePx(2)).toBe(0.5);
  });

  it("falls back to one CSS pixel for a broken ratio", () => {
    for (const ratio of [0, -1, Number.NaN, Number.POSITIVE_INFINITY]) {
      expect(devicePx(ratio)).toBe(1);
    }
  });
});

describe("devicePxValue", () => {
  it("writes a short CSS length that still lands on the device pixel", () => {
    expect(devicePxValue(1)).toBe("1px");
    expect(devicePxValue(1.25)).toBe("0.8px");
    expect(devicePxValue(1.5)).toBe("0.666667px");
    for (const ratio of [1.25, 1.5, 1.75, 2, 2.25, 2.5, 3]) {
      const px = Number.parseFloat(devicePxValue(ratio));
      // Whole device pixels in one and three CSS pixels, as CSS round(down, ...) computes them.
      expect(Math.floor(1 / px)).toBe(Math.floor(ratio));
      expect(Math.floor(3 / px)).toBe(Math.floor(3 * ratio));
    }
  });
});
