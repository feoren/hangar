import { describe, expect, it } from "vitest";

import {
  DEFAULT_ROW_STYLE,
  isRowStyle,
  loadSummary,
  parseRowStyle,
  ROW_STYLES,
  rowExpanded,
  rowParts,
  stateCounts,
  toggleRow,
} from "./rowStyle";

describe("row style setting", () => {
  it("is the standard row by default", () => {
    expect(DEFAULT_ROW_STYLE).toBe("standard");
    expect(parseRowStyle(undefined)).toBe("standard");
  });

  it("keeps every known style", () => {
    expect(ROW_STYLES).toHaveLength(14);
    for (const style of ROW_STYLES) {
      expect(isRowStyle(style)).toBe(true);
      expect(parseRowStyle(style)).toBe(style);
    }
  });

  it("falls back for unknown or wrong values", () => {
    for (const value of ["fancy", "", "Standard", 3, null, {}, ["pill"]]) {
      expect(isRowStyle(value)).toBe(false);
      expect(parseRowStyle(value)).toBe(DEFAULT_ROW_STYLE);
    }
  });
});

describe("the parts each style adds", () => {
  it("adds nothing to the styles that are pure CSS", () => {
    for (const style of ["pill", "tags", "edge", "band"] as const) {
      expect(rowParts(style)).toEqual(rowParts("standard"));
    }
    expect(rowParts("standard")).toMatchObject({
      avatar: null,
      ring: null,
      expander: false,
      meter: "bar",
      load: "line",
      runtime: "context",
      end: "word",
      counters: false,
      peekFacts: false,
    });
  });

  it("gives each other style only what it needs", () => {
    expect(rowParts("avatar").avatar).toBe("dot");
    expect(rowParts("accordion")).toMatchObject({ expander: true, mini: "title" });
    expect(rowParts("ring")).toMatchObject({ ring: "start", meter: "none", runtime: "load" });
    expect(rowParts("tiles").load).toBe("tiles");
    expect(rowParts("segments").meter).toBe("segments");
    expect(rowParts("compact")).toMatchObject({
      mini: "meta",
      load: "none",
      meter: "none",
      runtime: "none",
      peekFacts: true,
    });
    expect(rowParts("counters")).toMatchObject({
      end: "runtime",
      runtime: "title",
      counters: true,
    });
    expect(rowParts("columns")).toMatchObject({ end: "columns", load: "none", peekFacts: true });
    expect(rowParts("card")).toMatchObject({ avatar: "plain", ring: "end", runtime: "load" });
  });
});

describe("state counts of a group", () => {
  it("counts each state in a fixed order and leaves out the empty ones", () => {
    expect(stateCounts(["done", "working", null, "working", "asks", "paused"])).toEqual([
      { kind: "working", count: 2 },
      { kind: "asks", count: 1 },
      { kind: "done", count: 1 },
      { kind: "paused", count: 1 },
    ]);
    expect(stateCounts([null, null])).toEqual([]);
  });
});

describe("folding rows", () => {
  it("shows the active row's details until it is folded by hand", () => {
    const none = new Map<number, boolean>();
    expect(rowExpanded(none, 1, true)).toBe(true);
    expect(rowExpanded(none, 2, false)).toBe(false);
    const folded = toggleRow(none, 1, true);
    expect(rowExpanded(folded, 1, true)).toBe(false);
    expect(rowExpanded(toggleRow(folded, 1, true), 1, true)).toBe(true);
  });

  it("unfolds another row by hand and keeps it open when it is no longer active", () => {
    const opened = toggleRow(new Map(), 2, false);
    expect(rowExpanded(opened, 2, false)).toBe(true);
    expect(rowExpanded(opened, 2, true)).toBe(true);
    // The map the row held before stays as it was.
    expect(toggleRow(opened, 3, false)).not.toBe(opened);
    expect(opened.has(3)).toBe(false);
  });
});

describe("load for the peek", () => {
  const names = { cpu: "CPU", memory: "RAM", gpu: "GPU" };

  it("names each value that is there", () => {
    expect(
      loadSummary(
        { cpu: "12%", memory: "476 MB", gpu: "30% · 1.2 GB", gpuLoad: "30%", gpuMemory: "1.2 GB" },
        names,
      ),
    ).toBe("CPU 12% · RAM 476 MB · GPU 30% · 1.2 GB");
    expect(
      loadSummary({ cpu: null, memory: "64 MB", gpu: null, gpuLoad: null, gpuMemory: null }, names),
    ).toBe("RAM 64 MB");
  });
});
