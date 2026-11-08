import { describe, expect, it } from "vitest";

import {
  ClickGuard,
  CLOSE_CLICK_GUARD_MS,
  isDue,
  PROFILES_REFRESH_MIN_MS,
  RESTART_LOCK_MS,
  SessionActionGuard,
} from "./timing";

describe("ClickGuard", () => {
  it("lets every click through initially", () => {
    expect(new ClickGuard().blocked(0)).toBe(false);
  });

  it("blocks for exactly the given time after arming", () => {
    const guard = new ClickGuard();
    guard.arm(1000, CLOSE_CLICK_GUARD_MS);
    expect(guard.blocked(1000)).toBe(true);
    expect(guard.blocked(1399)).toBe(true);
    expect(guard.blocked(1400)).toBe(false);
  });

  it("does not shorten a running block", () => {
    const guard = new ClickGuard();
    guard.arm(0, 600);
    guard.arm(100, 100);
    expect(guard.blocked(500)).toBe(true);
    expect(guard.blocked(600)).toBe(false);
  });
});

describe("isDue", () => {
  it("is due the first time and after the interval", () => {
    expect(isDue(null, 0, PROFILES_REFRESH_MIN_MS)).toBe(true);
    expect(isDue(0, 29_999, PROFILES_REFRESH_MIN_MS)).toBe(false);
    expect(isDue(0, 30_000, PROFILES_REFRESH_MIN_MS)).toBe(true);
  });
});

describe("SessionActionGuard", () => {
  it("drops clicks shortly after closing", () => {
    let now = 0;
    const guard = new SessionActionGuard(() => now);
    expect(guard.allows()).toBe(true);
    guard.closed();
    now = CLOSE_CLICK_GUARD_MS - 1;
    expect(guard.allows()).toBe(false);
    expect(guard.allowsRestart(1)).toBe(false);
    now = CLOSE_CLICK_GUARD_MS;
    expect(guard.allows()).toBe(true);
  });

  it("locks the restart of only the ended session for a short time", () => {
    let now = 1000;
    const guard = new SessionActionGuard(() => now);
    guard.killed(7);
    now += RESTART_LOCK_MS - 1;
    expect(guard.allowsRestart(7)).toBe(false);
    expect(guard.allowsRestart(8)).toBe(true);
    now += 1;
    expect(guard.allowsRestart(7)).toBe(true);
  });
});
