import { afterEach, describe, expect, it, vi } from "vitest";

import { followResizes, MotionGate, WINDOW_RESIZE_SETTLE_MS } from "./sidebarMotion";

describe("MotionGate", () => {
  it("fits right away while nothing moves", () => {
    const gate = new MotionGate();
    const fit = vi.fn();
    gate.whenSettled(fit);
    expect(fit).toHaveBeenCalledTimes(1);
  });

  it("holds fits during the motion and runs each once at its end", () => {
    const gate = new MotionGate();
    const a = vi.fn();
    const b = vi.fn();
    gate.setMoving(true);
    gate.whenSettled(a);
    gate.whenSettled(a);
    gate.whenSettled(b);
    expect(a).not.toHaveBeenCalled();
    gate.setMoving(false);
    expect(a).toHaveBeenCalledTimes(1);
    expect(b).toHaveBeenCalledTimes(1);
    gate.setMoving(false);
    expect(a).toHaveBeenCalledTimes(1);
  });

  it("drops a fit whose pane went away", () => {
    const gate = new MotionGate();
    const fit = vi.fn();
    gate.setMoving(true);
    gate.whenSettled(fit);
    gate.forget(fit);
    gate.setMoving(false);
    expect(fit).not.toHaveBeenCalled();
  });
});

describe("followResizes", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("holds hidden fits while the window resizes and runs each once when it rests", () => {
    vi.useFakeTimers();
    const target = new EventTarget();
    const gate = new MotionGate();
    const stop = followResizes(target, gate, WINDOW_RESIZE_SETTLE_MS);
    const fit = vi.fn();
    for (let frame = 0; frame < 20; frame += 1) {
      target.dispatchEvent(new Event("resize"));
      gate.whenSettled(fit);
      vi.advanceTimersByTime(16);
    }
    expect(gate.isMoving).toBe(true);
    expect(fit).not.toHaveBeenCalled();
    vi.advanceTimersByTime(WINDOW_RESIZE_SETTLE_MS - 16 - 1);
    expect(fit).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(gate.isMoving).toBe(false);
    expect(fit).toHaveBeenCalledTimes(1);
    stop();
  });

  it("opens the gate and stops listening when stopped mid-resize", () => {
    vi.useFakeTimers();
    const target = new EventTarget();
    const gate = new MotionGate();
    const stop = followResizes(target, gate, WINDOW_RESIZE_SETTLE_MS);
    const fit = vi.fn();
    target.dispatchEvent(new Event("resize"));
    gate.whenSettled(fit);
    stop();
    expect(gate.isMoving).toBe(false);
    expect(fit).toHaveBeenCalledTimes(1);
    target.dispatchEvent(new Event("resize"));
    expect(gate.isMoving).toBe(false);
  });
});
