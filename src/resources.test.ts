import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import {
  formatCpu,
  formatGpu,
  formatMemory,
  parseResources,
  resourceParts,
  resourceText,
  sameResources,
  type SessionResources,
} from "./resources";

const MB = 1024 * 1024;
const GB = 1024 * MB;

const QUIET: SessionResources = {
  cpu: 12,
  memory: 340 * MB,
  gpu: null,
  gpuMemory: null,
  outlier: false,
};

describe("parseResources", () => {
  it("reads the backend's answer per session", () => {
    const parsed = parseResources([
      { id: 1, cpu: 12.5, memory: 340 * MB, gpu: 30, gpuMemory: 1.2 * GB, outlier: false },
      { id: 2, cpu: null, memory: 0, gpu: null, gpuMemory: null, outlier: true },
    ]);
    expect(parsed).toEqual(
      new Map([
        [1, { cpu: 12.5, memory: 340 * MB, gpu: 30, gpuMemory: 1.2 * GB, outlier: false }],
        [2, { cpu: null, memory: 0, gpu: null, gpuMemory: null, outlier: true }],
      ]),
    );
  });

  it("accepts answers without GPU fields", () => {
    expect(parseResources([{ id: 1, cpu: 1, memory: 2, outlier: false }])?.get(1)).toEqual({
      cpu: 1,
      memory: 2,
      gpu: null,
      gpuMemory: null,
      outlier: false,
    });
  });

  it("rejects malformed answers as a whole", () => {
    expect(parseResources(null)).toBeNull();
    expect(parseResources({})).toBeNull();
    expect(parseResources([{ id: 1, cpu: 1, memory: -1, outlier: false }])).toBeNull();
    expect(parseResources([{ id: 1.5, cpu: 1, memory: 1, outlier: false }])).toBeNull();
    expect(parseResources([{ id: 1, cpu: "1", memory: 1, outlier: false }])).toBeNull();
    expect(parseResources([{ id: 1, cpu: 1, memory: 1 }])).toBeNull();
    expect(parseResources([{ id: 1, cpu: 1, memory: 1, outlier: false, gpu: "5" }])).toBeNull();
    expect(parseResources([{ id: 1, cpu: 1, memory: 1, outlier: false, gpu: -1 }])).toBeNull();
    expect(
      parseResources([{ id: 1, cpu: 1, memory: 1, outlier: false, gpuMemory: Number.NaN }]),
    ).toBeNull();
  });

  it("caps the shares at 100 %", () => {
    const parsed = parseResources([{ id: 1, cpu: 250, memory: 1, gpu: 140, outlier: false }]);
    expect(parsed?.get(1)?.cpu).toBe(100);
    expect(parsed?.get(1)?.gpu).toBe(100);
  });
});

describe("formatting", () => {
  it("uses megabytes below a gigabyte and one decimal above", () => {
    expect(formatMemory(340 * MB, "en")).toBe("340 MB");
    expect(formatMemory(4.25 * GB, "en")).toBe("4.3 GB");
    expect(formatMemory(340 * MB, "fr")).toMatch(/^340\s?Mo$/u);
  });

  it("keeps a decimal for a share below one percent", () => {
    expect(formatCpu(12.4, "en")).toBe("12%");
    expect(formatCpu(0.4, "en")).toBe("0.4%");
    expect(formatCpu(0, "en")).toBe("0%");
  });

  it("shows the GPU only when the tree really uses it", () => {
    expect(formatGpu(QUIET, "en")).toBeNull();
    expect(formatGpu({ ...QUIET, gpu: 0, gpuMemory: 0 }, "en")).toBeNull();
    expect(formatGpu({ ...QUIET, gpu: 30, gpuMemory: 1.2 * GB }, "en")).toBe("30% 1.2 GB");
    expect(formatGpu({ ...QUIET, gpu: 30, gpuMemory: 0 }, "en")).toBe("30%");
    expect(formatGpu({ ...QUIET, gpu: 0, gpuMemory: 64 * MB }, "en")).toBe("64 MB");
    expect(formatGpu({ ...QUIET, gpu: 30, gpuMemory: 1.2 * GB }, "de")).toMatch(
      /^30\s%\s1,2\sGB$/u,
    );
  });

  it("gives the row's parts, without CPU before the second reading", () => {
    expect(resourceParts(QUIET, "en")).toEqual({ cpu: "12%", memory: "340 MB", gpu: null });
    expect(resourceParts({ ...QUIET, cpu: null, gpu: 8 }, "en")).toEqual({
      cpu: null,
      memory: "340 MB",
      gpu: "8%",
    });
  });

  it("says CPU, memory and GPU, or the memory alone before the second reading", () => {
    const t = createTranslator("en");
    expect(resourceText(QUIET, "en", t)).toBe("CPU 12% · 340 MB");
    expect(resourceText({ ...QUIET, cpu: null }, "en", t)).toBe("Memory 340 MB");
    expect(resourceText({ ...QUIET, gpu: 30, gpuMemory: 1.2 * GB }, "en", t)).toBe(
      "CPU 12% · 340 MB · GPU 30% 1.2 GB",
    );
  });

  it("compares what is shown", () => {
    expect(sameResources(QUIET, { ...QUIET })).toBe(true);
    expect(sameResources(QUIET, { ...QUIET, outlier: true })).toBe(false);
    expect(sameResources(QUIET, { ...QUIET, gpu: 1 })).toBe(false);
    expect(sameResources(QUIET, { ...QUIET, gpuMemory: 1 })).toBe(false);
  });
});
