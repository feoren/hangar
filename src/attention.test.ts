import { beforeEach, describe, expect, it } from "vitest";

import { ATTENTION_TIMING, AttentionTracker, type AttentionState, type Clock } from "./attention";

/** Clock with timers that only advances via `advance`. */
class FakeClock implements Clock {
  time = 0;
  private nextHandle = 1;
  private timers = new Map<number, { at: number; callback: () => void }>();

  now(): number {
    return this.time;
  }

  setTimeout(callback: () => void, ms: number): number {
    const handle = this.nextHandle++;
    this.timers.set(handle, { at: this.time + ms, callback });
    return handle;
  }

  clearTimeout(handle: number): void {
    this.timers.delete(handle);
  }

  get pending(): number {
    return this.timers.size;
  }

  advance(ms: number): void {
    const end = this.time + ms;
    for (;;) {
      let next: [number, { at: number; callback: () => void }] | undefined;
      for (const entry of this.timers) {
        if (entry[1].at <= end && (!next || entry[1].at < next[1].at)) next = entry;
      }
      if (!next) break;
      this.timers.delete(next[0]);
      this.time = next[1].at;
      next[1].callback();
    }
    this.time = end;
  }
}

const QUIET = 3000;
const BIG = ATTENTION_TIMING.minBurstBytes;

let clock: FakeClock;
let watched: boolean;
let changes: AttentionState[];
let tracker: AttentionTracker;

beforeEach(() => {
  clock = new FakeClock();
  watched = false;
  changes = [];
  tracker = new AttentionTracker(
    clock,
    { enabled: true, quietMs: QUIET },
    () => watched,
    (state) => changes.push(state),
  );
});

describe("output and quiet", () => {
  it("is working on output and needs attention after a quiet period", () => {
    tracker.output(BIG);
    expect(tracker.state).toBe("working");
    clock.advance(QUIET - 1);
    expect(tracker.state).toBe("working");
    clock.advance(1);
    expect(tracker.state).toBe("attention");
    expect(changes).toEqual(["working", "attention"]);
  });

  it("further output postpones the notice", () => {
    tracker.output(BIG);
    clock.advance(2000);
    tracker.output(10);
    clock.advance(2000);
    expect(tracker.state).toBe("working");
    clock.advance(1000);
    expect(tracker.state).toBe("attention");
  });

  it("keeps at most one timer, even for many chunks", () => {
    for (let i = 0; i < 1000; i++) {
      tracker.output(100);
      clock.advance(5);
    }
    expect(clock.pending).toBe(1);
    expect(changes).toEqual(["working"]);
  });

  it("reports nothing while the user is watching", () => {
    watched = true;
    tracker.output(BIG);
    clock.advance(QUIET);
    expect(tracker.state).toBe("idle");
    expect(changes).toEqual(["working", "idle"]);
  });

  it("asks whether the user is watching only when the timer fires", () => {
    watched = true;
    tracker.output(BIG);
    watched = false; // user switches away while the agent works
    clock.advance(QUIET);
    expect(tracker.state).toBe("attention");
  });

  it("a small redraw triggers nothing", () => {
    tracker.output(BIG - 1);
    clock.advance(QUIET);
    expect(tracker.state).toBe("idle");
    expect(changes).toEqual([]);
  });

  it("small amounts start counting again after a quiet period", () => {
    tracker.output(BIG / 2);
    clock.advance(QUIET);
    tracker.output(BIG / 2);
    clock.advance(QUIET);
    expect(tracker.state).toBe("idle");
  });

  it("the quiet time is configurable", () => {
    tracker.setConfig({ enabled: true, quietMs: 10_000 });
    tracker.output(BIG);
    clock.advance(QUIET);
    expect(tracker.state).toBe("working");
    clock.advance(7000);
    expect(tracker.state).toBe("attention");
  });
});

describe("false alarms", () => {
  it("output shortly after a resize does not count", () => {
    tracker.resized();
    clock.advance(ATTENTION_TIMING.resizeGraceMs - 1);
    tracker.output(BIG * 10);
    clock.advance(QUIET * 2);
    expect(tracker.state).toBe("idle");
    expect(changes).toEqual([]);
  });

  it("output counts again after the grace period", () => {
    tracker.resized();
    clock.advance(ATTENTION_TIMING.resizeGraceMs);
    tracker.output(BIG);
    expect(tracker.state).toBe("working");
  });

  it("repeated resizes while dragging extend the grace period", () => {
    for (let i = 0; i < 10; i++) {
      tracker.resized();
      clock.advance(100);
      tracker.output(BIG);
    }
    expect(tracker.state).toBe("idle");
  });

  it("a redraw after a focus change does not count", () => {
    tracker.focusChanged();
    tracker.output(BIG);
    clock.advance(QUIET);
    expect(tracker.state).toBe("idle");
  });

  it("the echo of the user's own input does not count", () => {
    tracker.typed();
    clock.advance(50);
    tracker.output(BIG);
    clock.advance(QUIET);
    expect(tracker.state).toBe("idle");
  });
});

describe("immediate notices", () => {
  it("bell and process exit report immediately", () => {
    tracker.bell();
    expect(tracker.state).toBe("attention");
    tracker.seen();
    tracker.exited();
    expect(tracker.state).toBe("attention");
  });

  it("not while the user is watching", () => {
    watched = true;
    tracker.bell();
    tracker.exited();
    expect(tracker.state).toBe("idle");
    expect(changes).toEqual([]);
  });

  it("process exit cancels a running quiet timer", () => {
    tracker.output(BIG);
    tracker.exited();
    expect(clock.pending).toBe(0);
    expect(tracker.state).toBe("attention");
  });
});

describe("ended by the user", () => {
  it("a process ended by the user does not pulse", () => {
    tracker.output(BIG);
    tracker.stopRequested();
    tracker.output(BIG); // output during shutdown
    clock.advance(QUIET);
    tracker.exited();
    expect(tracker.state).toBe("idle");
    expect(clock.pending).toBe(0);
    expect(changes).toEqual(["working", "idle"]);
  });

  it("an open notice counts as seen when the user ends the process", () => {
    tracker.bell();
    tracker.stopRequested();
    expect(tracker.state).toBe("idle");
  });

  it("only the next exit counts as ended by the user", () => {
    tracker.stopRequested();
    tracker.exited();
    expect(tracker.state).toBe("idle");
    tracker.exited();
    expect(tracker.state).toBe("attention");
  });

  it("a restart cancels the stop request", () => {
    tracker.stopRequested();
    tracker.reset();
    tracker.exited();
    expect(tracker.state).toBe("attention");
  });

  it("if stopping fails, output and exit report again", () => {
    tracker.stopRequested();
    tracker.stopFailed();
    tracker.output(BIG);
    clock.advance(QUIET);
    expect(tracker.state).toBe("attention");
  });
});

describe("reset", () => {
  it("activating and typing clear the notice", () => {
    tracker.bell();
    tracker.seen();
    expect(tracker.state).toBe("idle");
    tracker.bell();
    tracker.typed();
    expect(tracker.state).toBe("idle");
  });

  it("an open notice stays despite further output", () => {
    tracker.bell();
    tracker.output(BIG);
    clock.advance(QUIET);
    expect(tracker.state).toBe("attention");
    expect(changes).toEqual(["attention"]);
  });

  it("viewing while working keeps the working state", () => {
    tracker.output(BIG);
    tracker.seen();
    expect(tracker.state).toBe("working");
  });

  it("a restart resets everything", () => {
    tracker.output(BIG);
    tracker.reset();
    expect(tracker.state).toBe("idle");
    expect(clock.pending).toBe(0);
  });
});

describe("disabled", () => {
  it("reports nothing and clears existing notices", () => {
    tracker.bell();
    tracker.setConfig({ enabled: false, quietMs: QUIET });
    expect(tracker.state).toBe("idle");
    tracker.output(BIG);
    tracker.bell();
    tracker.exited();
    clock.advance(QUIET);
    expect(tracker.state).toBe("idle");
    expect(clock.pending).toBe(0);
  });
});
