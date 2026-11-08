import { describe, expect, it } from "vitest";

import type { Clock } from "./attention";
import {
  CHIME_TIMING,
  ChimeScheduler,
  chimeFor,
  chimeGain,
  MAX_GAIN,
  type AttentionChange,
  type ChimeContext,
  type ChimeKind,
} from "./chime";

class FakeClock implements Clock {
  time = 0;
  private next = 1;
  private timers = new Map<number, { at: number; callback: () => void }>();

  now(): number {
    return this.time;
  }

  setTimeout(callback: () => void, ms: number): number {
    const handle = this.next++;
    this.timers.set(handle, { at: this.time + ms, callback });
    return handle;
  }

  clearTimeout(handle: number): void {
    this.timers.delete(handle);
  }

  advance(ms: number): void {
    this.time += ms;
    for (const [handle, timer] of [...this.timers]) {
      if (timer.at > this.time) continue;
      this.timers.delete(handle);
      timer.callback();
    }
  }
}

const away: ChimeContext = { enabled: true, windowFocused: false, shown: true };
const running = { status: "running", exitCode: null } as const;
const failed = { status: "exited", exitCode: 2 } as const;
const fine = { status: "exited", exitCode: 0 } as const;

const enter = (reason: AttentionChange["reason"]): AttentionChange => ({
  previous: "working",
  previousReason: null,
  next: "attention",
  reason,
});

describe("chimeFor", () => {
  it("chimes when a session starts waiting in the background", () => {
    expect(chimeFor(enter("quiet"), running, away)).toBe("waiting");
    expect(chimeFor(enter("bell"), running, away)).toBe("waiting");
    expect(chimeFor(enter("exit"), fine, away)).toBe("waiting");
  });

  it("plays the low tone for a failed exit", () => {
    expect(chimeFor(enter("exit"), failed, away)).toBe("failed");
  });

  it("stays silent while the session is in sight", () => {
    expect(chimeFor(enter("quiet"), running, { ...away, windowFocused: true })).toBeNull();
  });

  it("chimes for a session hidden behind another one in a focused window", () => {
    expect(chimeFor(enter("quiet"), running, { ...away, windowFocused: true, shown: false })).toBe(
      "waiting",
    );
  });

  it("stays silent with the setting off", () => {
    expect(chimeFor(enter("quiet"), running, { ...away, enabled: false })).toBeNull();
  });

  it("only reacts to waiting", () => {
    const working: AttentionChange = {
      previous: "idle",
      previousReason: null,
      next: "working",
      reason: null,
    };
    expect(chimeFor(working, running, away)).toBeNull();
  });

  it("never chimes for a process the user ended", () => {
    expect(chimeFor(enter("exit"), { ...failed, stoppedByUser: true }, away)).toBeNull();
  });

  it("does not chime twice for one wait, but a failure that follows is news", () => {
    const upgrade: AttentionChange = {
      previous: "attention",
      previousReason: "quiet",
      next: "attention",
      reason: "bell",
    };
    expect(chimeFor(upgrade, running, away)).toBeNull();
    expect(chimeFor({ ...upgrade, reason: "exit" }, failed, away)).toBe("failed");
    expect(chimeFor({ ...upgrade, reason: "exit" }, fine, away)).toBeNull();
    expect(chimeFor({ ...upgrade, previousReason: "exit", reason: "exit" }, failed, away)).toBe(
      null,
    );
  });
});

describe("ChimeScheduler", () => {
  const setup = () => {
    const clock = new FakeClock();
    const played: ChimeKind[] = [];
    const volumes: number[] = [];
    const scheduler = new ChimeScheduler(clock, (kind, volume) => {
      played.push(kind);
      volumes.push(volume);
    });
    return { clock, played, volumes, scheduler };
  };

  it("plays once after a short gathering time", () => {
    const { clock, played, scheduler } = setup();
    scheduler.request("waiting", 40);
    expect(played).toEqual([]);
    clock.advance(CHIME_TIMING.gatherMs);
    expect(played).toEqual(["waiting"]);
  });

  it("merges sessions that start waiting together, the failure wins", () => {
    const { clock, played, scheduler } = setup();
    scheduler.request("waiting", 40);
    scheduler.request("failed", 40);
    scheduler.request("waiting", 40);
    clock.advance(CHIME_TIMING.gatherMs);
    expect(played).toEqual(["failed"]);
  });

  it("keeps a quiet gap after every sound", () => {
    const { clock, played, scheduler } = setup();
    scheduler.request("waiting", 40);
    clock.advance(CHIME_TIMING.gatherMs);
    scheduler.request("waiting", 40);
    clock.advance(CHIME_TIMING.gatherMs);
    expect(played).toEqual(["waiting"]);
    clock.advance(CHIME_TIMING.gapMs);
    scheduler.request("failed", 40);
    clock.advance(CHIME_TIMING.gatherMs);
    expect(played).toEqual(["waiting", "failed"]);
  });

  it("plays at the volume of the newest request", () => {
    const { clock, volumes, scheduler } = setup();
    scheduler.request("waiting", 40);
    scheduler.request("waiting", 75);
    clock.advance(CHIME_TIMING.gatherMs);
    expect(volumes).toEqual([75]);
  });

  it("plays nothing after dispose", () => {
    const { clock, played, scheduler } = setup();
    scheduler.request("waiting", 40);
    scheduler.dispose();
    clock.advance(CHIME_TIMING.gatherMs);
    expect(played).toEqual([]);
  });
});

describe("chimeGain", () => {
  it("maps the slider onto a quiet, even curve", () => {
    expect(chimeGain(0)).toBe(0);
    expect(chimeGain(100)).toBe(MAX_GAIN);
    expect(chimeGain(50)).toBeCloseTo(MAX_GAIN / 4);
    expect(chimeGain(250)).toBe(MAX_GAIN);
    expect(chimeGain(-5)).toBe(0);
  });
});
