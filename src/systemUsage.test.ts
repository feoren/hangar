import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import {
  advance,
  barPending,
  EMPTY_STATE,
  formatShare,
  gaugeAmount,
  gaugeAmountLabel,
  gaugeWarns,
  HISTORY_LENGTH,
  parseSystemUsage,
  pushHistory,
  SPARK_HEIGHT,
  SPARK_WIDTH,
  sparkPaths,
  SYSTEM_POLL_MS,
  WARN_PERCENT,
  type SystemUsage,
} from "./systemUsage";

const GB = 1024 * 1024 * 1024;
const NO_AMOUNTS = { memoryUsed: null, memoryTotal: null, gpuMemory: null };

function reading(cpu: number | null, memory: number | null, gpu: number | null): SystemUsage {
  return { cpu, memory, gpu, ...NO_AMOUNTS };
}

describe("parseSystemUsage", () => {
  it("accepts shares and unknown values, also without amounts", () => {
    expect(parseSystemUsage({ cpu: 23.4, memory: 61, gpu: null })).toEqual(reading(23.4, 61, null));
  });

  it("reads the amounts in bytes", () => {
    expect(
      parseSystemUsage({
        cpu: 1,
        memory: 62,
        gpu: 30,
        memoryUsed: 19.8 * GB,
        memoryTotal: 32 * GB,
        gpuMemory: null,
      }),
    ).toEqual({ ...reading(1, 62, 30), memoryUsed: 19.8 * GB, memoryTotal: 32 * GB });
  });

  it("clamps to 0 to 100", () => {
    expect(parseSystemUsage({ cpu: 140, memory: -3, gpu: 0 })).toEqual(reading(100, 0, 0));
  });

  it("rejects anything malformed", () => {
    expect(parseSystemUsage(null)).toBeNull();
    expect(parseSystemUsage([])).toBeNull();
    expect(parseSystemUsage({ cpu: "1", memory: 2, gpu: 3 })).toBeNull();
    expect(parseSystemUsage({ cpu: 1, memory: 2 })).toBeNull();
    expect(parseSystemUsage({ cpu: Number.NaN, memory: 2, gpu: 3 })).toBeNull();
    expect(parseSystemUsage({ cpu: 1, memory: 2, gpu: 3, memoryUsed: -1 })).toBeNull();
    expect(parseSystemUsage({ cpu: 1, memory: 2, gpu: 3, gpuMemory: "1" })).toBeNull();
  });
});

describe("history", () => {
  it("covers the last minute", () => {
    expect((HISTORY_LENGTH - 1) * SYSTEM_POLL_MS).toBeGreaterThanOrEqual(60_000);
  });

  it("keeps the newest readings, oldest first", () => {
    let history: readonly number[] = [];
    for (let value = 0; value < HISTORY_LENGTH + 5; value += 1) {
      history = pushHistory(history, value);
    }
    expect(history).toHaveLength(HISTORY_LENGTH);
    expect(history[0]).toBe(5);
    expect(history.at(-1)).toBe(HISTORY_LENGTH + 4);
  });

  it("starts over when a value becomes unknown", () => {
    expect(pushHistory([1, 2, 3], null)).toEqual([]);
    const empty: readonly number[] = [];
    expect(pushHistory(empty, null)).toBe(empty);
  });
});

describe("advance", () => {
  const usage = reading(10, 50, null);

  it("publishes a changed reading and moves every history on", () => {
    const { state, changed } = advance(EMPTY_STATE, usage);
    expect(changed).toBe(true);
    expect(state.usage).toBe(usage);
    expect(state.history).toEqual({ cpu: [10], memory: [50], gpu: [] });
    expect(state.tick).toBe(1);
  });

  it("does not publish an equal reading, but still records it", () => {
    const first = advance(EMPTY_STATE, usage).state;
    const { state, changed } = advance(first, { ...usage });
    expect(changed).toBe(false);
    expect(advance(first, { ...usage, memoryUsed: 1 }).changed).toBe(true);
    // The same object: nothing for React to render.
    expect(state.usage).toBe(first.usage);
    expect(state.history.cpu).toEqual([10, 10]);
  });
});

describe("barPending", () => {
  it("waits for the second reading while the CPU is unknown", () => {
    const first = reading(null, 40, null);
    expect(barPending(first, 1)).toBe(true);
    expect(barPending(first, 2)).toBe(false);
  });

  it("shows a first reading that already has a CPU", () => {
    expect(barPending(reading(5, 40, null), 1)).toBe(false);
  });
});

describe("gaugeWarns", () => {
  it("warns from the threshold on", () => {
    expect(gaugeWarns(WARN_PERCENT - 0.1)).toBe(false);
    expect(gaugeWarns(WARN_PERCENT)).toBe(true);
    expect(gaugeWarns(100)).toBe(true);
  });
});

describe("sparkPaths", () => {
  it("needs two readings", () => {
    expect(sparkPaths([])).toBeNull();
    expect(sparkPaths([40])).toBeNull();
  });

  it("puts the newest reading at the right edge, one unit per reading", () => {
    const paths = sparkPaths([0, 50, 100]);
    const left = SPARK_WIDTH - 2;
    expect(paths?.line).toBe(
      `M${String(left)} ${String(SPARK_HEIGHT)} L${String(left + 1)} ${String(SPARK_HEIGHT / 2)} L${String(SPARK_WIDTH)} 0`,
    );
    expect(paths?.area).toBe(
      `${paths?.line ?? ""} L${String(SPARK_WIDTH)} ${String(SPARK_HEIGHT)} L${String(left)} ${String(SPARK_HEIGHT)} Z`,
    );
  });

  it("starts at the left edge with a full minute", () => {
    const full = Array.from({ length: HISTORY_LENGTH }, () => 25);
    expect(sparkPaths(full)?.line.startsWith("M0 12")).toBe(true);
  });
});

describe("formatShare", () => {
  it("rounds to whole percent in the UI language's style", () => {
    expect(formatShare(23.4, "en")).toBe("23%");
    expect(formatShare(61.5, "de")).toBe("62 %");
  });
});

describe("gauge amounts", () => {
  const usage: SystemUsage = {
    ...reading(10, 62, 30),
    memoryUsed: 19.8 * GB,
    memoryTotal: 31.9 * GB,
    gpuMemory: 3.1 * GB,
  };

  it("gives the memory in use behind RAM and GPU, none for the CPU", () => {
    expect(gaugeAmount("memory", usage, "en")).toBe("19.8 GB");
    expect(gaugeAmount("gpu", usage, "en")).toBe("3.1 GB");
    expect(gaugeAmount("cpu", usage, "en")).toBeNull();
    expect(gaugeAmount("gpu", { ...usage, gpuMemory: 0 }, "en")).toBeNull();
    expect(gaugeAmount("memory", { ...usage, memoryUsed: null }, "en")).toBeNull();
  });

  it("spells out used of total for the tooltip", () => {
    const t = createTranslator("en");
    expect(gaugeAmountLabel("memory", usage, "en", t)).toBe("19.8 GB of 31.9 GB");
    expect(gaugeAmountLabel("memory", { ...usage, memoryTotal: null }, "en", t)).toBe("19.8 GB");
    expect(gaugeAmountLabel("gpu", usage, "en", t)).toBe("3.1 GB");
    expect(gaugeAmountLabel("cpu", usage, "en", t)).toBeNull();
    expect(gaugeAmountLabel("memory", usage, "de", createTranslator("de"))).toMatch(
      /^19,8\sGB von 31,9\sGB$/u,
    );
  });
});
