// Small UI timing rules, taking the current time (ms) as an argument so they are testable without timers.

/** Reload the profile list on returning to the window at most this often (PATH search in the backend). */
export const PROFILES_REFRESH_MIN_MS = 30_000;
/** Count background sessions on returning to the window at most this often (just reads a list). */
export const ORPHANS_REFRESH_MIN_MS = 5_000;
/** Save scrollback only once the session has had no output for this long. */
export const SCROLLBACK_QUIET_MS = 1500;

/** Has at least `interval` passed since `last` (ms, `null`: never)? */
export function isDue(last: number | null, now: number, interval: number): boolean {
  return last === null || now - last >= interval;
}
