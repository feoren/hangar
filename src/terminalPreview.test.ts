import { describe, expect, it } from "vitest";

import { lineWidth, PREVIEW_LINES, TILE_LINES } from "./terminalPreview";

describe("terminal preview sample", () => {
  it("draws a closed frame: every framed line has the same width", () => {
    const framed = PREVIEW_LINES.filter((line) => /^[╭│╰]/.test(line[0]?.[1] ?? ""));
    expect(framed).toHaveLength(4);
    const widths = new Set(framed.map(lineWidth));
    expect(widths.size).toBe(1);
  });

  it("closes the frame on the right with the matching corners", () => {
    const ends = PREVIEW_LINES.map((line) => line.at(-1)?.[1].at(-1));
    expect(ends).toContain("╮");
    expect(ends).toContain("╯");
    expect(ends.filter((end) => end === "│")).toHaveLength(2);
  });

  it("shows every accent and the dimmed gray", () => {
    const colors = new Set<string | null>(
      [...PREVIEW_LINES, ...TILE_LINES].flat().map(([color]) => color),
    );
    for (const color of ["red", "green", "yellow", "blue", "magenta", "cyan", "brightBlack"]) {
      expect(colors.has(color), color).toBe(true);
    }
  });

  it("keeps the tile sample short enough for a small tile", () => {
    for (const line of TILE_LINES) expect(lineWidth(line)).toBeLessThanOrEqual(18);
  });
});
