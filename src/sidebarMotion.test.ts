import { describe, expect, it, vi } from "vitest";

import { MotionGate } from "./sidebarMotion";

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
