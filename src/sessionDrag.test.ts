import { describe, expect, it } from "vitest";

import {
  AUTOSCROLL_MAX,
  autoscrollStep,
  beyondThreshold,
  dropIndex,
  dropLine,
  dropPlacement,
  sameDragTarget,
} from "./sessionDrag";
import { moveItem } from "./sessions";

describe("beyondThreshold", () => {
  it("keeps small wiggles a click", () => {
    expect(beyondThreshold(3, 3)).toBe(false);
    expect(beyondThreshold(0, 4)).toBe(false);
    expect(beyondThreshold(0, 5)).toBe(true);
    expect(beyondThreshold(-4, 4)).toBe(true);
  });
});

describe("dropIndex", () => {
  // Rows 1 to 4, 40 px high, starting at y = 0.
  const rows = [1, 2, 3, 4].map((id, index) => ({ id, mid: index * 40 + 20 }));

  it("counts the other rows above the pointer", () => {
    expect(dropIndex(rows, 1, 5)).toBe(0);
    expect(dropIndex(rows, 1, 70)).toBe(1);
    expect(dropIndex(rows, 1, 150)).toBe(3);
    expect(dropIndex(rows, 4, 0)).toBe(0);
    expect(dropIndex(rows, 4, 65)).toBe(2);
  });

  it("gives the index moveItem expects", () => {
    const items = rows.map((row) => ({ id: row.id }));
    // Dragging 1 just below the middle of 3 puts it between 3 and 4.
    expect(moveItem(items, 1, dropIndex(rows, 1, 105))?.map((item) => item.id)).toEqual([
      2, 3, 1, 4,
    ]);
  });
});

describe("dropLine", () => {
  const order = [1, 2, 3, 4];

  it("draws the line before the row that will follow", () => {
    expect(dropLine(order, 1, 2)).toEqual({ id: 4, edge: "before" });
    expect(dropLine(order, 4, 0)).toEqual({ id: 1, edge: "before" });
  });

  it("draws it after the last row for the end of the list", () => {
    expect(dropLine(order, 1, 3)).toEqual({ id: 4, edge: "after" });
  });

  it("draws nothing when the drop changes nothing", () => {
    expect(dropLine(order, 2, 1)).toBeNull();
    expect(dropLine(order, 9, 0)).toBeNull();
    expect(dropLine([1], 1, 0)).toBeNull();
  });
});

describe("autoscrollStep", () => {
  it("scrolls faster towards the edges and not in the middle", () => {
    expect(autoscrollStep(200, 0, 400)).toBe(0);
    expect(autoscrollStep(30, 0, 400)).toBeLessThan(0);
    expect(autoscrollStep(2, 0, 400)).toBeLessThan(autoscrollStep(20, 0, 400));
    expect(autoscrollStep(-50, 0, 400)).toBe(-AUTOSCROLL_MAX);
    expect(autoscrollStep(390, 0, 400)).toBeGreaterThan(0);
    expect(autoscrollStep(900, 0, 400)).toBe(AUTOSCROLL_MAX);
  });

  it("does nothing for a list without height", () => {
    expect(autoscrollStep(0, 100, 100)).toBe(0);
  });
});

describe("drop targets", () => {
  it("compare by kind and place", () => {
    expect(sameDragTarget({ kind: "list", index: 1 }, { kind: "list", index: 1 })).toBe(true);
    expect(sameDragTarget({ kind: "pane", zone: 1 }, { kind: "list", index: 1 })).toBe(false);
    expect(
      sameDragTarget(
        { kind: "snap", layout: "pair", zone: 0 },
        { kind: "snap", layout: "quad", zone: 0 },
      ),
    ).toBe(false);
    expect(sameDragTarget(null, null)).toBe(true);
    expect(sameDragTarget(null, { kind: "pane", zone: 0 })).toBe(false);
  });

  it("place a session in a bar zone or a pane of the current layout", () => {
    expect(dropPlacement({ kind: "snap", layout: "mainStack", zone: 2 }, "pair")).toEqual({
      layout: "mainStack",
      zone: 2,
    });
    expect(dropPlacement({ kind: "pane", zone: 1 }, "pair")).toEqual({ layout: "pair", zone: 1 });
    expect(dropPlacement({ kind: "list", index: 0 }, "pair")).toBeNull();
  });
});
