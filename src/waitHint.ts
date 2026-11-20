// "Still waiting for you": a session that has waited for the person longer than the setting says
// so in its status line ("waiting for 12 min") and its row's dot pulses softly (styles.css,
// "Wait hint"). Only the attention state counts; a session the person looks at goes back to idle
// when it is shown (attention.ts `seen`), so it never counts. A process that ended also waits in
// the attention state, but there is nothing to answer, so it does not count either.
//
// No sampling: the monitor learns each change of state (App.tsx feeds it from the rows' attention,
// the gate's waiting included, `waitingIds`) and sets one timer for the next session
// that will cross the threshold. It reports on changes only, so React renders once per crossing.

import type { AttentionReason, AttentionState, Clock } from "./attention";

/** Setting in minutes; 0 turns the hint off. */
export const WAIT_MINUTES = { min: 0, max: 60, default: 10 } as const;

export function clampWaitMinutes(value: number): number {
  if (!Number.isFinite(value)) return WAIT_MINUTES.default;
  return Math.min(WAIT_MINUTES.max, Math.max(WAIT_MINUTES.min, Math.round(value)));
}

/** The setting as a threshold in milliseconds; `null` when off. */
export function waitThresholdMs(minutes: number): number | null {
  return minutes <= 0 ? null : minutes * 60_000;
}

/** The session waits for the person: it asks, rang the bell, went quiet or waits at the gate. */
export function waitsForPerson(state: AttentionState, reason: AttentionReason | null): boolean {
  return state === "attention" && reason !== "exit";
}

/**
 * The sessions that wait for the person as their rows show it: the tracker's attention with the
 * gate's waiting on top (gate.ts `withGate`), which never passes through the tracker itself.
 * Sessions on screen (`shown`) are left out: the person sees them, also one waiting at the gate,
 * whose "decision" chip stays.
 */
export function waitingIds(
  ids: Iterable<number>,
  attention: ReadonlyMap<number, AttentionState>,
  reasons: ReadonlyMap<number, AttentionReason>,
  shown: ReadonlySet<number> = new Set(),
): Set<number> {
  const waiting = new Set<number>();
  for (const id of ids) {
    if (shown.has(id)) continue;
    if (waitsForPerson(attention.get(id) ?? "idle", reasons.get(id) ?? null)) waiting.add(id);
  }
  return waiting;
}

function sameWaits(a: ReadonlyMap<number, number>, b: ReadonlyMap<number, number>): boolean {
  if (a.size !== b.size) return false;
  for (const [id, since] of a) if (b.get(id) !== since) return false;
  return true;
}

/**
 * Keeps since when each session waits and reports those that wait `threshold` or longer (id to
 * the time the waiting began), on changes only. `setOnCross` names who hears of each session once when its
 * waiting crosses the threshold while the monitor runs, not when a changed setting lets it
 * across at once, so a new setting never flashes the taskbar.
 */
export class WaitMonitor {
  private readonly since = new Map<number, number>();
  private long: ReadonlyMap<number, number> = new Map();
  private thresholdMs: number | null = null;
  private timer: number | null = null;
  private onCross: (id: number) => void = () => undefined;

  constructor(
    private readonly clock: Clock,
    private readonly onChange: (long: ReadonlyMap<number, number>) => void,
  ) {}

  /** Who hears of a session crossing the threshold in time (the taskbar flash). */
  setOnCross(onCross: (id: number) => void): void {
    this.onCross = onCross;
  }

  /** `null` turns the hint off and clears it everywhere; the times of waiting are kept. */
  setThreshold(ms: number | null): void {
    if (ms === this.thresholdMs) return;
    this.thresholdMs = ms;
    this.update(false);
  }

  /** A session's state changed; a session that keeps waiting keeps its start. */
  changed(id: number, waiting: boolean): void {
    if (waiting === this.since.has(id)) return;
    if (waiting) this.since.set(id, this.clock.now());
    else this.since.delete(id);
    this.update(true);
  }

  /** The session is gone. */
  remove(id: number): void {
    this.changed(id, false);
  }

  dispose(): void {
    this.stop();
  }

  /** `crossing`: sessions that newly reach the threshold crossed it in time, not by a setting. */
  private update(crossing: boolean): void {
    this.stop();
    const threshold = this.thresholdMs;
    const now = this.clock.now();
    const next = new Map<number, number>();
    let due: number | null = null;
    if (threshold !== null) {
      for (const [id, since] of this.since) {
        const at = since + threshold;
        if (at <= now) next.set(id, since);
        else if (due === null || at < due) due = at;
      }
    }
    const before = this.long;
    if (!sameWaits(before, next)) {
      this.long = next;
      this.onChange(next);
      if (crossing) for (const id of next.keys()) if (!before.has(id)) this.onCross(id);
    }
    if (due !== null) {
      this.timer = this.clock.setTimeout(
        () => {
          this.timer = null;
          this.update(true);
        },
        Math.max(0, due - now),
      );
    }
  }

  private stop(): void {
    if (this.timer !== null) this.clock.clearTimeout(this.timer);
    this.timer = null;
  }
}
