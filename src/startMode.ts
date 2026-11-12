// Private or team session (ADR 0008). The switch under "Starts in" picks the mode of the next
// start; it is remembered per device. A team session always runs in a git worktree of its own,
// gets the agent gate and is visible to the team in a shared project; a private one starts in the
// folder itself, without the gate, and the team never sees it.

import type { Profile, SessionMode, Worktree } from "./api";
import type { WorktreeChance } from "./startTarget";

export type StartMode = SessionMode;

export const DEFAULT_START_MODE: StartMode = "private";

const STORAGE_KEY = "hangar.startMode";

/** A stored value; anything unknown is the default, so a broken entry never blocks a start. */
export function parseStartMode(value: unknown): StartMode {
  return value === "team" ? "team" : DEFAULT_START_MODE;
}

export function loadStartMode(): StartMode {
  try {
    return parseStartMode(localStorage.getItem(STORAGE_KEY));
  } catch {
    return DEFAULT_START_MODE;
  }
}

export function saveStartMode(mode: StartMode): void {
  try {
    localStorage.setItem(STORAGE_KEY, mode);
  } catch {
    // Without storage the switch simply starts private next time.
  }
}

/**
 * A team session needs a worktree, so a folder outside a git repository falls back to private.
 * While the answer is pending the chosen mode stays, so the switch does not flicker; a start in
 * that moment gets git's own answer.
 */
export function teamPossible(chance: WorktreeChance): boolean {
  return chance !== "no";
}

/** The mode the switch shows and the next start uses. */
export function effectiveMode(chosen: StartMode, chance: WorktreeChance): StartMode {
  return chosen === "team" && teamPossible(chance) ? "team" : "private";
}

/**
 * Whether a program can run as a team session: the backend says so from the catalog's
 * capabilities (`teamCapable`). Plain terminals and programs without a gate cannot yet.
 */
export function teamCapable(profile: Pick<Profile, "teamCapable">): boolean {
  return profile.teamCapable === true;
}

/**
 * How a start of `profile` goes under `mode`: as chosen, or `null` when the program cannot be a
 * team session (the list shows it disabled).
 */
export function startModeFor(
  mode: StartMode,
  profile: Pick<Profile, "teamCapable">,
): StartMode | null {
  if (mode === "private") return "private";
  return teamCapable(profile) ? "team" : null;
}

/**
 * The mode of a session the window restores or starts again: a team session always has its own
 * worktree, a private one never does. So worktree sessions from older workspaces count as team
 * sessions.
 */
export function sessionMode(session: { worktree?: Worktree | null | undefined }): StartMode {
  return session.worktree ? "team" : "private";
}
