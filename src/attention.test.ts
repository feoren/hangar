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

/** A submitted task and its first output, as Enter and the agent's answer produce them. */
function task(bytes = BIG): void {
  tracker.submitted();
  tracker.output(bytes);
}

describe("output and quiet", () => {
  it("is working on output and needs attention after a quiet period", () => {
    task();
    expect(tracker.state).toBe("working");
    clock.advance(QUIET - 1);
    expect(tracker.state).toBe("working");
    clock.advance(1);
    expect(tracker.state).toBe("attention");
    expect(changes).toEqual(["working", "attention"]);
  });

  it("further output postpones the notice", () => {
    task();
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
    task();
    clock.advance(QUIET);
    expect(tracker.state).toBe("idle");
    expect(changes).toEqual(["working", "idle"]);
  });

  it("asks whether the user is watching only when the timer fires", () => {
    watched = true;
    task();
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
    task();
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
    task();
    expect(tracker.state).toBe("working");
  });

  it("repeated resizes while dragging extend the grace period", () => {
    for (let i = 0; i < 10; i++) {
      tracker.resized();
      clock.advance(100);
      task();
    }
    expect(tracker.state).toBe("idle");
  });

  it("a redraw after a focus change does not count", () => {
    tracker.focusChanged();
    task();
    clock.advance(QUIET);
    expect(tracker.state).toBe("idle");
  });

  it("the echo of the user's own input does not count", () => {
    tracker.typed();
    clock.advance(50);
    task();
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
    task();
    tracker.exited();
    expect(clock.pending).toBe(0);
    expect(tracker.state).toBe("attention");
  });
});

describe("ended by the user", () => {
  it("a process ended by the user does not pulse", () => {
    task();
    tracker.stopRequested();
    task(); // output during shutdown
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
    task();
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
    task();
    clock.advance(QUIET);
    expect(tracker.state).toBe("attention");
    expect(changes).toEqual(["attention"]);
  });

  it("viewing while working keeps the working state", () => {
    task();
    tracker.seen();
    expect(tracker.state).toBe("working");
  });

  it("a restart resets everything", () => {
    task();
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
    task();
    tracker.bell();
    tracker.exited();
    clock.advance(QUIET);
    expect(tracker.state).toBe("idle");
    expect(clock.pending).toBe(0);
  });
});

describe("reason", () => {
  let reasons: (string | null)[];

  beforeEach(() => {
    reasons = [];
    tracker = new AttentionTracker(
      clock,
      { enabled: true, quietMs: QUIET },
      () => watched,
      (state, reason) => {
        changes.push(state);
        reasons.push(reason);
      },
    );
  });

  it("names a quiet period after work", () => {
    task();
    clock.advance(QUIET);
    expect(tracker.reason).toBe("quiet");
    expect(reasons).toEqual([null, "quiet"]);
  });

  it("names the bell and the exit", () => {
    tracker.bell();
    expect(tracker.reason).toBe("bell");
    tracker.seen();
    expect(tracker.reason).toBeNull();
    tracker.exited();
    expect(tracker.reason).toBe("exit");
  });

  it("a bell while waiting quietly upgrades the reason without a new state", () => {
    task();
    clock.advance(QUIET);
    tracker.bell();
    expect(changes).toEqual(["working", "attention", "attention"]);
    expect(reasons).toEqual([null, "quiet", "bell"]);
  });

  it("a bell after the exit keeps the exit", () => {
    tracker.exited();
    tracker.bell();
    expect(tracker.reason).toBe("exit");
    expect(reasons).toEqual(["exit"]);
  });

  it("an exit while waiting reports the exit", () => {
    tracker.bell();
    tracker.exited();
    expect(reasons).toEqual(["bell", "exit"]);
  });

  it("is null when nothing waits", () => {
    task();
    expect(tracker.reason).toBeNull();
    watched = true;
    tracker.bell();
    expect(tracker.state).toBe("idle");
    expect(tracker.reason).toBeNull();
  });
});

describe("why a working phase ended", () => {
  /** `endedBy` as the change listener sees it, read at the moment of each change. */
  const endings = (): (string | null)[] => {
    const seen: (string | null)[] = [];
    tracker = new AttentionTracker(
      clock,
      { enabled: true, quietMs: QUIET },
      () => watched,
      () => {
        seen.push(tracker.endedBy);
      },
    );
    return seen;
  };

  it("says quiet when the program went quiet, also while watched", () => {
    const seen = endings();
    watched = true;
    task();
    clock.advance(QUIET);
    expect(tracker.state).toBe("idle");
    expect(seen).toEqual([null, "quiet"]);
  });

  it("says bell for a bell, also while watched, where the state is only idle", () => {
    const seen = endings();
    watched = true;
    task();
    tracker.bell();
    expect(tracker.state).toBe("idle");
    expect(seen.at(-1)).toBe("bell");
  });

  it("says exit for an exit and nothing for a stop by the user", () => {
    const seen = endings();
    task();
    tracker.exited();
    expect(seen.at(-1)).toBe("exit");
    tracker.reset();
    task();
    tracker.stopRequested();
    expect(seen.at(-1)).toBeNull();
  });

  it("says nothing when the user interrupted the work (Ctrl+C, Escape)", () => {
    const seen = endings();
    task();
    tracker.interrupted();
    task();
    clock.advance(QUIET);
    expect(tracker.state).toBe("attention");
    expect(seen.at(-1)).toBeNull();
  });

  it("forgets an interruption once the next working phase starts", () => {
    const seen = endings();
    task();
    tracker.interrupted();
    clock.advance(QUIET);
    tracker.seen();
    task();
    clock.advance(QUIET);
    expect(seen.at(-1)).toBe("quiet");
  });

  it("ignores an interruption outside of work", () => {
    const seen = endings();
    tracker.interrupted();
    task();
    clock.advance(QUIET);
    expect(seen.at(-1)).toBe("quiet");
  });

  it("forgets the ending once new work starts", () => {
    const seen = endings();
    task();
    clock.advance(QUIET);
    tracker.seen();
    task();
    expect(tracker.state).toBe("working");
    expect(seen.at(-1)).toBeNull();
  });
});

describe("only a task ends as done", () => {
  it("a redraw without a submitted task rests instead of waiting", () => {
    // A status line or prompt redraw in a background tab: output, then quiet, but no task.
    tracker.output(BIG);
    expect(tracker.state).toBe("working");
    clock.advance(QUIET);
    expect(tracker.state).toBe("idle");
    expect(tracker.endedBy).toBeNull();
    expect(changes).toEqual(["working", "idle"]);
  });

  it("a submitted task ends as done after the quiet period", () => {
    tracker.submitted();
    tracker.output(BIG);
    clock.advance(QUIET);
    expect(tracker.state).toBe("attention");
    expect(tracker.reason).toBe("quiet");
  });

  it("a task counts for one working phase only", () => {
    tracker.submitted();
    tracker.output(BIG);
    clock.advance(QUIET);
    tracker.seen();
    tracker.output(BIG);
    clock.advance(QUIET);
    expect(tracker.state).toBe("idle");
  });

  it("long output without a task still ends as done (an agent working on its own)", () => {
    tracker.output(BIG);
    for (let i = 0; i < ATTENTION_TIMING.sustainedWorkMs / 1000; i++) {
      clock.advance(1000);
      tracker.output(10);
    }
    clock.advance(QUIET);
    expect(tracker.state).toBe("attention");
    expect(tracker.endedBy).toBe("quiet");
  });

  it("a bell and an exit still report without a task", () => {
    tracker.bell();
    expect(tracker.state).toBe("attention");
    tracker.seen();
    tracker.exited();
    expect(tracker.reason).toBe("exit");
  });

  it("a restart forgets the submitted task", () => {
    tracker.submitted();
    tracker.reset();
    tracker.output(BIG);
    clock.advance(QUIET);
    expect(tracker.state).toBe("idle");
  });
});
