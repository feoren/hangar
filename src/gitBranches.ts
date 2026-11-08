// Git branch per working folder for the status line. The backend reads `.git/HEAD` itself (no
// process); this module picks the folders to ask for and checks the answer.

/** The backend answers at most this many folders per call. */
export const MAX_BRANCH_PATHS = 64;
/** Refresh while the window is visible; a checkout in a terminal shows up within this time. */
export const BRANCH_REFRESH_MS = 15_000;

/** Working folders of the sessions, each once, in sidebar order, at most MAX_BRANCH_PATHS. */
export function branchPaths(sessions: readonly { cwd: string }[]): string[] {
  const paths: string[] = [];
  for (const { cwd } of sessions) {
    if (cwd === "" || paths.includes(cwd)) continue;
    paths.push(cwd);
    if (paths.length === MAX_BRANCH_PATHS) break;
  }
  return paths;
}

/** Answer of `git_branches`: folder to branch (or `null`). Anything malformed is left out. */
export function parseBranches(value: unknown): Map<string, string | null> {
  const branches = new Map<string, string | null>();
  if (typeof value !== "object" || value === null || Array.isArray(value)) return branches;
  for (const [path, branch] of Object.entries(value)) {
    if (branch === null) branches.set(path, null);
    else if (typeof branch === "string" && branch.trim() !== "") branches.set(path, branch);
  }
  return branches;
}

/** Same content: then the sidebar keeps the old map and does not re-render for nothing. */
export function sameBranches(
  a: ReadonlyMap<string, string | null>,
  b: ReadonlyMap<string, string | null>,
): boolean {
  if (a.size !== b.size) return false;
  for (const [path, branch] of a) {
    if (!b.has(path) || b.get(path) !== branch) return false;
  }
  return true;
}
