// Own worktree per session: the label for the backend, when the option applies, checks of the
// backend's answers and the texts after closing. The backend (worktree.rs) creates the branch
// `hangar/<slug>` and the folder, and never removes anything by force.

import { errorText, type Worktree } from "./api";
import { folderKey } from "./folders";
import type { Translate } from "./i18n";
import { hasInvisible } from "./invisible";

/** Prefix of every branch Hangar creates; the backend refuses others. */
export const WORKTREE_BRANCH_PREFIX = "hangar/";

/**
 * Label for `create_worktree`: the program name, so the branch reads `hangar/claude-code-1a2b3c`.
 * The backend turns it into lowercase ASCII and adds a random suffix.
 */
export function worktreeLabel(programName: string): string {
  const label = programName.trim();
  return label === "" ? "session" : label;
}

/**
 * The option "Own worktree" applies only to a folder inside a git repository. The signal is the
 * branch the status line already reads (`git_branches`): a name or short hash inside a repository,
 * `null` outside, missing while the answer is pending.
 */
export function canUseWorktree(
  branches: ReadonlyMap<string, string | null>,
  cwd: string | null,
): boolean {
  if (cwd === null || cwd === "") return false;
  const branch = branches.get(cwd);
  return typeof branch === "string" && branch !== "";
}

function text(value: unknown, field: string): string | null {
  if (typeof value !== "object" || value === null) return null;
  const found: unknown = Reflect.get(value, field);
  return typeof found === "string" && found.trim() !== "" ? found : null;
}

/** Branch for the sidebar: `hangar/claude-code-1a2b3c` reads `claude-code-1a2b3c` next to the icon. */
export function shortBranch(branch: string): string {
  return branch.startsWith(WORKTREE_BRANCH_PREFIX) && branch.length > WORKTREE_BRANCH_PREFIX.length
    ? branch.slice(WORKTREE_BRANCH_PREFIX.length)
    : branch;
}

/** Answer of `create_worktree`, or a stored one from the workspace; `null` if malformed. */
export function parseWorktree(value: unknown): Worktree | null {
  const path = text(value, "path");
  const branch = text(value, "branch");
  const repoRoot = text(value, "repoRoot");
  if (path === null || branch === null || repoRoot === null) return null;
  if (!branch.startsWith(WORKTREE_BRANCH_PREFIX) || branch === WORKTREE_BRANCH_PREFIX) return null;
  // Hangar never creates such names; a record with bidi or control characters is not its own.
  if ([path, branch, repoRoot].some(hasInvisible)) return null;
  return { path, branch, repoRoot };
}

/** `child` is `parent` or lies inside it (case and separators as the platform compares them). */
export function insideFolder(child: string, parent: string): boolean {
  const c = folderKey(child);
  const p = folderKey(parent);
  if (c === p) return true;
  const separator = p.includes("\\") ? "\\" : "/";
  return c.startsWith(p.endsWith(separator) ? p : p + separator);
}

interface SessionPlace {
  id: number;
  cwd: string;
  status: string;
  worktree?: Worktree | undefined;
}

/**
 * Another running session still works in this worktree (started there by hand, or a second
 * session in the same folder): then it stays, or git would delete files under that session.
 */
export function worktreeInUse(
  worktree: Worktree,
  sessions: readonly SessionPlace[],
  closingId: number,
): boolean {
  return sessions.some(
    (session) =>
      session.id !== closingId &&
      session.status !== "exited" &&
      insideFolder(session.cwd, worktree.path),
  );
}

/**
 * Worktrees to clean up on quit when sessions are not remembered: nothing would bring those
 * sessions back, so their worktrees would be left behind. Each once, with every session that
 * works inside it (all of them end first, or git would delete files under a running program);
 * never while sessions are remembered.
 */
export function orphanedWorktrees(
  sessions: readonly SessionPlace[],
  restoreSessions: boolean,
): { ids: number[]; worktree: Worktree }[] {
  if (restoreSessions) return [];
  const seen = new Set<string>();
  const result: { ids: number[]; worktree: Worktree }[] = [];
  for (const session of sessions) {
    const worktree = session.worktree;
    if (!worktree) continue;
    const key = folderKey(worktree.path);
    if (seen.has(key)) continue;
    seen.add(key);
    const ids = sessions
      .filter(
        (other) =>
          (other.worktree !== undefined && folderKey(other.worktree.path) === key) ||
          insideFolder(other.cwd, worktree.path),
      )
      .map((other) => other.id);
    result.push({ ids, worktree });
  }
  return result;
}

export interface Removal {
  removed: boolean;
  branchDeleted: boolean;
  reason: string | null;
}

/** Answer of `remove_worktree`; `null` if malformed. */
export function parseRemoval(value: unknown): Removal | null {
  if (typeof value !== "object" || value === null) return null;
  const removed: unknown = Reflect.get(value, "removed");
  const branchDeleted: unknown = Reflect.get(value, "branchDeleted");
  const reason: unknown = Reflect.get(value, "reason");
  if (typeof removed !== "boolean" || typeof branchDeleted !== "boolean") return null;
  if (reason !== null && reason !== undefined && typeof reason !== "string") return null;
  return { removed, branchDeleted, reason: typeof reason === "string" ? reason : null };
}

/** Calm hint after closing: what stayed and where. `path` is shown copyable. */
export interface WorktreeNotice {
  message: string;
  /** Folder or branch the user may want to look at or clean up. */
  path: string;
}

/**
 * Why the worktree stays. The two everyday cases (an agent left changes, a build left ignored
 * output) get a translated sentence; anything else keeps the backend's or git's own words.
 */
function keptMessage(reason: string, t: Translate): string {
  if (reason === "") return t("worktree.kept");
  // git: "fatal: '<path>' contains modified or untracked files, use --force to delete it".
  if (reason.includes("contains modified or untracked files")) return t("worktree.keptChanges");
  // worktree.rs `ignored_reason`.
  if (reason.startsWith("The worktree contains ignored files")) return t("worktree.keptIgnored");
  return t("worktree.keptBecause", { reason });
}

/**
 * What to tell after `remove_worktree`: nothing when folder and branch are gone; the folder and
 * git's reason when it stays (changes, ignored files); the branch when only it stays (commits
 * that are not merged yet).
 */
export function removalNotice(
  removal: Removal,
  worktree: Worktree,
  t: Translate,
): WorktreeNotice | null {
  if (!removal.removed) {
    return { message: keptMessage(removal.reason?.trim() ?? "", t), path: worktree.path };
  }
  if (!removal.branchDeleted) {
    return { message: t("worktree.branchKept"), path: worktree.branch };
  }
  return null;
}

/** Notice when removing failed outright (git missing, refused path). */
export function removalErrorNotice(
  error: unknown,
  worktree: Worktree,
  t: Translate,
): WorktreeNotice {
  return { message: t("worktree.removeFailed", { error: errorText(error) }), path: worktree.path };
}

/**
 * Clear text for a failed `create_worktree`. The backend answers in English; the common cases get
 * a translated sentence, anything else keeps git's own words.
 */
export function createErrorText(error: unknown, t: Translate): string {
  const message = errorText(error);
  if (message.includes("git was not found")) return t("worktree.noGit");
  if (message.includes("not inside a git repository")) return t("worktree.notRepo");
  if (message.includes("network share or device")) return t("worktree.share");
  return t("worktree.createFailed", { error: message });
}
