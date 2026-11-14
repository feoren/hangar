import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { TermSize } from "./api";
import { PTY_RESIZE_DEBOUNCE_MS } from "./latestDebounce";
import { PtySizeSync } from "./ptySize";

const size = (cols: number, rows: number): TermSize => ({ cols, rows });

describe("PtySizeSync", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  function setup() {
    const send = vi.fn<(id: number, size: TermSize) => void>();
    return { send, sync: new PtySizeSync(send) };
  }

  it("sends only the size a drag ends on", () => {
    const { send, sync } = setup();
    sync.started(size(80, 24));
    sync.attach(7);
    for (let cols = 81; cols <= 120; cols += 1) {
      sync.resized(size(cols, 30));
      vi.advanceTimersByTime(16);
    }
    expect(send).not.toHaveBeenCalled();
    vi.advanceTimersByTime(PTY_RESIZE_DEBOUNCE_MS);
    expect(send.mock.calls).toEqual([[7, size(120, 30)]]);
  });

  it("keeps a size that settled before the ID and sends it on attach", () => {
    const { send, sync } = setup();
    sync.started(size(80, 24));
    sync.resized(size(132, 40));
    vi.advanceTimersByTime(PTY_RESIZE_DEBOUNCE_MS);
    expect(send).not.toHaveBeenCalled();
    sync.attach(3);
    expect(send.mock.calls).toEqual([[3, size(132, 40)]]);
  });

  it("sends nothing the new process already started with", () => {
    const { send, sync } = setup();
    sync.started(size(100, 30));
    sync.resized(size(100, 30));
    vi.advanceTimersByTime(PTY_RESIZE_DEBOUNCE_MS);
    sync.attach(1);
    expect(send).not.toHaveBeenCalled();
  });

  it("sends a size again after a restart started with another one", () => {
    const { send, sync } = setup();
    sync.started(size(80, 24));
    sync.attach(1);
    sync.resized(size(120, 40));
    vi.advanceTimersByTime(PTY_RESIZE_DEBOUNCE_MS);
    // Restarted at the size of the moment; the window went back to the old size meanwhile.
    sync.started(size(80, 24));
    sync.resized(size(120, 40));
    vi.advanceTimersByTime(PTY_RESIZE_DEBOUNCE_MS);
    expect(send.mock.calls).toEqual([
      [1, size(120, 40)],
      [1, size(120, 40)],
    ]);
  });

  it("returns to a size it left mid-drag", () => {
    const { send, sync } = setup();
    sync.started(size(80, 24));
    sync.attach(2);
    sync.resized(size(90, 24));
    sync.resized(size(80, 24));
    vi.advanceTimersByTime(PTY_RESIZE_DEBOUNCE_MS);
    expect(send).not.toHaveBeenCalled();
    sync.resized(size(90, 24));
    vi.advanceTimersByTime(PTY_RESIZE_DEBOUNCE_MS);
    sync.resized(size(80, 24));
    vi.advanceTimersByTime(PTY_RESIZE_DEBOUNCE_MS);
    expect(send.mock.calls).toEqual([
      [2, size(90, 24)],
      [2, size(80, 24)],
    ]);
  });

  it("sends nothing once disposed", () => {
    const { send, sync } = setup();
    sync.attach(4);
    sync.resized(size(100, 30));
    sync.dispose();
    vi.advanceTimersByTime(PTY_RESIZE_DEBOUNCE_MS);
    sync.attach(5);
    expect(send).not.toHaveBeenCalled();
  });
});
