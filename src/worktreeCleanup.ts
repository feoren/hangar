// Worktree cleanup: the answers of the backend's cleanup commands (worktree_cleanup.rs), what
// each entry offers, and the texts of the dialog. The backend checks every state again before it
// removes anything; the list here is only a view.

import type { Worktree } from "./api";
import { folderKey } from "./folders";
import { formatBytes } from "./format";
import type { Translate } from "./i18n";
import { keptMessage, WORKTREE_BRANCH_PREFIX } from "./worktree";

export type CleanupState =
  | { kind: "inUse" | "missing" | "changes" | "ignored" | "merged" | "clean" }
  | { kind: "unknown"; cause: UnknownCause; detail: string | null };

/**
 * Why an entry cannot be checked (Rust `Cause`). `detail` is git's own message for `gitFailed`
 * or the branch for `otherBranch`; only `gitFailed` shows it.
 */
export type UnknownCause =
  | "repoNotFound"
  | "notKnownToGit"
  | "otherBranch"
  | "noBranch"
  | "locked"
  | "notCheckedInTime"
  | "remoteRepo"
  | "gitFailed";

const CAUSES: readonly UnknownCause[] = [
  "repoNotFound",
  "notKnownToGit",
  "otherBranch",
  "noBranch",
  "locked",
  "notCheckedInTime",
  "remoteRepo",
  "gitFailed",
];

function isCause(value: unknown): value is UnknownCause {
  return CAUSES.some((cause) => cause === value);
}

export interface CleanupEntry {
  path: string;
  /** `null` when the repository could not be found. */
  repoRoot: string | null;
  branch: string;
  /** Unix ms of the newest of folder, `.git` file and git's index, HEAD and reflog. */
  modified: number | null;
  state: CleanupState;
}

export interface WorktreeSize {
  bytes: number;
  files: number;
  /** `false`: cut off, `bytes` is a lower bound. */
  complete: boolean;
  newest: number | null;
}

export interface CleanupOutcome {
  done: boolean;
  branchDeleted: boolean;
  state: CleanupState;
  reason: string | null;
}

/** What a row offers: remove, drop git's record of a missing folder, or show the folder. */
export type EntryAction = "remove" | "prune" | "reveal";

/** Rust `worktree_cleanup::MAX_FOLDERS`. */
export const MAX_CLEANUP_FOLDERS = 64;

function field(value: unknown, name: string): unknown {
  return typeof value === "object" && value !== null ? Reflect.get(value, name) : undefined;
}

export function parseState(value: unknown): CleanupState | null {
  const kind = field(value, "kind");
  if (kind === "unknown") {
    const cause = field(value, "cause");
    const detail = field(value, "detail");
    if (!isCause(cause)) return null;
    if (detail !== null && detail !== undefined && typeof detail !== "string") return null;
    return {
      kind,
      cause,
      detail: typeof detail === "string" && detail.trim() !== "" ? detail : null,
    };
  }
  if (
    kind === "inUse" ||
    kind === "missing" ||
    kind === "changes" ||
    kind === "ignored" ||
    kind === "merged" ||
    kind === "clean"
  ) {
    return { kind };
  }
  return null;
}

function timeOrNull(value: unknown): number | null | undefined {
  if (value === null || value === undefined) return null;
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : undefined;
}

function parseEntry(value: unknown): CleanupEntry | null {
  const path = field(value, "path");
  const repoRoot = field(value, "repoRoot");
  const branch = field(value, "branch");
  const modified = timeOrNull(field(value, "modified"));
  const state = parseState(field(value, "state"));
  if (typeof path !== "string" || path === "") return null;
  if (repoRoot !== null && typeof repoRoot !== "string") return null;
  if (typeof branch !== "string" || !branch.startsWith(WORKTREE_BRANCH_PREFIX)) return null;
  if (modified === undefined || state === null) return null;
  return { path, repoRoot, branch, modified, state };
}

/** Answer of `list_hangar_worktrees`; malformed entries are dropped, `null` if no list. */
export function parseCleanupEntries(value: unknown): CleanupEntry[] | null {
  if (!Array.isArray(value)) return null;
  return value.flatMap((item: unknown) => {
    const parsed = parseEntry(item);
    return parsed === null ? [] : [parsed];
  });
}

function count(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : null;
}

/** Answer of `hangar_worktree_size`; `null` if malformed. */
export function parseWorktreeSize(value: unknown): WorktreeSize | null {
  const bytes = count(field(value, "bytes"));
  const files = count(field(value, "files"));
  const complete = field(value, "complete");
  const newest = timeOrNull(field(value, "newest"));
  if (bytes === null || files === null || typeof complete !== "boolean" || newest === undefined) {
    return null;
  }
  return { bytes, files, complete, newest };
}

/** Answer of `remove_hangar_worktree` and `prune_hangar_worktree`; `null` if malformed. */
export function parseCleanupOutcome(value: unknown): CleanupOutcome | null {
  const done = field(value, "done");
  const branchDeleted = field(value, "branchDeleted");
  const state = parseState(field(value, "state"));
  const reason = field(value, "reason");
  if (typeof done !== "boolean" || typeof branchDeleted !== "boolean" || state === null) {
    return null;
  }
  if (reason !== null && reason !== undefined && typeof reason !== "string") return null;
  return { done, branchDeleted, state, reason: typeof reason === "string" ? reason : null };
}

/**
 * What the row offers. Only an entry with its repository can be removed or pruned; one with
 * content (or one that cannot be checked) is shown in the file manager instead, and a missing
 * folder without a repository offers nothing.
 */
export function entryAction(entry: CleanupEntry): EntryAction | null {
  const kind = entry.state.kind;
  if (entry.repoRoot !== null) {
    if (kind === "merged" || kind === "clean") return "remove";
    if (kind === "missing") return "prune";
  }
  return kind === "missing" ? null : "reveal";
}

/** Entries the bulk action takes: every one that can be removed or pruned. */
export function safeEntries(entries: readonly CleanupEntry[]): CleanupEntry[] {
  return entries.filter((entry) => {
    const action = entryAction(entry);
    return action === "remove" || action === "prune";
  });
}

interface SessionFolder {
  cwd: string;
  worktree?: Worktree | undefined;
}

/**
 * Folders whose repositories the backend also searches: the repositories of the sessions'
 * worktrees, the sessions' folders, favorites and recent folders. Each once, at most
 * MAX_CLEANUP_FOLDERS.
 */
export function cleanupFolders(input: {
  favorites: readonly string[];
  recent: readonly string[];
  sessions: readonly SessionFolder[];
}): string[] {
  const all = [
    ...input.sessions.flatMap((session) => (session.worktree ? [session.worktree.repoRoot] : [])),
    ...input.sessions.map((session) => session.cwd),
    ...input.favorites,
    ...input.recent,
  ];
  const seen = new Set<string>();
  const result: string[] = [];
  for (const path of all) {
    const key = folderKey(path);
    if (path.trim() === "" || seen.has(key)) continue;
    seen.add(key);
    result.push(path);
    if (result.length === MAX_CLEANUP_FOLDERS) break;
  }
  return result;
}

/** Short state name for the row's chip. */
export function stateLabel(state: CleanupState, t: Translate): string {
  switch (state.kind) {
    case "inUse":
      return t("cleanup.stateInUse");
    case "missing":
      return t("cleanup.stateMissing");
    case "changes":
      return t("cleanup.stateChanges");
    case "ignored":
      return t("cleanup.stateIgnored");
    case "merged":
      return t("cleanup.stateMerged");
    case "clean":
      return t("cleanup.stateClean");
    case "unknown":
      return t("cleanup.stateUnknown");
  }
}

/** One sentence under the row: what the state means for the user. */
export function stateHint(state: CleanupState, t: Translate): string {
  switch (state.kind) {
    case "inUse":
      return t("cleanup.hintInUse");
    case "missing":
      return t("cleanup.hintMissing");
    case "changes":
      return t("cleanup.hintChanges");
    case "ignored":
      return t("cleanup.hintIgnored");
    case "merged":
      return t("cleanup.hintMerged");
    case "clean":
      return t("cleanup.hintClean");
    case "unknown":
      return causeText(state.cause, state.detail, t);
  }
}

/** The cause in the window's own words; only git's message for `gitFailed` is passed through. */
function causeText(cause: UnknownCause, detail: string | null, t: Translate): string {
  switch (cause) {
    case "repoNotFound":
      return t("cleanup.causeRepoNotFound");
    case "notKnownToGit":
      return t("cleanup.causeNotKnownToGit");
    case "otherBranch":
      return t("cleanup.causeOtherBranch");
    case "noBranch":
      return t("cleanup.causeNoBranch");
    case "locked":
      return t("cleanup.causeLocked");
    case "notCheckedInTime":
      return t("cleanup.causeNotCheckedInTime");
    case "remoteRepo":
      return t("cleanup.causeRemoteRepo");
    case "gitFailed":
      return detail === null
        ? t("cleanup.causeGitFailedPlain")
        : t("cleanup.causeGitFailed", { detail });
  }
}

/** "2 KiB", or "≥ 2 KiB" when the backend stopped counting; empty while unknown. */
export function sizeText(size: WorktreeSize | undefined, t: Translate, language: string): string {
  if (size === undefined) return "";
  const amount = formatBytes(size.bytes, language);
  return size.complete ? amount : t("cleanup.sizeAtLeast", { size: amount });
}

/** The newest change known: the listing's, or a newer file the size walk saw. */
export function lastChange(entry: CleanupEntry, size: WorktreeSize | undefined): number | null {
  const times = [entry.modified, size?.newest ?? null].filter((time) => time !== null);
  return times.length === 0 ? null : Math.max(...times);
}

/** What to tell after a removal; `null` when everything went. */
export function outcomeNotice(outcome: CleanupOutcome, t: Translate): string | null {
  if (outcome.done) return outcome.branchDeleted ? null : t("cleanup.branchKept");
  // Why git kept it: the everyday cases in the window's words, anything else in git's.
  if (outcome.reason !== null && outcome.reason.trim() !== "") {
    return keptMessage(outcome.reason.trim(), t);
  }
  return t("cleanup.changedMeanwhile");
}

/** What one removal means for its row: it goes, or it stays with its fresh state and a sentence. */
export type CleanupResult =
  { done: true; notice: string | null } | { done: false; entry: CleanupEntry; notice: string };

/** Answer of a removal or prune for `entry` (unchecked), as the row shows it. */
export function cleanupResult(entry: CleanupEntry, value: unknown, t: Translate): CleanupResult {
  const outcome = parseCleanupOutcome(value);
  if (outcome === null) {
    return { done: false, entry, notice: t("cleanup.removeFailed", { error: t("error.unknown") }) };
  }
  const notice = outcomeNotice(outcome, t);
  if (outcome.done) return { done: true, notice };
  return { done: false, entry: { ...entry, state: outcome.state }, notice: notice ?? "" };
}

export interface RepositoryGroup {
  key: string;
  /** Folder name of the repository; `null` when it was not found. */
  name: string | null;
  repoRoot: string | null;
  entries: CleanupEntry[];
}

function lastSegment(path: string): string {
  const parts = path.split(/[\\/]+/).filter((part) => part !== "");
  return parts[parts.length - 1] ?? path;
}

/** Entries per repository, sorted by name; worktrees of a repository not found come last. */
export function groupByRepository(entries: readonly CleanupEntry[]): RepositoryGroup[] {
  const groups = new Map<string, RepositoryGroup>();
  for (const entry of entries) {
    const key = entry.repoRoot === null ? "" : folderKey(entry.repoRoot);
    let group = groups.get(key);
    if (group === undefined) {
      group = {
        key,
        name: entry.repoRoot === null ? null : lastSegment(entry.repoRoot),
        repoRoot: entry.repoRoot,
        entries: [],
      };
      groups.set(key, group);
    }
    group.entries.push(entry);
  }
  return [...groups.values()].sort((a, b) => {
    if (a.name === null) return 1;
    if (b.name === null) return -1;
    return a.name.localeCompare(b.name) || a.key.localeCompare(b.key);
  });
}

/** The worktree's folder name, the part that tells two rows apart. */
export function folderName(entry: CleanupEntry): string {
  return lastSegment(entry.path);
}
