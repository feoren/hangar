import { describe, expect, it } from "vitest";

import { rangeFillStyle } from "./rangeFill";

describe("rangeFillStyle", () => {
  it("puts the value between the bounds as a ratio", () => {
    expect(rangeFillStyle(60, 0, 100)).toEqual({ "--range-ratio": 0.6 });
    expect(rangeFillStyle(3, 2, 6)).toEqual({ "--range-ratio": 0.25 });
  });

  it("keeps the ratio between 0 and 1", () => {
    expect(rangeFillStyle(-5, 0, 10)).toEqual({ "--range-ratio": 0 });
    expect(rangeFillStyle(15, 0, 10)).toEqual({ "--range-ratio": 1 });
  });

  it("stays empty for an empty range or a value that is not a number", () => {
    expect(rangeFillStyle(4, 4, 4)).toEqual({ "--range-ratio": 0 });
    expect(rangeFillStyle(Number.NaN, 0, 10)).toEqual({ "--range-ratio": 0 });
  });
});
