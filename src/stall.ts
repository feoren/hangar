// "Is the agent stuck?": a quiet hint on a working agent session that has shown nothing new for a
// while. A working agent keeps redrawing its spinner and elapsed time, so raw output never stops;
// what stops is progress. Progress is therefore read from the screen: its text without spinner
// glyphs and elapsed times (a fingerprint), sampled every STALL_SAMPLE_MS outside React, never per
// output chunk. With resource readings on, CPU use of the session's process tree also counts as
// being busy (a long build or test run prints nothing and is not stuck).
//
// No sound, no notification, no color: the row's status line only says how long nothing new came.

import type { Clock } from "./attention";

/** Setting in minutes; 0 turns the hint off. */
export const STALL_MINUTES = { min: 0, max: 30, default: 5 } as const;

/** CPU share (percent of the machine) from which the process tree counts as busy. */
export const STALL_CPU_PERCENT = 1;

/** How often the screens of working agent sessions are compared (on battery: power.ts). */
export const STALL_SAMPLE_MS = 10_000;

/** Wall clock: the row says "for 7 min", so the times are epoch milliseconds. */
export const wallClock: Clock = {
  now: () => Date.now(),
  setTimeout: (callback, ms) => window.setTimeout(callback, ms),
  clearTimeout: (handle) => {
    window.clearTimeout(handle);
  },
};

export function clampStallMinutes(value: number): number {
  if (!Number.isFinite(value)) return STALL_MINUTES.default;
  return Math.min(STALL_MINUTES.max, Math.max(STALL_MINUTES.min, Math.round(value)));
}

/** The setting as a threshold in milliseconds; `null` when off. */
export function stallThresholdMs(minutes: number): number | null {
  return minutes <= 0 ? null : minutes * 60_000;
}

/** "12s", "3m 05s", "1.5 s", "2 min", "00:42": elapsed times that tick while nothing happens. */
const ELAPSED =
  /\b\d+(?:[.,]\d+)?\s?(?:ms|s|sec|secs|m|min|mins|h|hr|hrs)\b|\b\d{1,2}:\d{2}(?::\d{2})?\b/giu;
/** Everything but letters and digits: spinner glyphs, braille dots, arrows, separators. */
const NOISE = /[^\p{L}\p{N}]+/gu;

/**
 * The screen's text with what changes without progress taken out: elapsed times, spinner glyphs
 * and spacing. Token counts and any new line stay, so they count as progress.
 */
export function progressFingerprint(lines: readonly string[]): string {
  return lines
    .map((line) => line.replace(ELAPSED, " ").replace(NOISE, " ").trim())
    .filter((line) => line !== "")
    .join("\n");
}

export interface StallInput {
  /** The session works right now (attention state "working"). */
  working: boolean;
  /** An agent session with attention detection; never a plain shell. */
  detected: boolean;
  /** `null`: the hint is off. */
  thresholdMs: number | null;
  now: number;
  /** Last time the screen changed (or the working phase began). */
  progressAt: number;
  /** Last CPU reading at or above STALL_CPU_PERCENT; `null` without readings. */
  busyAt: number | null;
}

/**
 * Since when the session shows nothing new, once that is `thresholdMs` or longer and its process
 * tree was not busy during that time either; `null` otherwise.
 */
export function stallSince(input: StallInput): number | null {
  const { working, detected, thresholdMs, now, progressAt, busyAt } = input;
  if (!working || !detected || thresholdMs === null) return null;
  if (now - progressAt < thresholdMs) return null;
  if (busyAt !== null && now - busyAt < thresholdMs) return null;
  return progressAt;
}

/** What the monitor needs of one session at a sample. */
export interface StallSource {
  id: number;
  working: boolean;
  detected: boolean;
  /** The visible screen lines; only read while the session works. */
  lines: () => readonly string[];
}

interface Watch {
  fingerprint: string;
  progressAt: number;
  busyAt: number | null;
}

function sameStalls(a: ReadonlyMap<number, number>, b: ReadonlyMap<number, number>): boolean {
  if (a.size !== b.size) return false;
  for (const [id, since] of a) if (b.get(id) !== since) return false;
  return true;
}

/**
 * Samples the working sessions every STALL_SAMPLE_MS (slower while saving energy,
 * `setSampleInterval`) while the hint is on and reports the stalled ones (id to the time since
 * which nothing new came) on changes only, so React renders a few times per stall, not per
 * sample. Its own timer runs on the injected clock instead of power.ts `PollTimer`, so the tests
 * drive it with fake time and the first sample waits one interval instead of running at once.
 */
export class StallMonitor {
  private readonly watches = new Map<number, Watch>();
  private stalled: ReadonlyMap<number, number> = new Map();
  private thresholdMs: number | null = null;
  private timer: number | null = null;
  private sampleMs = STALL_SAMPLE_MS;

  private sources: () => readonly StallSource[] = () => [];

  constructor(
    private readonly clock: Clock,
    private readonly onChange: (stalled: ReadonlyMap<number, number>) => void,
  ) {}

  /** Where the sessions come from; read at every sample. */
  setSources(sources: () => readonly StallSource[]): void {
    this.sources = sources;
  }

  /** `null` stops sampling and clears every hint. */
  setThreshold(ms: number | null): void {
    this.thresholdMs = ms;
    if (ms === null) {
      this.stop();
      this.watches.clear();
      this.publish(new Map());
      return;
    }
    if (this.timer === null) this.schedule();
  }

  /**
   * The time between two samples. A running timer starts over with the new interval; the times
   * since the last progress stay, so a hint neither comes early nor starts its waiting anew.
   */
  setSampleInterval(ms: number): void {
    if (ms === this.sampleMs) return;
    this.sampleMs = ms;
    if (this.timer === null) return;
    this.stop();
    this.schedule();
  }

  /** A CPU reading of a session's process tree; `null` while unknown. */
  cpu(id: number, percent: number | null): void {
    const watch = this.watches.get(id);
    if (watch === undefined || percent === null || percent < STALL_CPU_PERCENT) return;
    watch.busyAt = this.clock.now();
  }

  /**
   * The session made progress outside the screen (state change, Ctrl+C sent): its hint goes at
   * once and the waiting starts anew.
   */
  progressed(id: number): void {
    const watch = this.watches.get(id);
    if (watch !== undefined) watch.progressAt = this.clock.now();
    if (this.stalled.has(id)) {
      const next = new Map(this.stalled);
      next.delete(id);
      this.publish(next);
    }
  }

  /** Compares every working session's screen with the last sample. */
  sample(): void {
    const now = this.clock.now();
    const next = new Map<number, number>();
    const seen = new Set<number>();
    for (const source of this.sources()) {
      if (!source.working || !source.detected || this.thresholdMs === null) continue;
      seen.add(source.id);
      const fingerprint = progressFingerprint(source.lines());
      const watch = this.watches.get(source.id);
      if (watch === undefined) {
        this.watches.set(source.id, { fingerprint, progressAt: now, busyAt: null });
        continue;
      }
      if (watch.fingerprint !== fingerprint) {
        watch.fingerprint = fingerprint;
        watch.progressAt = now;
      }
      const since = stallSince({
        working: true,
        detected: true,
        thresholdMs: this.thresholdMs,
        now,
        progressAt: watch.progressAt,
        busyAt: watch.busyAt,
      });
      if (since !== null) next.set(source.id, since);
    }
    // A session that stopped working starts its next phase with a fresh watch.
    for (const id of this.watches.keys()) if (!seen.has(id)) this.watches.delete(id);
    this.publish(next);
  }

  dispose(): void {
    this.stop();
  }

  private schedule(): void {
    this.timer = this.clock.setTimeout(() => {
      this.timer = null;
      this.sample();
      if (this.thresholdMs !== null) this.schedule();
    }, this.sampleMs);
  }

  private stop(): void {
    if (this.timer !== null) this.clock.clearTimeout(this.timer);
    this.timer = null;
  }

  private publish(next: ReadonlyMap<number, number>): void {
    if (sameStalls(this.stalled, next)) return;
    this.stalled = next;
    this.onChange(next);
  }
}
