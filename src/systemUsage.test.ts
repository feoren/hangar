import { describe, expect, it } from "vitest";

import {
  advance,
  barPending,
  EMPTY_STATE,
  formatShare,
  gaugeWarns,
  HISTORY_LENGTH,
  parseSystemUsage,
  pushHistory,
  SPARK_HEIGHT,
  SPARK_WIDTH,
  sparkPaths,
  SYSTEM_POLL_MS,
  WARN_PERCENT,
} from "./systemUsage";

describe("parseSystemUsage", () => {
  it("accepts shares and unknown values", () => {
    expect(parseSystemUsage({ cpu: 23.4, memory: 61, gpu: null })).toEqual({
      cpu: 23.4,
      memory: 61,
      gpu: null,
    });
  });

  it("clamps to 0 to 100", () => {
    expect(parseSystemUsage({ cpu: 140, memory: -3, gpu: 0 })).toEqual({
      cpu: 100,
      memory: 0,
      gpu: 0,
    });
  });

  it("rejects anything malformed", () => {
    expect(parseSystemUsage(null)).toBeNull();
    expect(parseSystemUsage([])).toBeNull();
    expect(parseSystemUsage({ cpu: "1", memory: 2, gpu: 3 })).toBeNull();
    expect(parseSystemUsage({ cpu: 1, memory: 2 })).toBeNull();
    expect(parseSystemUsage({ cpu: Number.NaN, memory: 2, gpu: 3 })).toBeNull();
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
  const usage = { cpu: 10, memory: 50, gpu: null };

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
    // The same object: nothing for React to render.
    expect(state.usage).toBe(first.usage);
    expect(state.history.cpu).toEqual([10, 10]);
  });
});

describe("barPending", () => {
  it("waits for the second reading while the CPU is unknown", () => {
    const first = { cpu: null, memory: 40, gpu: null };
    expect(barPending(first, 1)).toBe(true);
    expect(barPending(first, 2)).toBe(false);
  });

  it("shows a first reading that already has a CPU", () => {
    expect(barPending({ cpu: 5, memory: 40, gpu: null }, 1)).toBe(false);
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
