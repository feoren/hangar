// When Hangar plays its chime: a soft two-tone sound when a session starts waiting while you do
// not see it, a lower one when a process fails. Pure decisions plus a small scheduler with an
// injected clock; the sound itself comes from chimeAudio.ts.
//
// Several sessions that start waiting at the same moment (a broadcast, one exit ending a chain)
// give one sound: requests within `gatherMs` are merged, the failure tone wins, and after a sound
// nothing plays for `gapMs`.

import type { AttentionReason, AttentionState, Clock } from "./attention";
import { exitedWithError } from "./sessionChip";
import type { SessionView } from "./sessions";

export type ChimeKind = "waiting" | "failed";

export const CHIME_TIMING = {
  gatherMs: 150,
  gapMs: 2000,
} as const;

export interface ChimeContext {
  /** "Play a sound" setting (only effective while attention is on, which the tracker ensures). */
  enabled: boolean;
  /** The Hangar window is visible and has focus. */
  windowFocused: boolean;
  /** The session is shown in one of the panes. */
  shown: boolean;
}

export interface AttentionChange {
  previous: AttentionState;
  previousReason: AttentionReason | null;
  next: AttentionState;
  reason: AttentionReason | null;
}

/**
 * The sound for this change, or `null`. A session that starts waiting chimes; one already waiting
 * chimes again only when its process then fails. A process the user ended never reaches
 * "attention" (the tracker reports it as idle), and `stoppedByUser` guards the rest.
 */
export function chimeFor(
  change: AttentionChange,
  session: Pick<SessionView, "status" | "exitCode" | "stoppedByUser">,
  context: ChimeContext,
): ChimeKind | null {
  if (!context.enabled || change.next !== "attention") return null;
  if (context.windowFocused && context.shown) return null;
  if (session.stoppedByUser === true) return null;
  const failed = change.reason === "exit" && exitedWithError(session);
  if (change.previous !== "attention") return failed ? "failed" : "waiting";
  return failed && change.previousReason !== "exit" ? "failed" : null;
}

const PRIORITY: Record<ChimeKind, number> = { waiting: 0, failed: 1 };

/** Merges requests that arrive together and keeps a quiet gap after every sound. */
export class ChimeScheduler {
  private pending: ChimeKind | null = null;
  /** Volume of the newest request; the setting may change between requests. */
  private volume = 0;
  private timer: number | null = null;
  private lastPlayed = Number.NEGATIVE_INFINITY;

  constructor(
    private readonly clock: Clock,
    private readonly play: (kind: ChimeKind, volume: number) => void,
  ) {}

  request(kind: ChimeKind, volume: number): void {
    this.volume = volume;
    if (this.pending === null || PRIORITY[kind] > PRIORITY[this.pending]) this.pending = kind;
    if (this.timer !== null) return;
    this.timer = this.clock.setTimeout(() => {
      this.flush();
    }, CHIME_TIMING.gatherMs);
  }

  dispose(): void {
    if (this.timer !== null) this.clock.clearTimeout(this.timer);
    this.timer = null;
    this.pending = null;
  }

  private flush(): void {
    this.timer = null;
    const kind = this.pending;
    this.pending = null;
    if (kind === null) return;
    const now = this.clock.now();
    if (now - this.lastPlayed < CHIME_TIMING.gapMs) return;
    this.lastPlayed = now;
    this.play(kind, this.volume);
  }
}

/** Loudest peak; a chime is a hint, never louder than a quiet system sound. */
export const MAX_GAIN = 0.32;

/** Volume setting (0 to 100) as a gain; squared, so the slider feels even to the ear. */
export function chimeGain(volume: number): number {
  const level = Math.min(100, Math.max(0, volume)) / 100;
  return level * level * MAX_GAIN;
}
