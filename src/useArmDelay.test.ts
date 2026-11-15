import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ARM_DELAY_MS, ArmClock } from "./useArmDelay";

describe("ArmClock", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  const clock = () => new ArmClock(() => Date.now());

  it("ignores clicks right after it starts", () => {
    const arm = clock();
    expect(arm.ready()).toBe(false);
    vi.advanceTimersByTime(ARM_DELAY_MS - 1);
    expect(arm.ready()).toBe(false);
    vi.advanceTimersByTime(1);
    expect(arm.ready()).toBe(true);
  });

  it("starts over when what it guards changes", () => {
    const arm = clock();
    vi.advanceTimersByTime(ARM_DELAY_MS);
    expect(arm.ready()).toBe(true);
    arm.restart();
    expect(arm.ready()).toBe(false);
    expect(arm.remaining()).toBe(ARM_DELAY_MS);
    vi.advanceTimersByTime(ARM_DELAY_MS);
    expect(arm.ready()).toBe(true);
    expect(arm.remaining()).toBe(0);
  });

  it("lets a guarded action through only once armed", () => {
    const arm = clock();
    const action = vi.fn();
    const guarded = arm.guard(action);
    guarded();
    expect(action).not.toHaveBeenCalled();
    vi.advanceTimersByTime(ARM_DELAY_MS);
    guarded();
    expect(action).toHaveBeenCalledTimes(1);
  });
});
