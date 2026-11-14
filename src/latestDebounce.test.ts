import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { LatestDebounce, PTY_RESIZE_DEBOUNCE_MS } from "./latestDebounce";

describe("LatestDebounce", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("sends only the last value of a burst, once the burst rests", () => {
    const send = vi.fn();
    const debounce = new LatestDebounce<number>(send, PTY_RESIZE_DEBOUNCE_MS);
    for (let size = 1; size <= 30; size += 1) {
      debounce.push(size);
      vi.advanceTimersByTime(16);
    }
    expect(send).not.toHaveBeenCalled();
    expect(debounce.pending).toBe(true);
    vi.advanceTimersByTime(PTY_RESIZE_DEBOUNCE_MS - 16 - 1);
    expect(send).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(send).toHaveBeenCalledTimes(1);
    expect(send).toHaveBeenCalledWith(30);
    expect(debounce.pending).toBe(false);
  });

  it("never loses the final value, even when it equals an earlier one", () => {
    const send = vi.fn();
    const debounce = new LatestDebounce<string>(send, 100);
    debounce.push("80x24");
    vi.advanceTimersByTime(100);
    debounce.push("120x40");
    debounce.push("80x24");
    vi.advanceTimersByTime(100);
    expect(send.mock.calls).toEqual([["80x24"], ["80x24"]]);
  });

  it("sends a waiting value at once on flush, and nothing twice", () => {
    const send = vi.fn();
    const debounce = new LatestDebounce<number>(send, 100);
    debounce.flush();
    expect(send).not.toHaveBeenCalled();
    debounce.push(7);
    debounce.flush();
    expect(send).toHaveBeenCalledWith(7);
    vi.advanceTimersByTime(500);
    expect(send).toHaveBeenCalledTimes(1);
  });

  it("drops the waiting value on cancel", () => {
    const send = vi.fn();
    const debounce = new LatestDebounce<number>(send, 100);
    debounce.push(1);
    debounce.cancel();
    vi.advanceTimersByTime(500);
    expect(send).not.toHaveBeenCalled();
    expect(debounce.pending).toBe(false);
  });
});
