// Commit history in the changes panel: checks the answers of `git_log` and `git_commit_diff`,
// merges pages of the list and groups it into "not pushed" and the rest. Pure, so it can be tested
// without the backend or React.

import { parseGitDiff, type GitDiff } from "./gitDiff";

/** What the changes panel shows: the uncommitted changes or the commit history. */
export type ChangesView = "open" | "commits";
export const CHANGES_VIEWS: readonly ChangesView[] = ["open", "commits"];

/** A commit the history should open and scroll to; a new `token` asks again for the same one. */
export interface CommitFocus {
  sha: string;
  token: number;
}

/** Commits per page of the list (and per "Load more"). */
export const COMMIT_PAGE = 30;
/** Largest page the backend serves (`limit` is clamped to 1..=100). */
export const COMMIT_PAGE_MAX = 100;
/** A message body longer than this many lines starts collapsed. */
export const BODY_PREVIEW_LINES = 6;

export interface CommitSummary {
  /** Full id, 40 (SHA-1) or 64 (SHA-256) hex characters. */
  sha: string;
  shortSha: string;
  subject: string;
  author: string;
  /** Author date in Unix seconds. */
  authoredAt: number;
  /** Reachable from the upstream; always `false` without one. */
  pushed: boolean;
}

export interface GitLog {
  /** `false`: the folder lies outside a work tree. */
  isRepo: boolean;
  /** Newest first. */
  commits: CommitSummary[];
  /** For example "origin/main"; `null` without upstream or on a detached HEAD. */
  upstream: string | null;
  /** Commits on HEAD the upstream lacks, over the whole history (not only this page). */
  aheadCount: number;
  /** Another page follows. */
  hasMore: boolean;
}

export interface CommitHeader {
  sha: string;
  shortSha: string;
  /** Empty for a root commit, two or more for a merge. */
  parents: string[];
  subject: string;
  body: string;
  author: string;
  authoredAt: number;
}

export interface GitCommitDiff extends GitDiff {
  commit: CommitHeader;
}

const FULL_SHA = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;

/** A full commit id as `git_log` gives it; only these are passed back to `git_commit_diff`. */
export function isFullSha(value: unknown): value is string {
  return typeof value === "string" && FULL_SHA.test(value);
}

function count(value: unknown): number {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 ? value : 0;
}

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function commitSummary(value: unknown): CommitSummary | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const sha: unknown = Reflect.get(value, "sha");
  if (!isFullSha(sha)) return null;
  const shortSha = text(Reflect.get(value, "shortSha"));
  return {
    sha,
    shortSha: shortSha === "" ? sha.slice(0, 7) : shortSha,
    subject: text(Reflect.get(value, "subject")),
    author: text(Reflect.get(value, "author")),
    authoredAt: count(Reflect.get(value, "authoredAt")),
    pushed: Reflect.get(value, "pushed") === true,
  };
}

/** Answer of `git_log`; `null` if malformed. Malformed commits are left out. */
export function parseGitLog(value: unknown): GitLog | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const isRepo: unknown = Reflect.get(value, "isRepo");
  const commits: unknown = Reflect.get(value, "commits");
  const upstream: unknown = Reflect.get(value, "upstream");
  if (typeof isRepo !== "boolean" || !Array.isArray(commits)) return null;
  return {
    isRepo,
    commits: commits
      .map(commitSummary)
      .filter((commit): commit is CommitSummary => commit !== null),
    upstream: typeof upstream === "string" && upstream !== "" ? upstream : null,
    aheadCount: count(Reflect.get(value, "aheadCount")),
    hasMore: Reflect.get(value, "hasMore") === true,
  };
}

/** Answer of `git_commit_diff`; `null` if malformed. The diff part is read like `git_diff`. */
export function parseCommitDiff(value: unknown): GitCommitDiff | null {
  const diff = parseGitDiff(value);
  if (diff === null || typeof value !== "object" || value === null) return null;
  const commit: unknown = Reflect.get(value, "commit");
  if (typeof commit !== "object" || commit === null || Array.isArray(commit)) return null;
  const sha: unknown = Reflect.get(commit, "sha");
  const parents: unknown = Reflect.get(commit, "parents");
  if (!isFullSha(sha) || !Array.isArray(parents)) return null;
  return {
    ...diff,
    commit: {
      sha,
      shortSha: text(Reflect.get(commit, "shortSha")) || sha.slice(0, 7),
      parents: parents.filter(isFullSha),
      subject: text(Reflect.get(commit, "subject")),
      body: text(Reflect.get(commit, "body")),
      author: text(Reflect.get(commit, "author")),
      authoredAt: count(Reflect.get(commit, "authoredAt")),
    },
  };
}

/**
 * The list after the next page arrived. Commits already listed are not added twice (a new commit
 * between two pages shifts the history by one). An empty page while the backend still says "more"
 * ends the paging, so "Load more" can never loop.
 */
export function appendPage(current: GitLog, page: GitLog): GitLog {
  const known = new Set(current.commits.map((commit) => commit.sha));
  const added = page.commits.filter((commit) => !known.has(commit.sha));
  return {
    isRepo: page.isRepo,
    commits: [...current.commits, ...added],
    upstream: page.upstream,
    aheadCount: page.aheadCount,
    hasMore: page.hasMore && added.length > 0,
  };
}

/** Page size when the list is read again: as many as were shown, within the backend's bounds. */
export function reloadLimit(shown: number): number {
  return Math.min(COMMIT_PAGE_MAX, Math.max(COMMIT_PAGE, shown));
}

/**
 * The first `shown` commits again (at least one page), in as many backend pages as needed, so a
 * reload keeps every commit the user loaded with "Load more" instead of cutting the list back to
 * the backend's largest page. `null` if an answer is malformed; a rejected read rejects.
 */
export async function readLog(
  fetchPage: (skip: number, limit: number) => Promise<unknown>,
  shown: number,
): Promise<GitLog | null> {
  let log = parseGitLog(await fetchPage(0, reloadLimit(shown)));
  // Ends: every round either adds commits or `appendPage` clears `hasMore`.
  while (log !== null && log.hasMore && log.commits.length < shown) {
    const page = parseGitLog(
      await fetchPage(log.commits.length, reloadLimit(shown - log.commits.length)),
    );
    if (page === null) return null;
    log = appendPage(log, page);
  }
  return log;
}

export interface CommitGroup {
  /** `unpushed`: not on the upstream yet; `pushed`: on it; `all`: no upstream to compare with. */
  kind: "unpushed" | "pushed" | "all";
  commits: CommitSummary[];
}

/**
 * The list in groups: with an upstream, the commits it lacks first, then the rest (history order
 * is kept inside each group). Without an upstream there is nothing to compare, so one group.
 */
export function commitGroups(log: GitLog): CommitGroup[] {
  if (log.upstream === null) {
    return log.commits.length > 0 ? [{ kind: "all", commits: log.commits }] : [];
  }
  const unpushed = log.commits.filter((commit) => !commit.pushed);
  const pushed = log.commits.filter((commit) => commit.pushed);
  const groups: CommitGroup[] = [];
  if (unpushed.length > 0) groups.push({ kind: "unpushed", commits: unpushed });
  if (pushed.length > 0) groups.push({ kind: "pushed", commits: pushed });
  return groups;
}

const MINUTE = 60;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;
const WEEK = 7 * DAY;
const MONTH = 30 * DAY;
const YEAR = 365 * DAY;

/**
 * How long ago a commit was made: "5 min ago", "yesterday", "3 weeks ago" (in the UI language via
 * Intl, so no unit needs translating). A date in the future (clock skew) counts as now.
 */
export function commitAge(authoredAt: number, nowMs: number, language: string): string {
  const seconds = Math.max(0, Math.floor(nowMs / 1000) - authoredAt);
  const format = new Intl.RelativeTimeFormat(language, { numeric: "auto", style: "short" });
  if (seconds < MINUTE) return format.format(0, "second");
  if (seconds < HOUR) return format.format(-Math.floor(seconds / MINUTE), "minute");
  if (seconds < DAY) return format.format(-Math.floor(seconds / HOUR), "hour");
  if (seconds < WEEK) return format.format(-Math.floor(seconds / DAY), "day");
  if (seconds < MONTH) return format.format(-Math.floor(seconds / WEEK), "week");
  if (seconds < YEAR) return format.format(-Math.floor(seconds / MONTH), "month");
  return format.format(-Math.floor(seconds / YEAR), "year");
}

/** Full date and time for tooltips. */
export function commitDate(authoredAt: number, language: string): string {
  return new Intl.DateTimeFormat(language, { dateStyle: "medium", timeStyle: "short" }).format(
    new Date(authoredAt * 1000),
  );
}

/** The message body without trailing blank lines; long ones are cut for the collapsed view. */
export function bodyPreview(body: string): { text: string; long: boolean } {
  const lines = body.replace(/\s+$/, "").split(/\r?\n/);
  if (lines.length <= BODY_PREVIEW_LINES) return { text: lines.join("\n"), long: false };
  return { text: lines.slice(0, BODY_PREVIEW_LINES).join("\n"), long: true };
}
