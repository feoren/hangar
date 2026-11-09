import { describe, expect, it } from "vitest";

import { rowPlace, waitingHints, type RowPlace } from "./waitingHint";

describe("rowPlace", () => {
  const view = { top: 100, bottom: 300 };

  it("counts a row as visible while half of it shows", () => {
    expect(rowPlace({ top: 150, bottom: 190 }, view)).toBe("visible");
    expect(rowPlace({ top: 80, bottom: 120 }, view)).toBe("visible");
    expect(rowPlace({ top: 60, bottom: 110 }, view)).toBe("above");
    expect(rowPlace({ top: 290, bottom: 340 }, view)).toBe("below");
    expect(rowPlace({ top: 400, bottom: 440 }, view)).toBe("below");
  });
});

describe("waitingHints", () => {
  const order = [1, 2, 3, 4, 5];
  const places = new Map<number, RowPlace>([
    [1, "above"],
    [2, "above"],
    [3, "visible"],
    [4, "below"],
    [5, "below"],
  ]);

  it("counts waiting sessions per edge and targets the nearest", () => {
    expect(waitingHints(order, new Set([1, 2, 5]), places)).toEqual({
      above: { count: 2, targetId: 2 },
      below: { count: 1, targetId: 5 },
    });
  });

  it("stays quiet for visible or unmeasured rows", () => {
    expect(waitingHints(order, new Set([3]), places)).toEqual({ above: null, below: null });
    expect(waitingHints([9], new Set([9]), places)).toEqual({ above: null, below: null });
  });
});
