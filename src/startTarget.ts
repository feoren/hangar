// Where a new session starts ("start target"). One rule everywhere: the active session's project
// folder, without sessions the last used folder, then the home folder. A folder chosen in the
// folder menu deviates for the next start only; the caller clears it after that start.

import { folderKey, samePath } from "./folders";
import type { Translate } from "./i18n";

export type TargetSource =
  /** Chosen in the folder menu for the next start. */
  | "chosen"
  /** The active session's project folder. */
  | "session"
  /** The folder of the last start (no session is active). */
  | "last"
  /** Nothing else known. */
  | "home";

export interface StartTarget {
  path: string | null;
  source: TargetSource;
}

/** What the rules need to know about a session. */
export interface ProjectSession {
  cwd: string;
  status: string;
  kind?: "agent" | "update" | undefined;
  worktree?: { repoRoot: string } | undefined;
}

/**
 * The project a session works on: a worktree session's repository (its own folder is a temporary
 * checkout), nothing for update sessions (they run in the home folder on purpose).
 */
export function projectFolder(session: ProjectSession): string | null {
  if (session.kind === "update") return null;
  const path = session.worktree?.repoRoot ?? session.cwd;
  return path.trim() === "" ? null : path;
}

export interface TargetInput {
  /** Folder chosen in the menu for the next start, `null` without a deviation. */
  chosen: string | null;
  /** The active session, `undefined` without one. */
  active: ProjectSession | undefined;
  /** Folder of the most recent start (`FolderPrefs.recent[0]`), `null` if none. */
  lastUsed: string | null;
  home: string | null;
}

/** The target without a deviation. */
export function defaultTarget(input: Omit<TargetInput, "chosen">): StartTarget {
  const fromSession = input.active ? projectFolder(input.active) : null;
  if (fromSession !== null) return { path: fromSession, source: "session" };
  if (input.lastUsed !== null && input.lastUsed !== "") {
    return { path: input.lastUsed, source: "last" };
  }
  return { path: input.home, source: "home" };
}

/** The chosen folder wins; choosing the default itself is no deviation. */
export function resolveStartTarget(input: TargetInput): StartTarget {
  const fallback = defaultTarget(input);
  const chosen = input.chosen;
  if (chosen === null || chosen === "") return fallback;
  if (fallback.path !== null && samePath(chosen, fallback.path)) return fallback;
  return { path: chosen, source: "chosen" };
}

/** Project folders of the sessions that still run, in sidebar order, each once. */
export function runningProjects(sessions: readonly ProjectSession[]): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const session of sessions) {
    if (session.status === "exited") continue;
    const path = projectFolder(session);
    if (path === null || seen.has(folderKey(path))) continue;
    seen.add(folderKey(path));
    result.push(path);
  }
  return result;
}

/** Folders offered for starting (quick access): target, running projects, favorites, recent. */
export function startPlaces(
  target: string | null,
  running: readonly string[],
  favorites: readonly string[],
  recent: readonly string[],
): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const path of [target, ...running, ...favorites, ...recent]) {
    if (path === null || path === "" || seen.has(folderKey(path))) continue;
    seen.add(folderKey(path));
    result.push(path);
  }
  return result;
}

/**
 * Whether a folder can get an own worktree, from the branch the status line reads
 * (`git_branches`): a name inside a repository, `null` outside, missing while the answer is pending.
 */
export type WorktreeChance = "yes" | "no" | "unknown";

export function worktreeChance(
  branches: ReadonlyMap<string, string | null>,
  path: string | null,
): WorktreeChance {
  if (path === null || path === "") return "no";
  if (!branches.has(path)) return "unknown";
  const branch = branches.get(path);
  return typeof branch === "string" && branch !== "" ? "yes" : "no";
}

/** Why the target is what it is, for its tooltip. */
export function targetSourceText(source: TargetSource, t: Translate): string {
  switch (source) {
    case "chosen":
      return t("start.sourceChosen");
    case "session":
      return t("start.sourceSession");
    case "last":
      return t("start.sourceLast");
    case "home":
      return t("start.sourceHome");
  }
}

/** Tooltip of every place that shows the target: full path, then where it comes from. */
export function targetTooltip(target: StartTarget, t: Translate): string {
  return [target.path ?? "", targetSourceText(target.source, t)].filter(Boolean).join("\n");
}
