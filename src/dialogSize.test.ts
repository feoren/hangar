import { afterEach, describe, expect, it, vi } from "vitest";

import {
  clampDialogSize,
  DIALOG_KEY_STEP,
  dragDialogSize,
  edgeAxes,
  loadDialogSize,
  maxDialogSize,
  parseDialogSize,
  parseStoredDialogSize,
  RESIZE_EDGES,
  saveDialogSize,
  snapDialogSize,
  stepDialogSize,
} from "./dialogSize";

const min = { width: 640, height: 420 };
const viewport = { width: 1600, height: 1000 };
const start = { width: 936, height: 672 };

describe("dialog size limits", () => {
  it("leaves the margin free on every side of the window", () => {
    expect(maxDialogSize(viewport)).toEqual({ width: 1568, height: 968 });
    expect(maxDialogSize({ width: 20, height: 10 })).toEqual({ width: 0, height: 0 });
  });

  it("keeps the size between the minimum and the window", () => {
    expect(clampDialogSize(start, viewport, min)).toEqual(start);
    expect(clampDialogSize({ width: 100, height: 100 }, viewport, min)).toEqual(min);
    expect(clampDialogSize({ width: 5000, height: 5000 }, viewport, min)).toEqual({
      width: 1568,
      height: 968,
    });
    expect(clampDialogSize({ width: 700.4, height: 500.6 }, viewport, min)).toEqual({
      width: 700,
      height: 501,
    });
  });

  it("lets a window smaller than the minimum win, so the dialog still fits", () => {
    expect(clampDialogSize(start, { width: 600, height: 400 }, min)).toEqual({
      width: 568,
      height: 368,
    });
  });
});

describe("dragDialogSize", () => {
  it("maps every edge to the axes it changes", () => {
    expect(RESIZE_EDGES.map(edgeAxes)).toEqual([
      { x: 0, y: -1 },
      { x: 0, y: 1 },
      { x: 1, y: 0 },
      { x: -1, y: 0 },
      { x: 1, y: -1 },
      { x: -1, y: -1 },
      { x: 1, y: 1 },
      { x: -1, y: 1 },
    ]);
  });

  it("grows the centered dialog to both sides, so the edge stays under the pointer", () => {
    expect(dragDialogSize(start, "e", { x: 10, y: 50 }, viewport, min)).toEqual({
      width: 956,
      height: 672,
    });
    expect(dragDialogSize(start, "w", { x: 10, y: 0 }, viewport, min)).toEqual({
      width: 916,
      height: 672,
    });
    expect(dragDialogSize(start, "s", { x: 30, y: -20 }, viewport, min)).toEqual({
      width: 936,
      height: 632,
    });
    expect(dragDialogSize(start, "nw", { x: -10, y: -10 }, viewport, min)).toEqual({
      width: 956,
      height: 692,
    });
    expect(dragDialogSize(start, "se", { x: 4, y: 6 }, viewport, min)).toEqual({
      width: 944,
      height: 684,
    });
  });

  it("stops at the limits and follows the pointer again from the start size", () => {
    expect(dragDialogSize(start, "e", { x: -1000, y: 0 }, viewport, min).width).toBe(640);
    expect(dragDialogSize(start, "e", { x: 1000, y: 0 }, viewport, min).width).toBe(1568);
    // Back from beyond the limit: computed from the start, no offset left over.
    expect(dragDialogSize(start, "e", { x: 2, y: 0 }, viewport, min).width).toBe(940);
  });
});

describe("stepDialogSize", () => {
  it("changes the width with Left and Right and the height with Up and Down", () => {
    expect(stepDialogSize(start, "ArrowRight", viewport, min)).toEqual({
      width: 936 + DIALOG_KEY_STEP,
      height: 672,
    });
    expect(stepDialogSize(start, "ArrowLeft", viewport, min)?.width).toBe(936 - DIALOG_KEY_STEP);
    expect(stepDialogSize(start, "ArrowDown", viewport, min)?.height).toBe(672 + DIALOG_KEY_STEP);
    expect(stepDialogSize(start, "ArrowUp", viewport, min)?.height).toBe(672 - DIALOG_KEY_STEP);
    expect(stepDialogSize(min, "ArrowUp", viewport, min)).toEqual(min);
    expect(stepDialogSize(start, "Enter", viewport, min)).toBeNull();
  });
});

describe("snapDialogSize", () => {
  it("keeps whole pixel sizes at 100 % when the free space splits evenly", () => {
    expect(snapDialogSize(start, viewport, 1)).toEqual(start);
  });

  it("places the dialog's left and top edge on a whole device pixel", () => {
    for (const ratio of [1, 1.25, 1.5, 1.75, 2]) {
      const size = snapDialogSize(
        { width: 937, height: 671 },
        { width: 1536, height: 864.8 },
        ratio,
      );
      const left = ((1536 - size.width) / 2) * ratio;
      const top = ((864.8 - size.height) / 2) * ratio;
      expect(left).toBeCloseTo(Math.round(left), 6);
      expect(top).toBeCloseTo(Math.round(top), 6);
      expect(Math.abs(size.width - 937)).toBeLessThanOrEqual(2 / ratio);
    }
  });

  it("treats a missing ratio as 1", () => {
    expect(snapDialogSize(start, viewport, Number.NaN)).toEqual(start);
    expect(snapDialogSize(start, viewport, 0)).toEqual(start);
  });
});

describe("stored size", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("accepts only positive finite numbers", () => {
    expect(parseDialogSize({ width: 800, height: 500 })).toEqual({ width: 800, height: 500 });
    expect(parseDialogSize({ width: 800 })).toBeNull();
    expect(parseDialogSize({ width: "800", height: 500 })).toBeNull();
    expect(parseDialogSize({ width: -1, height: 500 })).toBeNull();
    expect(parseDialogSize({ width: Number.POSITIVE_INFINITY, height: 500 })).toBeNull();
    expect(parseDialogSize(null)).toBeNull();
    expect(parseDialogSize([800, 500])).toBeNull();
  });

  it("reads the JSON text and ignores anything broken", () => {
    expect(parseStoredDialogSize('{"width":800,"height":500}')).toEqual({
      width: 800,
      height: 500,
    });
    expect(parseStoredDialogSize("{width")).toBeNull();
    expect(parseStoredDialogSize(null)).toBeNull();
  });

  it("remembers, forgets and survives a missing storage", () => {
    const items = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => items.get(key) ?? null,
      setItem: (key: string, value: string) => items.set(key, value),
      removeItem: (key: string) => items.delete(key),
    });
    saveDialogSize("size", { width: 800, height: 500 });
    expect(loadDialogSize("size")).toEqual({ width: 800, height: 500 });
    saveDialogSize("size", null);
    expect(loadDialogSize("size")).toBeNull();

    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
      removeItem: () => {
        throw new Error("blocked");
      },
    });
    expect(loadDialogSize("size")).toBeNull();
    expect(() => {
      saveDialogSize("size", { width: 800, height: 500 });
    }).not.toThrow();
  });
});
