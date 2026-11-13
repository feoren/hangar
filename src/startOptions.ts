// What a new agent session asks the backend for besides its program's arguments: HANGAR.md as an
// extra instruction and, in a worktree Hangar has just created, a first prompt that checks the
// base branch upstream. The backend applies each only where the program takes it.

import type { Profile, StartOptions } from "./api";
import type { Settings } from "./settings";

/**
 * `freshWorktree`: the session starts in a worktree made for it right now. Restored and restarted
 * sessions pass `false`: they continue a conversation and must not be asked again.
 */
export function startOptions(
  settings: Pick<Settings, "hangarMd" | "worktreeUpstreamCheck">,
  freshWorktree: boolean,
): StartOptions {
  return {
    instructions: settings.hangarMd,
    startPrompt: freshWorktree && settings.worktreeUpstreamCheck ? "worktree" : null,
  };
}

/**
 * Names of the installed programs that take HANGAR.md but cannot get it as found on this machine
 * (an npm cmd.exe launcher such as `codex.cmd`), for the quiet line in Settings; empty when it
 * reaches every program that takes it.
 */
export function instructionsUnavailable(
  profiles: readonly Pick<Profile, "name" | "installed" | "instructions">[],
): string[] {
  return profiles
    .filter((profile) => profile.installed && profile.instructions === "launcher")
    .map((profile) => profile.name);
}
