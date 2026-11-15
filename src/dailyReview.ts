// Daily review (ADR 0011): checks the answer of `daily_review`, computes the local day as a time
// window, formats times and renders the review as Markdown for the notes. Also records the
// working phases the backend logs. Pure, so day boundaries, daylight saving time and the
// Markdown are tested without the backend or React.

import { folderName } from "./folders";
import type { Translate } from "./i18n";
import { formatDuration } from "./statusLine";

/** Days the work log keeps (RETENTION_DAYS in work_log.rs), today included. */
export const REVIEW_DAYS = 90;
/** Entries the Markdown and the dialog list per section before "and N more". */
export const LIST_LIMIT = 20;

export interface ReviewSession {
  key: string;
  title: string;
  program: string;
  cwd: string;
  worktree: boolean;
  /** First and last moment within the day, Unix ms. */
  first: number;
  last: number;
  activeMs: number;
  runMs: number;
  running: boolean;
}

export interface ReviewCommit {
  hash: string;
  subject: string;
  /** Author time, Unix ms. */
  time: number;
  files: string[];
  fileCount: number;
}

export interface ReviewTree {
  path: string;
  worktree: boolean;
  files: string[];
  count: number;
}

export interface ReviewProject {
  path: string;
  isRepo: boolean;
  activeMs: number;
  runMs: number;
  sessions: ReviewSession[];
  commits: ReviewCommit[];
  files: string[];
  fileCount: number;
  uncommitted: ReviewTree[];
}

export interface DailyReview {
  start: number;
  end: number;
  activeMs: number;
  runMs: number;
  sessionCount: number;
  commitCount: number;
  projects: ReviewProject[];
  truncated: boolean;
}

// ---------------------------------------------------------------------------------------------
// Checking the answer

/** Reads the fields of a plain object; `null` for anything else. */
type Fields = (name: string) => unknown;

function record(value: unknown): Fields | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  return (name) => {
    const field: unknown = Reflect.get(value, name);
    return field;
  };
}

function num(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function str(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function bool(value: unknown): boolean | null {
  return typeof value === "boolean" ? value : null;
}

function strings(value: unknown): string[] | null {
  if (!Array.isArray(value)) return null;
  const items = value.filter((item): item is string => typeof item === "string");
  return items.length === value.length ? items : null;
}

function list<T>(value: unknown, parse: (item: unknown) => T | null): T[] | null {
  if (!Array.isArray(value)) return null;
  const items: T[] = [];
  for (const item of value) {
    const parsed = parse(item);
    if (parsed === null) return null;
    items.push(parsed);
  }
  return items;
}

function parseSession(value: unknown): ReviewSession | null {
  const v = record(value);
  if (!v) return null;
  const session = {
    key: str(v("key")),
    title: str(v("title")),
    program: str(v("program")),
    cwd: str(v("cwd")),
    worktree: bool(v("worktree")),
    first: num(v("first")),
    last: num(v("last")),
    activeMs: num(v("activeMs")),
    runMs: num(v("runMs")),
    running: bool(v("running")),
  };
  const { key, title, program, cwd, worktree, first, last, activeMs, runMs, running } = session;
  if (
    key === null ||
    title === null ||
    program === null ||
    cwd === null ||
    worktree === null ||
    first === null ||
    last === null ||
    activeMs === null ||
    runMs === null ||
    running === null
  ) {
    return null;
  }
  return { key, title, program, cwd, worktree, first, last, activeMs, runMs, running };
}

function parseCommit(value: unknown): ReviewCommit | null {
  const v = record(value);
  if (!v) return null;
  const hash = str(v("hash"));
  const subject = str(v("subject"));
  const time = num(v("time"));
  const fileCount = num(v("fileCount"));
  const files = strings(v("files"));
  if (hash === null || subject === null || time === null || fileCount === null || !files) {
    return null;
  }
  return { hash, subject, time, files, fileCount };
}

function parseTree(value: unknown): ReviewTree | null {
  const v = record(value);
  if (!v) return null;
  const path = str(v("path"));
  const worktree = bool(v("worktree"));
  const count = num(v("count"));
  const files = strings(v("files"));
  if (path === null || worktree === null || count === null || !files) return null;
  return { path, worktree, files, count };
}

function parseProject(value: unknown): ReviewProject | null {
  const v = record(value);
  if (!v) return null;
  const path = str(v("path"));
  const isRepo = bool(v("isRepo"));
  const activeMs = num(v("activeMs"));
  const runMs = num(v("runMs"));
  const fileCount = num(v("fileCount"));
  const files = strings(v("files"));
  const sessions = list(v("sessions"), parseSession);
  const commits = list(v("commits"), parseCommit);
  const uncommitted = list(v("uncommitted"), parseTree);
  if (
    path === null ||
    isRepo === null ||
    activeMs === null ||
    runMs === null ||
    fileCount === null ||
    !files ||
    !sessions ||
    !commits ||
    !uncommitted
  ) {
    return null;
  }
  return { path, isRepo, activeMs, runMs, sessions, commits, files, fileCount, uncommitted };
}

/** The answer of `daily_review`; `null` if malformed. */
export function parseDailyReview(value: unknown): DailyReview | null {
  const v = record(value);
  if (!v) return null;
  const start = num(v("start"));
  const end = num(v("end"));
  const activeMs = num(v("activeMs"));
  const runMs = num(v("runMs"));
  const sessionCount = num(v("sessionCount"));
  const commitCount = num(v("commitCount"));
  const projects = list(v("projects"), parseProject);
  const truncated = bool(v("truncated"));
  if (
    start === null ||
    end === null ||
    activeMs === null ||
    runMs === null ||
    sessionCount === null ||
    commitCount === null ||
    !projects ||
    truncated === null
  ) {
    return null;
  }
  return { start, end, activeMs, runMs, sessionCount, commitCount, projects, truncated };
}

// ---------------------------------------------------------------------------------------------
// Days

/** Local midnight of the day that contains `date`. */
export function startOfDay(date: Date): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

/** The day `delta` days after `day`, at local midnight (calendar days, not 24 h steps). */
export function shiftDay(day: Date, delta: number): Date {
  return new Date(day.getFullYear(), day.getMonth(), day.getDate() + delta);
}

/** The local day as `[start, end)` in Unix ms; 23 or 25 hours on a daylight saving change. */
export function dayWindow(day: Date): { start: number; end: number } {
  const start = startOfDay(day);
  return { start: start.getTime(), end: shiftDay(start, 1).getTime() };
}

/** First and last day the review can show: the kept days up to today. */
export function dayRange(now: Date): { first: Date; last: Date } {
  const last = startOfDay(now);
  return { first: shiftDay(last, -(REVIEW_DAYS - 1)), last };
}

/** `day` moved into the range of [`dayRange`]. */
export function clampDay(day: Date, now: Date): Date {
  const { first, last } = dayRange(now);
  const start = startOfDay(day);
  if (start.getTime() < first.getTime()) return first;
  if (start.getTime() > last.getTime()) return last;
  return start;
}

export function sameDay(a: Date, b: Date): boolean {
  return startOfDay(a).getTime() === startOfDay(b).getTime();
}

/** `YYYY-MM-DD` of the local day, the value of a date input. */
export function dayInputValue(day: Date): string {
  const pad = (value: number) => String(value).padStart(2, "0");
  return `${String(day.getFullYear()).padStart(4, "0")}-${pad(day.getMonth() + 1)}-${pad(day.getDate())}`;
}

/** The local day of a date input's value; `null` for an empty or impossible value. */
export function parseDayInput(value: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;
  const [year, month, day] = [Number(match[1]), Number(match[2]), Number(match[3])];
  const date = new Date(year, month - 1, day);
  const valid =
    date.getFullYear() === year && date.getMonth() === month - 1 && date.getDate() === day;
  return valid ? date : null;
}

// ---------------------------------------------------------------------------------------------
// Formatting

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;

/**
 * A span as hours and minutes ("2 h 45 min"), each part from the duration formatter of the
 * status line; below a minute the "under a minute" text.
 */
export function formatSpan(ms: number, language: string, t: Translate): string {
  if (!Number.isFinite(ms) || ms < MINUTE_MS) return t("review.underMinute");
  const hours = Math.floor(ms / HOUR_MS);
  const minutes = Math.floor((ms % HOUR_MS) / MINUTE_MS);
  // The status line's formatter turns 24 hours into a day; a day's review stays in hours.
  const hourText =
    hours >= 24
      ? new Intl.NumberFormat(language, {
          style: "unit",
          unit: "hour",
          unitDisplay: "short",
        }).format(hours)
      : formatDuration(hours * HOUR_MS, language);
  const parts = [
    hours > 0 ? hourText : null,
    minutes > 0 ? formatDuration(minutes * MINUTE_MS, language) : null,
  ].filter((part): part is string => part !== null);
  return parts.join(" ");
}

/** Clock time in the user's time zone ("09:12"). */
export function formatClock(ms: number, language: string): string {
  return new Intl.DateTimeFormat(language, { hour: "2-digit", minute: "2-digit" }).format(
    new Date(ms),
  );
}

/** The day in words ("Saturday, November 14, 2026"). */
export function formatDayLong(day: Date, language: string): string {
  return new Intl.DateTimeFormat(language, { dateStyle: "full" }).format(day);
}

/** The day in short form ("14.11.2026"), for the note's title. */
export function formatDayShort(day: Date, language: string): string {
  return new Intl.DateTimeFormat(language, {
    day: "2-digit",
    month: "2-digit",
    year: "numeric",
  }).format(day);
}

/** Program and title of a session, the title left out when it only repeats the program. */
export function sessionName(session: Pick<ReviewSession, "program" | "title">): string {
  const title = session.title.trim();
  if (title === "" || title === session.program) return session.program;
  if (session.program === "") return title;
  return `${session.program}: ${title}`;
}

/** The first `LIST_LIMIT` items and how many more there are (with a known total). */
export function limited<T>(
  items: readonly T[],
  total = items.length,
): { shown: T[]; more: number } {
  const shown = items.slice(0, LIST_LIMIT);
  return { shown, more: Math.max(0, total - shown.length) };
}

// ---------------------------------------------------------------------------------------------
// Markdown

/** Plain text that Markdown would otherwise read as markup (emphasis, links, HTML, headings). */
export function escapeMarkdown(text: string): string {
  return text.replace(/[\\`*_[\]<>#|~]/g, (char) => `\\${char}`).replace(/\s+/g, " ");
}

/** An inline code span that survives backticks in `text`. */
export function codeSpan(text: string): string {
  const runs = text.match(/`+/g) ?? [];
  const fence = "`".repeat(Math.max(0, ...runs.map((run) => run.length)) + 1);
  const pad = text.startsWith("`") || text.endsWith("`") ? " " : "";
  return `${fence}${pad}${text.replace(/[\r\n]+/g, " ")}${pad}${fence}`;
}

/** The review as Markdown for the notes: summary, then per project sessions, commits, files. */
export function reviewMarkdown(
  review: DailyReview,
  day: Date,
  language: string,
  t: Translate,
): string {
  const span = (ms: number) => formatSpan(ms, language, t);
  const clock = (ms: number) => formatClock(ms, language);
  const lines: string[] = [`# ${t("review.title")}: ${formatDayLong(day, language)}`, ""];
  lines.push(
    `- ${t("review.active")}: ${span(review.activeMs)}`,
    `- ${t("review.sessions")}: ${String(review.sessionCount)}`,
    `- ${t("review.projects")}: ${String(review.projects.length)}`,
    `- ${t("review.commits")}: ${String(review.commitCount)}`,
  );
  if (review.projects.length === 0) lines.push("", t("review.empty"));
  const more = (count: number) => {
    if (count > 0) lines.push(`- ${t("review.moreFiles", { count })}`);
  };
  for (const project of review.projects) {
    lines.push("", `## ${escapeMarkdown(folderName(project.path))}`, "", codeSpan(project.path));
    lines.push(
      "",
      `${t("review.active")}: ${span(project.activeMs)} · ${t("review.runTime")}: ${span(project.runMs)}`,
    );
    lines.push("", `### ${t("review.sessions")}`, "");
    for (const session of project.sessions) {
      const times = t("review.sessionTimes", {
        from: clock(session.first),
        to: clock(session.last),
      });
      const detail = t("review.sessionDetail", {
        active: span(session.activeMs),
        run: span(session.runMs),
      });
      const flags = [
        session.worktree ? t("review.worktree") : null,
        session.running ? t("review.running") : null,
      ].filter((flag): flag is string => flag !== null);
      const suffix = flags.length > 0 ? ` (${flags.join(", ")})` : "";
      lines.push(`- ${escapeMarkdown(sessionName(session))}, ${times}: ${detail}${suffix}`);
    }
    if (project.commits.length > 0) {
      lines.push("", `### ${t("review.commits")}`, "");
      for (const commit of project.commits) {
        lines.push(
          `- ${codeSpan(commit.hash.slice(0, 7))} ${escapeMarkdown(commit.subject)} (${clock(commit.time)})`,
        );
      }
    }
    if (project.fileCount > 0) {
      lines.push("", `### ${t("review.files")}`, "");
      const { shown, more: rest } = limited(project.files, project.fileCount);
      for (const file of shown) lines.push(`- ${codeSpan(file)}`);
      more(rest);
    }
    if (project.uncommitted.length > 0) {
      lines.push("", `### ${t("review.uncommitted")}`, "");
      for (const tree of project.uncommitted) {
        // A worktree's files are listed under it; the main work tree's directly.
        const indent = tree.worktree ? "  " : "";
        if (tree.worktree) lines.push(`- ${t("review.worktree")}: ${codeSpan(tree.path)}`);
        const { shown, more: rest } = limited(tree.files, tree.count);
        for (const file of shown) lines.push(`${indent}- ${codeSpan(file)}`);
        if (rest > 0) lines.push(`${indent}- ${t("review.moreFiles", { count: rest })}`);
      }
    }
  }
  if (review.truncated) lines.push("", `_${t("review.truncated")}_`);
  return `${lines.join("\n")}\n`;
}

// ---------------------------------------------------------------------------------------------
// Working phases

export interface WorkPhase {
  id: number;
  from: number;
  to: number;
}

/**
 * Turns the attention states of the sessions into working phases: one phase per stretch of
 * "working", reported when it ends, never per output chunk. Times in Unix ms.
 */
export class PhaseRecorder {
  private readonly since = new Map<number, number>();

  /**
   * The session's state changed; the phase that just ended, if any. `lastOutput`: the program's
   * last output; "working" turns into "waiting" only after a quiet time, which is no work.
   */
  change(
    id: number,
    working: boolean,
    now: number,
    lastOutput: number | null = null,
  ): WorkPhase | null {
    const from = this.since.get(id);
    if (working) {
      if (from === undefined) this.since.set(id, now);
      return null;
    }
    return this.end(id, now, lastOutput);
  }

  /** The session closed or exited: its open phase ends now, or at its last output. */
  end(id: number, now: number, lastOutput: number | null = null): WorkPhase | null {
    const from = this.since.get(id);
    if (from === undefined) return null;
    this.since.delete(id);
    const to = lastOutput !== null && lastOutput > from && lastOutput < now ? lastOutput : now;
    return to > from ? { id, from, to } : null;
  }

  /** Hangar quits: every open phase ends now. */
  flush(now: number): WorkPhase[] {
    const ids = [...this.since.keys()];
    return ids.map((id) => this.end(id, now)).filter((phase): phase is WorkPhase => phase !== null);
  }
}
