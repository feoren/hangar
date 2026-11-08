// Detects when a session needs attention: an agent has been working (output) and has been quiet
// for `quietMs`, has rung the bell (BEL), or its process has exited. It is only reported while
// the user is not looking. Pure logic with an injected clock, so it can be tested.
//
// Performance: `output` runs for every chunk and is therefore cheap. There is at most one timer per
// session; instead of resetting it on every chunk, it recomputes the remaining time when it fires.
// Only state changes are reported, never individual chunks.

export type AttentionState = "idle" | "working" | "attention";

/**
 * Why a session waits: it went quiet after working, the program rang the bell (BEL, often a
 * question), or its process exited. Only set while the state is "attention".
 */
export type AttentionReason = "quiet" | "bell" | "exit";

export interface AttentionConfig {
  enabled: boolean;
  quietMs: number;
}

export const ATTENTION_TIMING = {
  /** A single prompt redraw stays below this and triggers nothing. */
  minBurstBytes: 256,
  /** ConPTY redraws after a resize; that is not the agent's work. */
  resizeGraceMs: 600,
  /** Echo of the user's own input and redraws after a focus change (focus reports, DECSET 1004). */
  echoGraceMs: 400,
} as const;

export interface Clock {
  now(): number;
  setTimeout(callback: () => void, ms: number): number;
  clearTimeout(handle: number): void;
}

export const systemClock: Clock = {
  now: () => performance.now(),
  setTimeout: (callback, ms) => window.setTimeout(callback, ms),
  clearTimeout: (handle) => {
    window.clearTimeout(handle);
  },
};

export class AttentionTracker {
  private current: AttentionState = "idle";
  private why: AttentionReason | null = null;
  private burstBytes = 0;
  private lastOutput = 0;
  private ignoreUntil = Number.NEGATIVE_INFINITY;
  private timer: number | null = null;
  /** The user ended the process themselves: the exit that follows is no news to them. */
  private stopping = false;

  constructor(
    private readonly clock: Clock,
    private config: AttentionConfig,
    /** Active session in a focused window. Only queried at the moment of reporting. */
    private readonly isWatched: () => boolean,
    /** State or reason changed; the reason is `null` unless the state is "attention". */
    private readonly onChange: (state: AttentionState, reason: AttentionReason | null) => void,
  ) {}

  get state(): AttentionState {
    return this.current;
  }

  get reason(): AttentionReason | null {
    return this.why;
  }

  output(bytes: number): void {
    // An open notice stays until the user looks.
    if (!this.config.enabled || this.current === "attention" || this.stopping) return;
    const now = this.clock.now();
    if (now < this.ignoreUntil) return;
    this.burstBytes += bytes;
    this.lastOutput = now;
    if (this.burstBytes >= ATTENTION_TIMING.minBurstBytes) this.set("working");
    this.arm(this.config.quietMs);
  }

  /** PTY resize: the redraw that follows does not count. */
  resized(): void {
    this.ignoreFor(ATTENTION_TIMING.resizeGraceMs);
  }

  /** The terminal gained or lost focus: programs then often redraw. */
  focusChanged(): void {
    this.ignoreFor(ATTENTION_TIMING.echoGraceMs);
  }

  /** The user is typing in the session: notice gone, the echo does not count as work. */
  typed(): void {
    this.ignoreFor(ATTENTION_TIMING.echoGraceMs);
    this.seen();
  }

  /** The session was shown. */
  seen(): void {
    if (this.current === "attention") this.set("idle");
  }

  bell(): void {
    // An exit says more than a bell that follows it.
    if (this.why === "exit") return;
    this.alert("bell");
  }

  exited(): void {
    if (this.stopping) {
      this.stopping = false;
      this.set("idle");
      return;
    }
    this.alert("exit");
  }

  /**
   * The user ends the process themselves (stop button, menu): output during shutdown and the
   * process exit report nothing, and an open notice counts as seen.
   */
  stopRequested(): void {
    this.stopping = true;
    this.cancel();
    this.set("idle");
  }

  /** Ending failed and the process keeps running: notices work as usual again. */
  stopFailed(): void {
    this.stopping = false;
  }

  /** Restart: everything back to the start. */
  reset(): void {
    this.cancel();
    this.stopping = false;
    this.ignoreUntil = Number.NEGATIVE_INFINITY;
    this.set("idle");
  }

  setConfig(config: AttentionConfig): void {
    this.config = config;
    if (!config.enabled) {
      this.cancel();
      this.set("idle");
    }
  }

  dispose(): void {
    this.cancel();
  }

  private alert(reason: AttentionReason): void {
    if (!this.config.enabled) return;
    this.cancel();
    if (this.isWatched()) this.set("idle");
    else this.set("attention", reason);
  }

  private ignoreFor(ms: number): void {
    this.ignoreUntil = Math.max(this.ignoreUntil, this.clock.now() + ms);
  }

  private arm(ms: number): void {
    if (this.timer !== null) return;
    this.timer = this.clock.setTimeout(() => {
      this.quiet();
    }, ms);
  }

  private quiet(): void {
    this.timer = null;
    const remaining = this.lastOutput + this.config.quietMs - this.clock.now();
    if (remaining > 0) {
      this.arm(remaining);
      return;
    }
    this.burstBytes = 0;
    if (this.current !== "working") return;
    if (this.isWatched()) this.set("idle");
    else this.set("attention", "quiet");
  }

  private cancel(): void {
    if (this.timer !== null) this.clock.clearTimeout(this.timer);
    this.timer = null;
    this.burstBytes = 0;
  }

  private set(state: AttentionState, reason: AttentionReason | null = null): void {
    const why = state === "attention" ? reason : null;
    if (state === this.current && why === this.why) return;
    this.current = state;
    this.why = why;
    this.onChange(state, why);
  }
}
