import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Clock } from "./attention";
import {
  clampStallMinutes,
  progressFingerprint,
  STALL_CPU_PERCENT,
  STALL_SAMPLE_MS,
  stallSince,
  stallThresholdMs,
  StallMonitor,
  type StallInput,
  type StallSource,
} from "./stall";

const MINUTE = 60_000;
const FIVE = 5 * MINUTE;

describe("progressFingerprint", () => {
  it("ignores spinner glyphs and elapsed times", () => {
    const a = progressFingerprint(["✻ Pondering… (12s · esc to interrupt)", "> "]);
    const b = progressFingerprint(["✶ Pondering… (3m 05s · esc to interrupt)", ">  "]);
    expect(a).toBe(b);
    expect(progressFingerprint(["⠋ Working 00:42"])).toBe(progressFingerprint(["⠙ Working 01:17"]));
  });

  it("sees new text and growing token counts as progress", () => {
    const before = progressFingerprint(["✻ Pondering… (12s · ↓ 1.2k tokens)"]);
    expect(progressFingerprint(["✻ Pondering… (14s · ↓ 1.4k tokens)"])).not.toBe(before);
    expect(
      progressFingerprint(["Edited src/app.ts", "✻ Pondering… (12s · ↓ 1.2k tokens)"]),
    ).not.toBe(before);
  });
});

describe("stallSince", () => {
  const base: StallInput = {
    working: true,
    detected: true,
    thresholdMs: FIVE,
    now: 10 * MINUTE,
    progressAt: 3 * MINUTE,
    busyAt: null,
  };

  it("reports the last progress once the threshold has passed", () => {
    expect(stallSince(base)).toBe(3 * MINUTE);
    expect(stallSince({ ...base, progressAt: 6 * MINUTE })).toBeNull();
  });

  it("stays quiet while the process tree was busy within the threshold", () => {
    expect(stallSince({ ...base, busyAt: 8 * MINUTE })).toBeNull();
    expect(stallSince({ ...base, busyAt: 4 * MINUTE })).toBe(3 * MINUTE);
  });

  it("never shows for other states, shells or with the hint off", () => {
    expect(stallSince({ ...base, working: false })).toBeNull();
    expect(stallSince({ ...base, detected: false })).toBeNull();
    expect(stallSince({ ...base, thresholdMs: null })).toBeNull();
  });

  it("maps the setting", () => {
    expect(stallThresholdMs(0)).toBeNull();
    expect(stallThresholdMs(5)).toBe(FIVE);
    expect(clampStallMinutes(99)).toBe(30);
    expect(clampStallMinutes(-1)).toBe(0);
    expect(clampStallMinutes(Number.NaN)).toBe(5);
  });
});

describe("StallMonitor", () => {
  const clock: Clock = {
    now: () => Date.now(),
    setTimeout: (callback, ms) => Number(setTimeout(callback, ms)),
    clearTimeout: (handle) => {
      clearTimeout(handle);
    },
  };
  let screen: string[];
  let source: StallSource;
  let reports: ReadonlyMap<number, number>[];
  let monitor: StallMonitor;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    screen = ["✻ Pondering… (1s · esc to interrupt)"];
    source = { id: 1, working: true, detected: true, lines: () => screen };
    reports = [];
    monitor = new StallMonitor(clock, (stalled) => reports.push(stalled));
    monitor.setSources(() => [source]);
    monitor.setThreshold(FIVE);
  });

  afterEach(() => {
    monitor.dispose();
    vi.useRealTimers();
  });

  /** Lets time pass while the spinner keeps ticking. */
  const tick = (ms: number) => {
    for (let passed = 0; passed < ms; passed += STALL_SAMPLE_MS) {
      screen = [`✻ Pondering… (${String(Math.round(Date.now() / 1000))}s · esc to interrupt)`];
      vi.advanceTimersByTime(STALL_SAMPLE_MS);
    }
  };

  it("hints once nothing new came for the threshold and reports it only once", () => {
    tick(FIVE);
    expect(reports).toEqual([]);
    tick(STALL_SAMPLE_MS);
    expect(reports).toHaveLength(1);
    expect(reports[0]?.get(1)).toBe(STALL_SAMPLE_MS);
    tick(2 * MINUTE);
    expect(reports).toHaveLength(1);
  });

  it("drops the hint with the next real output", () => {
    tick(FIVE + STALL_SAMPLE_MS);
    expect(reports.at(-1)?.has(1)).toBe(true);
    screen = ["Edited src/app.ts"];
    vi.advanceTimersByTime(STALL_SAMPLE_MS);
    expect(reports.at(-1)?.has(1)).toBe(false);
  });

  it("drops the hint at once on progress outside the screen", () => {
    tick(FIVE + STALL_SAMPLE_MS);
    monitor.progressed(1);
    expect(reports.at(-1)?.has(1)).toBe(false);
  });

  it("stays quiet while the process tree uses CPU", () => {
    for (let passed = 0; passed < 2 * FIVE; passed += STALL_SAMPLE_MS) {
      monitor.cpu(1, STALL_CPU_PERCENT + 4);
      tick(STALL_SAMPLE_MS);
    }
    expect(reports).toEqual([]);
    // Idle CPU from now on: the hint follows one threshold later.
    for (let passed = 0; passed <= FIVE; passed += STALL_SAMPLE_MS) {
      monitor.cpu(1, 0.2);
      tick(STALL_SAMPLE_MS);
    }
    expect(reports.at(-1)?.has(1)).toBe(true);
  });

  it("never hints for a session that is not working or not detected", () => {
    source = { ...source, working: false };
    tick(2 * FIVE);
    source = { id: 1, working: true, detected: false, lines: () => screen };
    tick(2 * FIVE);
    expect(reports).toEqual([]);
  });

  it("clears the hint when the session stops working", () => {
    tick(FIVE + STALL_SAMPLE_MS);
    source = { ...source, working: false };
    vi.advanceTimersByTime(STALL_SAMPLE_MS);
    expect(reports.at(-1)?.size).toBe(0);
  });

  it("stops sampling and clears when turned off", () => {
    tick(FIVE + STALL_SAMPLE_MS);
    monitor.setThreshold(null);
    expect(reports.at(-1)?.size).toBe(0);
    const count = reports.length;
    tick(2 * FIVE);
    expect(reports).toHaveLength(count);
    expect(vi.getTimerCount()).toBe(0);
  });
});
