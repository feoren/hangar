import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BRANCH_REFRESH_MS } from "./gitBranches";
import {
  parsePowerState,
  pollInterval,
  PollTimer,
  powerKnown,
  PowerStore,
  savingActive,
  type Poll,
} from "./power";
import { RESOURCE_POLL_MS } from "./resources";
import { SYSTEM_POLL_MS } from "./systemUsage";

const POLLS: readonly Poll[] = ["system", "resources", "branches"];

describe("parsePowerState", () => {
  it("takes the backend's states and nothing else", () => {
    for (const state of ["unknown", "noBattery", "ac", "battery", "saver"] as const) {
      expect(parsePowerState(state)).toBe(state);
    }
    for (const value of [undefined, null, 1, "Battery", "AC", { state: "battery" }]) {
      expect(parsePowerState(value)).toBe("unknown");
    }
  });
});

describe("powerKnown", () => {
  it("offers the setting only on a machine whose battery Hangar can watch", () => {
    expect(powerKnown("ac")).toBe(true);
    expect(powerKnown("battery")).toBe(true);
    expect(powerKnown("saver")).toBe(true);
    expect(powerKnown("noBattery")).toBe(false);
    expect(powerKnown("unknown")).toBe(false);
  });
});

describe("savingActive", () => {
  it("saves only on battery and with the setting on", () => {
    expect(savingActive("battery", true)).toBe(true);
    expect(savingActive("saver", true)).toBe(true);
    expect(savingActive("battery", false)).toBe(false);
    for (const state of ["ac", "noBattery", "unknown"] as const) {
      expect(savingActive(state, true)).toBe(false);
    }
  });
});

describe("pollInterval", () => {
  it("keeps the normal intervals on AC", () => {
    expect(pollInterval("system", false)).toBe(SYSTEM_POLL_MS);
    expect(pollInterval("resources", false)).toBe(RESOURCE_POLL_MS);
    expect(pollInterval("branches", false)).toBe(BRANCH_REFRESH_MS);
  });

  it("polls less often while saving", () => {
    for (const poll of POLLS) {
      expect(pollInterval(poll, true)).toBeGreaterThan(pollInterval(poll, false));
    }
    expect(pollInterval("system", true)).toBe(5000);
    expect(pollInterval("resources", true)).toBe(5000);
    expect(pollInterval("branches", true)).toBe(60_000);
  });
});

describe("PowerStore", () => {
  it("starts unknown and not saving", () => {
    const store = new PowerStore();
    expect(store.state()).toBe("unknown");
    expect(store.saving()).toBe(false);
  });

  it("saves on battery and stops at once back on AC", () => {
    const store = new PowerStore();
    store.accept("battery");
    expect(store.saving()).toBe(true);
    store.accept("ac");
    expect(store.saving()).toBe(false);
  });

  it("follows the setting", () => {
    const store = new PowerStore();
    store.accept("saver");
    store.setEnabled(false);
    expect(store.saving()).toBe(false);
    store.setEnabled(true);
    expect(store.saving()).toBe(true);
  });

  it("tells its listeners only about a change", () => {
    const store = new PowerStore();
    let calls = 0;
    const stop = store.subscribe(() => {
      calls += 1;
    });
    store.accept("ac");
    store.accept("ac");
    store.setEnabled(true);
    expect(calls).toBe(1);
    store.accept("battery");
    store.setEnabled(false);
    expect(calls).toBe(3);
    stop();
    store.accept("ac");
    expect(calls).toBe(3);
  });

  it("treats a malformed answer as unknown", () => {
    const store = new PowerStore();
    store.accept("battery");
    store.accept(42);
    expect(store.state()).toBe("unknown");
    expect(store.saving()).toBe(false);
  });
});

describe("PollTimer", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("runs at once, then every interval, with only one timer", () => {
    let runs = 0;
    const timer = new PollTimer(() => {
      runs += 1;
    });
    timer.set(true, 1500);
    timer.set(true, 1500);
    expect(runs).toBe(1);
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(4500);
    expect(runs).toBe(4);
  });

  it("starts over with the new interval when saving turns on and off", () => {
    let runs = 0;
    const timer = new PollTimer(() => {
      runs += 1;
    });
    timer.set(true, pollInterval("system", false));
    timer.set(true, pollInterval("system", true));
    expect(runs).toBe(2);
    expect(vi.getTimerCount()).toBe(1);
    vi.advanceTimersByTime(4999);
    expect(runs).toBe(2);
    vi.advanceTimersByTime(1);
    expect(runs).toBe(3);
    timer.set(true, pollInterval("system", false));
    expect(runs).toBe(4);
    vi.advanceTimersByTime(SYSTEM_POLL_MS);
    expect(runs).toBe(5);
    expect(vi.getTimerCount()).toBe(1);
  });

  it("stops when no longer wanted and does not start while unwanted", () => {
    let runs = 0;
    const timer = new PollTimer(() => {
      runs += 1;
    });
    timer.set(false, 5000);
    expect(runs).toBe(0);
    expect(vi.getTimerCount()).toBe(0);
    timer.set(true, 5000);
    timer.set(false, 5000);
    expect(vi.getTimerCount()).toBe(0);
    // An interval change while stopped starts nothing.
    timer.set(false, 1500);
    vi.advanceTimersByTime(60_000);
    expect(runs).toBe(1);
  });
});
