// Small UI timing rules, taking the current time (ms) as an argument so they are testable without timers.

/** After closing a session: clicks on the row that moves up do not count for this long. */
export const CLOSE_CLICK_GUARD_MS = 400;
/** After "End process": the restart button in the same spot stays locked for this long. */
export const RESTART_LOCK_MS = 600;
/** Reload the profile list on returning to the window at most this often (PATH search in the backend). */
export const PROFILES_REFRESH_MIN_MS = 30_000;
/** Count background sessions on returning to the window at most this often (just reads a list). */
export const ORPHANS_REFRESH_MIN_MS = 5_000;
/** Save scrollback only once the session has had no output for this long. */
export const SCROLLBACK_QUIET_MS = 1500;

/**
 * Guard against accidental double clicks: `arm` on the first click, `blocked` for the following ones.
 * A single guard per list is enough, because only the row under the pointer can be hit.
 */
export class ClickGuard {
  private until = Number.NEGATIVE_INFINITY;

  arm(now: number, ms: number): void {
    this.until = Math.max(this.until, now + ms);
  }

  blocked(now: number): boolean {
    return now < this.until;
  }
}

/**
 * Click guard for the buttons in the session list: after closing, the next row moves under the
 * pointer, and the second click of a double click would otherwise hit its buttons. After "End
 * process", "Restart" appears in the same spot right away; it stays locked briefly.
 */
export class SessionActionGuard {
  private readonly afterClose = new ClickGuard();
  private readonly restartLocks = new Map<number, ClickGuard>();

  constructor(private readonly now: () => number = () => performance.now()) {}

  /** May a click on a button in the list take effect? */
  allows(): boolean {
    return !this.afterClose.blocked(this.now());
  }

  closed(): void {
    this.afterClose.arm(this.now(), CLOSE_CLICK_GUARD_MS);
  }

  killed(id: number): void {
    const lock = new ClickGuard();
    lock.arm(this.now(), RESTART_LOCK_MS);
    this.restartLocks.set(id, lock);
  }

  allowsRestart(id: number): boolean {
    if (!this.allows()) return false;
    if (this.restartLocks.get(id)?.blocked(this.now()) === true) return false;
    this.restartLocks.delete(id);
    return true;
  }
}

/** Has at least `interval` passed since `last` (ms, `null`: never)? */
export function isDue(last: number | null, now: number, interval: number): boolean {
  return last === null || now - last >= interval;
}
