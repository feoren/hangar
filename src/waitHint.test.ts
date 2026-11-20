import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { Clock } from "./attention";
import {
  clampWaitMinutes,
  WAIT_MINUTES,
  WaitMonitor,
  waitingIds,
  waitsForPerson,
  waitThresholdMs,
} from "./waitHint";
import { withGate } from "./gate";

const MINUTE = 60_000;
const TEN = 10 * MINUTE;

const clock: Clock = {
  now: () => Date.now(),
  setTimeout: (callback, ms) => Number(setTimeout(callback, ms)),
  clearTimeout: (handle) => {
    clearTimeout(handle);
  },
};

describe("wait hint setting", () => {
  it("defaults to ten minutes, 0 is off", () => {
    expect(WAIT_MINUTES.default).toBe(10);
    expect(waitThresholdMs(0)).toBeNull();
    expect(waitThresholdMs(10)).toBe(TEN);
    expect(clampWaitMinutes(999)).toBe(WAIT_MINUTES.max);
    expect(clampWaitMinutes(-3)).toBe(0);
    expect(clampWaitMinutes(Number.NaN)).toBe(10);
  });

  it("counts only a session that waits for the person", () => {
    expect(waitsForPerson("attention", "quiet")).toBe(true);
    expect(waitsForPerson("attention", "bell")).toBe(true);
    expect(waitsForPerson("attention", "gate")).toBe(true);
    expect(waitsForPerson("attention", "exit")).toBe(false);
    // A session shown to the person goes back to idle: that is no waiting.
    expect(waitsForPerson("idle", null)).toBe(false);
    expect(waitsForPerson("working", null)).toBe(false);
  });
});

describe("waiting sessions as the rows show them", () => {
  it("counts a session waiting at the gate, which the tracker shows as seen", () => {
    const tracked = {
      attention: new Map([
        [1, "idle" as const],
        [2, "attention" as const],
        [3, "attention" as const],
        [4, "working" as const],
      ]),
      reasons: new Map([
        [2, "quiet" as const],
        [3, "exit" as const],
      ]),
    };
    const shown = withGate(tracked, new Set([1]));
    expect(waitingIds([1, 2, 3, 4, 5], shown.attention, shown.reasons)).toEqual(new Set([1, 2]));
    // Without the gate the seen session does not wait.
    expect(waitingIds([1, 2], tracked.attention, tracked.reasons)).toEqual(new Set([2]));
  });

  it("leaves out the sessions on screen, also one waiting at the gate", () => {
    const tracked = {
      attention: new Map([
        [1, "idle" as const],
        [2, "attention" as const],
        [3, "attention" as const],
      ]),
      reasons: new Map([
        [2, "quiet" as const],
        [3, "bell" as const],
      ]),
    };
    const shown = withGate(tracked, new Set([1]));
    expect(waitingIds([1, 2, 3], shown.attention, shown.reasons, new Set([1, 3]))).toEqual(
      new Set([2]),
    );
    // The gate's chip stays on the shown session: only the wait hint leaves it out.
    expect(shown.reasons.get(1)).toBe("gate");
  });
});

describe("WaitMonitor", () => {
  let reports: ReadonlyMap<number, number>[];
  let crossed: number[];
  let monitor: WaitMonitor;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    reports = [];
    crossed = [];
    monitor = new WaitMonitor(clock, (long) => reports.push(long));
    monitor.setOnCross((id) => crossed.push(id));
  });

  afterEach(() => {
    monitor.dispose();
    vi.useRealTimers();
  });

  it("reports a session once it waited as long as the threshold", () => {
    monitor.setThreshold(TEN);
    monitor.changed(1, true);
    vi.advanceTimersByTime(TEN - 1);
    expect(reports).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(reports).toEqual([new Map([[1, 0]])]);
    expect(crossed).toEqual([1]);
    // Nothing more happens while it keeps waiting.
    vi.advanceTimersByTime(TEN * 6);
    expect(reports).toHaveLength(1);
  });

  it("clears the hint when the session stops waiting", () => {
    monitor.setThreshold(TEN);
    monitor.changed(1, true);
    vi.advanceTimersByTime(TEN);
    monitor.changed(1, false);
    expect(reports.at(-1)).toEqual(new Map());
    // Waiting again starts anew.
    monitor.changed(1, true);
    vi.advanceTimersByTime(TEN - 1);
    expect(reports.at(-1)).toEqual(new Map());
    vi.advanceTimersByTime(1);
    expect(reports.at(-1)).toEqual(new Map([[1, TEN]]));
  });

  it("keeps the start of a session that keeps waiting", () => {
    monitor.setThreshold(TEN);
    monitor.changed(1, true);
    vi.advanceTimersByTime(4 * MINUTE);
    monitor.changed(1, true);
    vi.advanceTimersByTime(6 * MINUTE);
    expect(reports).toEqual([new Map([[1, 0]])]);
  });

  it("times each session by its own start", () => {
    monitor.setThreshold(TEN);
    monitor.changed(1, true);
    vi.advanceTimersByTime(3 * MINUTE);
    monitor.changed(2, true);
    vi.advanceTimersByTime(7 * MINUTE);
    expect(reports.at(-1)).toEqual(new Map([[1, 0]]));
    vi.advanceTimersByTime(3 * MINUTE);
    expect(reports.at(-1)).toEqual(
      new Map([
        [1, 0],
        [2, 3 * MINUTE],
      ]),
    );
    expect(crossed).toEqual([1, 2]);
    monitor.remove(1);
    expect(reports.at(-1)).toEqual(new Map([[2, 3 * MINUTE]]));
  });

  it("shows nothing while off, and keeps the times for when it is turned on", () => {
    monitor.setThreshold(null);
    monitor.changed(1, true);
    vi.advanceTimersByTime(TEN * 3);
    expect(reports).toEqual([]);
    monitor.setThreshold(TEN);
    expect(reports).toEqual([new Map([[1, 0]])]);
    // A changed setting lets it across at once: no taskbar flash for that.
    expect(crossed).toEqual([]);
    monitor.setThreshold(null);
    expect(reports.at(-1)).toEqual(new Map());
  });

  it("follows a changed threshold", () => {
    monitor.setThreshold(TEN);
    monitor.changed(1, true);
    vi.advanceTimersByTime(5 * MINUTE);
    monitor.setThreshold(20 * MINUTE);
    vi.advanceTimersByTime(10 * MINUTE);
    expect(reports).toEqual([]);
    vi.advanceTimersByTime(5 * MINUTE);
    expect(reports).toEqual([new Map([[1, 0]])]);
    expect(crossed).toEqual([1]);
  });

  it("sets no timer without waiting sessions", () => {
    monitor.setThreshold(TEN);
    expect(vi.getTimerCount()).toBe(0);
    monitor.changed(1, true);
    expect(vi.getTimerCount()).toBe(1);
    monitor.changed(1, false);
    expect(vi.getTimerCount()).toBe(0);
  });
});
