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

/** Why a worktree stays although removing it was allowed (Rust `worktree::KeptCause`). */
export type KeptCause =
  "changes" | "ignored" | "partly" | "unsafeFilters" | "notCheckedInTime" | "gitFailed";

const KEPT_CAUSES: readonly KeptCause[] = [
  "changes",
  "ignored",
  "partly",
  "unsafeFilters",
  "notCheckedInTime",
  "gitFailed",
];

function isKeptCause(value: unknown): value is KeptCause {
  return KEPT_CAUSES.some((cause) => cause === value);
}

/**
 * Rust `worktree::Kept`. `cause` is `null` for a cause this window does not know (a newer
 * backend); `detail` is git's own message for `gitFailed`.
 */
export interface Kept {
  cause: KeptCause | null;
  detail: string | null;
}

/** A kept reason from the backend; `undefined` if malformed. */
export function parseKept(value: unknown): Kept | null | undefined {
  if (value === null || value === undefined) return null;
  if (typeof value !== "object") return undefined;
  const cause: unknown = Reflect.get(value, "cause");
  const detail: unknown = Reflect.get(value, "detail");
  if (typeof cause !== "string") return undefined;
  if (detail !== null && detail !== undefined && typeof detail !== "string") return undefined;
  return {
    cause: isKeptCause(cause) ? cause : null,
    detail: typeof detail === "string" && detail.trim() !== "" ? detail.trim() : null,
  };
}

export interface Removal {
  removed: boolean;
  branchDeleted: boolean;
  reason: Kept | null;
}

/** Answer of `remove_worktree`; `null` if malformed. */
export function parseRemoval(value: unknown): Removal | null {
  if (typeof value !== "object" || value === null) return null;
  const removed: unknown = Reflect.get(value, "removed");
  const branchDeleted: unknown = Reflect.get(value, "branchDeleted");
  const reason = parseKept(Reflect.get(value, "reason"));
  if (typeof removed !== "boolean" || typeof branchDeleted !== "boolean") return null;
  if (reason === undefined) return null;
  return { removed, branchDeleted, reason };
}

/** Calm hint after closing: what stayed and where. `path` is shown copyable. */
export interface WorktreeNotice {
  message: string;
  /** Folder or branch the user may want to look at or clean up. */
  path: string;
}

/**
 * Why the worktree stays, in the window's own words; only git's message for `gitFailed` is passed
 * through. No reason, or a cause this window does not know, gives the general sentence.
 */
export function keptMessage(reason: Kept | null, t: Translate): string {
  switch (reason?.cause ?? null) {
    case "changes":
      return t("worktree.keptChanges");
    case "ignored":
      return t("worktree.keptIgnored");
    case "partly":
      return t("worktree.keptPartly");
    case "unsafeFilters":
      return t("worktree.keptUnsafeFilters");
    case "notCheckedInTime":
      return t("worktree.keptNotCheckedInTime");
    case "gitFailed":
      return reason?.detail
        ? t("worktree.keptBecause", { reason: reason.detail })
        : t("worktree.kept");
    case null:
      return t("worktree.kept");
  }
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
    return { message: keptMessage(removal.reason, t), path: worktree.path };
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
  return knownWorktreeErrorText(message, t) ?? t("worktree.createFailed", { error: message });
}

/** The translated sentence for a common git or worktree error, `null` for any other. */
export function knownWorktreeErrorText(message: string, t: Translate): string | null {
  if (message.includes("git was not found")) return t("worktree.noGit");
  if (message.includes("not inside a git repository")) return t("worktree.notRepo");
  if (message.includes("network share or device")) return t("worktree.share");
  return null;
}

/**
 * Title of a session in its own worktree: the program and the branch's random suffix
 * (`Claude Code · 1a2b3c`), so two agents on one repository read differently at a glance. The
 * full branch stays in the status line.
 */
export function worktreeTitle(programName: string, branch: string): string {
  const short = shortBranch(branch);
  const suffix = /-([a-z0-9]{4,12})$/i.exec(short)?.[1] ?? short;
  return `${programName} · ${suffix}`;
}
