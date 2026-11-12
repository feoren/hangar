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

/** What a program needs from its profile to be a team session. */
export type TeamProfile = Pick<Profile, "teamCapable" | "cwd">;

/**
 * Why a program cannot be a team session, `null` when it can. `noHooks`: the backend says from
 * the catalog's capabilities that Hangar cannot pause its edits (plain terminals, programs without
 * a hook format). `fixedFolder`: the profile fixes a working folder, which would replace the
 * session's worktree; the backend refuses such a start as well.
 */
export function teamBlock(profile: TeamProfile): "noHooks" | "fixedFolder" | null {
  if (profile.teamCapable !== true) return "noHooks";
  if (profile.cwd !== null && profile.cwd !== "") return "fixedFolder";
  return null;
}

export function teamCapable(profile: TeamProfile): boolean {
  return teamBlock(profile) === null;
}

/**
 * Whether the program list shows `profile` under `mode`. Team offers only what really runs as a
 * team session: plain terminals and agents without a gate (wiki: team-capable agents) leave the
 * list instead of standing there greyed out. Private lists every program.
 */
export function listedFor(mode: StartMode, profile: TeamProfile): boolean {
  return mode === "private" || teamCapable(profile);
}

/**
 * How a start of `profile` goes under `mode`: as chosen, or `null` when the program cannot be a
 * team session (the Team list leaves it out).
 */
export function startModeFor(mode: StartMode, profile: TeamProfile): StartMode | null {
  if (mode === "private") return "private";
  return teamCapable(profile) ? "team" : null;
}

/**
 * The mode of a session the window restores or starts again: a team session always has its own
 * worktree, a private one never does. Worktree sessions from older workspaces count as team
 * sessions, unless their program cannot be one (`profile` missing or not capable): those come back
 * private, in their worktree, rather than shared without the gate.
 */
export function sessionMode(
  session: { worktree?: Worktree | null | undefined },
  profile: TeamProfile | undefined,
): StartMode {
  return session.worktree && profile !== undefined && teamCapable(profile) ? "team" : "private";
}

/** The backend runs the session as a team session (its row shows the team marker). */
export function isTeamSession(session: { mode?: SessionMode | undefined }): boolean {
  return session.mode === "team";
}
