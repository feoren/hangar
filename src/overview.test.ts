import { describe, expect, it } from "vitest";

import { gridColumns, gridMove, isGridKey, tailLines } from "./overview";
import type { RowSource } from "./sessionSearch";

function buffer(rows: readonly string[]): RowSource {
  return {
    length: rows.length,
    row: (index) => {
      const text = rows[index];
      return text === undefined ? undefined : { text, wrapped: false };
    },
  };
}

describe("gridColumns", () => {
  it("aims for a square grid", () => {
    expect(gridColumns(1, 1200)).toBe(1);
    expect(gridColumns(2, 1200)).toBe(2);
    expect(gridColumns(4, 1200)).toBe(2);
    expect(gridColumns(5, 1200)).toBe(3);
    expect(gridColumns(9, 1200)).toBe(3);
    expect(gridColumns(12, 1200)).toBe(4);
  });

  it("never puts more tiles in a row than fit", () => {
    expect(gridColumns(12, 600)).toBe(2);
    expect(gridColumns(12, 100)).toBe(1);
    expect(gridColumns(0, 0)).toBe(1);
  });
});

describe("gridMove", () => {
  // 7 tiles in 3 columns:
  // 0 1 2
  // 3 4 5
  // 6
  it("moves left and right in reading order and stops at the ends", () => {
    expect(gridMove(2, 7, 3, "ArrowRight")).toBe(3);
    expect(gridMove(6, 7, 3, "ArrowRight")).toBe(6);
    expect(gridMove(0, 7, 3, "ArrowLeft")).toBe(0);
  });

  it("moves up and down within the column", () => {
    expect(gridMove(1, 7, 3, "ArrowDown")).toBe(4);
    expect(gridMove(4, 7, 3, "ArrowUp")).toBe(1);
    expect(gridMove(1, 7, 3, "ArrowUp")).toBe(1);
  });

  it("goes to the last tile from above a gap in the last row", () => {
    expect(gridMove(4, 7, 3, "ArrowDown")).toBe(6);
    expect(gridMove(6, 7, 3, "ArrowDown")).toBe(6);
  });

  it("jumps with Home and End and copes with no tiles", () => {
    expect(gridMove(4, 7, 3, "Home")).toBe(0);
    expect(gridMove(0, 7, 3, "End")).toBe(6);
    expect(gridMove(0, 0, 3, "ArrowDown")).toBe(-1);
  });

  it("knows its keys", () => {
    expect(isGridKey("ArrowUp")).toBe(true);
    expect(isGridKey("Enter")).toBe(false);
  });
});

describe("tailLines", () => {
  it("shows the last rows with content, without the blank screen below", () => {
    const source = buffer(["one", "two  ", "three", "", "   ", ""]);
    expect(tailLines(source, 2)).toEqual(["two", "three"]);
    expect(tailLines(source, 10)).toEqual(["one", "two", "three"]);
  });

  it("is empty for an empty terminal", () => {
    expect(tailLines(buffer(["", ""]), 5)).toEqual([]);
  });
});
