// Collapses a burst of values into the last one. A window or splitter drag resizes a terminal on
// every frame; each PTY resize makes the program in it redraw, so only the size the burst ends on
// goes to the backend.

/** Pause after the last terminal resize before the PTY hears of it (ms). */
export const PTY_RESIZE_DEBOUNCE_MS = 100;

/**
 * Trailing debounce that keeps only the newest value: `push` restarts the wait, and once it passes
 * without a new value, `send` receives the last one. Nothing is ever dropped silently: `flush`
 * sends a waiting value at once, `cancel` is only for teardown.
 */
export class LatestDebounce<T> {
  private timer: ReturnType<typeof setTimeout> | undefined = undefined;
  private waiting: { value: T } | null = null;

  constructor(
    private readonly send: (value: T) => void,
    private readonly delayMs: number,
  ) {}

  /** Whether a value waits to be sent. */
  get pending(): boolean {
    return this.waiting !== null;
  }

  push(value: T): void {
    this.waiting = { value };
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.flush();
    }, this.delayMs);
  }

  /** Sends the waiting value now, if there is one. */
  flush(): void {
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = undefined;
    const waiting = this.waiting;
    this.waiting = null;
    if (waiting !== null) this.send(waiting.value);
  }

  /** Drops the waiting value (the receiver is gone). */
  cancel(): void {
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = undefined;
    this.waiting = null;
  }
}
