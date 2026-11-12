import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import { formatCpu, formatMemory, parseResources, resourceText, sameResources } from "./resources";

const MB = 1024 * 1024;

describe("parseResources", () => {
  it("reads the backend's answer per session", () => {
    const parsed = parseResources([
      { id: 1, cpu: 12.5, memory: 340 * MB, outlier: false },
      { id: 2, cpu: null, memory: 0, outlier: true },
    ]);
    expect(parsed).toEqual(
      new Map([
        [1, { cpu: 12.5, memory: 340 * MB, outlier: false }],
        [2, { cpu: null, memory: 0, outlier: true }],
      ]),
    );
  });

  it("rejects malformed answers as a whole", () => {
    expect(parseResources(null)).toBeNull();
    expect(parseResources({})).toBeNull();
    expect(parseResources([{ id: 1, cpu: 1, memory: -1, outlier: false }])).toBeNull();
    expect(parseResources([{ id: 1.5, cpu: 1, memory: 1, outlier: false }])).toBeNull();
    expect(parseResources([{ id: 1, cpu: "1", memory: 1, outlier: false }])).toBeNull();
    expect(parseResources([{ id: 1, cpu: 1, memory: 1 }])).toBeNull();
  });

  it("caps the share at 100 %", () => {
    expect(parseResources([{ id: 1, cpu: 250, memory: 1, outlier: false }])?.get(1)?.cpu).toBe(100);
  });
});

describe("formatting", () => {
  it("uses megabytes below a gigabyte and one decimal above", () => {
    expect(formatMemory(340 * MB, "en")).toBe("340 MB");
    expect(formatMemory(4.25 * 1024 * MB, "en")).toBe("4.3 GB");
    expect(formatMemory(340 * MB, "fr")).toMatch(/^340\s?Mo$/u);
  });

  it("keeps a decimal for a share below one percent", () => {
    expect(formatCpu(12.4, "en")).toBe("12%");
    expect(formatCpu(0.4, "en")).toBe("0.4%");
    expect(formatCpu(0, "en")).toBe("0%");
  });

  it("says CPU and memory, or the memory alone before the second reading", () => {
    const t = createTranslator("en");
    expect(resourceText({ cpu: 12, memory: 340 * MB, outlier: false }, "en", t)).toBe(
      "CPU 12% · 340 MB",
    );
    expect(resourceText({ cpu: null, memory: 340 * MB, outlier: false }, "en", t)).toBe(
      "Memory 340 MB",
    );
  });

  it("compares what is shown", () => {
    const a = { cpu: 1, memory: 2, outlier: false };
    expect(sameResources(a, { ...a })).toBe(true);
    expect(sameResources(a, { ...a, outlier: true })).toBe(false);
  });
});
