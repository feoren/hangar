// Activity calendar in the empty window: the user's own commits of the last year as a grid of
// days, weeks as columns, like the contribution calendar on GitHub. Pure, so day boundaries,
// daylight saving changes and intensity levels are tested without rendering.

import { folderKey } from "./folders";

/** Folders sent to `git_activity`; the backend refuses more (MAX_FOLDERS in git_activity.rs). */
export const MAX_ACTIVITY_FOLDERS = 24;
/** Columns of a full calendar: 52 weeks plus the partial weeks at both ends. */
export const CALENDAR_WEEKS = 53;
/** Days per column. */
const DAYS = 7;
/** Month labels need room; of two labels closer than this many columns, the earlier is left out. */
const MIN_LABEL_GAP = 3;

export type ActivityLevel = 0 | 1 | 2 | 3 | 4;

export interface ActivityDay {
  /** Local midnight of the day. */
  date: Date;
  count: number;
  level: ActivityLevel;
}

export interface ActivityCalendar {
  /** Columns from oldest to newest; `null` marks a day before the start or after today. */
  weeks: (ActivityDay | null)[][];
  /** Commits from the first day up to and including today. */
  total: number;
}

export interface MonthLabel {
  /** Column of the first week that starts in this month. */
  week: number;
  /** First day of that column, for formatting the month's name. */
  date: Date;
}

/** Answer of `git_activity`: author times in seconds since the epoch; `null` if malformed. */
export function parseGitActivity(value: unknown): number[] | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const commits: unknown = Reflect.get(value, "commits");
  if (!Array.isArray(commits)) return null;
  return commits.filter(
    (time): time is number => typeof time === "number" && Number.isFinite(time),
  );
}

/** The folders whose repositories are read: each folder once, at most MAX_ACTIVITY_FOLDERS. */
export function activityFolders(...lists: readonly (readonly string[])[]): string[] {
  const keys = new Set<string>();
  const folders: string[] = [];
  for (const path of lists.flat()) {
    const key = folderKey(path);
    if (path.trim() === "" || keys.has(key)) continue;
    keys.add(key);
    folders.push(path);
    if (folders.length === MAX_ACTIVITY_FOLDERS) break;
  }
  return folders;
}

/**
 * First day of the week for a BCP 47 tag as `Date.getDay()` counts (0 Sunday, 1 Monday), from
 * the locale's week data where the WebView knows it, else Monday.
 */
export function firstDayOfWeek(tag: string | undefined): number {
  if (tag === undefined) return 1;
  try {
    const locale = new Intl.Locale(tag);
    // Chromium has `getWeekInfo()`, older engines the `weekInfo` property; neither is typed yet.
    const getter: unknown = Reflect.get(locale, "getWeekInfo");
    const info: unknown =
      typeof getter === "function"
        ? Reflect.apply(getter, locale, [])
        : Reflect.get(locale, "weekInfo");
    if (typeof info !== "object" || info === null) return 1;
    const first: unknown = Reflect.get(info, "firstDay");
    // ISO numbering: 1 Monday ... 7 Sunday.
    return typeof first === "number" && Number.isInteger(first) && first >= 1 && first <= 7
      ? first % 7
      : 1;
  } catch {
    return 1;
  }
}

function dayKey(date: Date): number {
  return date.getFullYear() * 10_000 + (date.getMonth() + 1) * 100 + date.getDate();
}

/** Local midnight `days` days after `date`'s day; calendar arithmetic, so DST changes do not shift it. */
function addDays(date: Date, days: number): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days);
}

/** Commits per local day. */
export function countByDay(times: readonly number[]): Map<number, number> {
  const counts = new Map<number, number>();
  for (const seconds of times) {
    const key = dayKey(new Date(seconds * 1000));
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return counts;
}

/**
 * Upper bounds of levels 1 to 3, from the quartiles of the days with commits (as on GitHub): a
 * day up to the first bound is level 1, above the third level 4.
 */
export function levelBounds(counts: readonly number[]): [number, number, number] {
  const sorted = counts.filter((count) => count > 0).sort((a, b) => a - b);
  if (sorted.length === 0) return [0, 0, 0];
  const at = (share: number): number =>
    sorted[Math.min(sorted.length - 1, Math.floor(share * (sorted.length - 1)))] ?? 0;
  return [at(0.25), at(0.5), at(0.75)];
}

export function levelOf(count: number, bounds: readonly [number, number, number]): ActivityLevel {
  if (count <= 0) return 0;
  if (count <= bounds[0]) return 1;
  if (count <= bounds[1]) return 2;
  if (count <= bounds[2]) return 3;
  return 4;
}

/**
 * The calendar ending today: the year from the day after today's date one year ago up to today,
 * in `weeks` columns that start on `firstDay`. Commits outside that year are not counted.
 */
export function buildCalendar(
  times: readonly number[],
  now: Date,
  firstDay: number,
  weeks: number = CALENDAR_WEEKS,
): ActivityCalendar {
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const start = new Date(today.getFullYear() - 1, today.getMonth(), today.getDate() + 1);
  const lastColumnStart = addDays(today, -((today.getDay() - firstDay + DAYS) % DAYS));
  const firstColumnStart = addDays(lastColumnStart, -DAYS * (weeks - 1));
  const counts = countByDay(times);
  const startKey = dayKey(start);
  const todayKey = dayKey(today);

  const days: (Date | null)[][] = [];
  const inRange: number[] = [];
  let total = 0;
  for (let week = 0; week < weeks; week += 1) {
    const column: (Date | null)[] = [];
    for (let day = 0; day < DAYS; day += 1) {
      const date = addDays(firstColumnStart, week * DAYS + day);
      const key = dayKey(date);
      if (key < startKey || key > todayKey) {
        column.push(null);
        continue;
      }
      column.push(date);
      const count = counts.get(key) ?? 0;
      inRange.push(count);
      total += count;
    }
    days.push(column);
  }
  const bounds = levelBounds(inRange);
  return {
    weeks: days.map((column) =>
      column.map((date) => {
        if (date === null) return null;
        const count = counts.get(dayKey(date)) ?? 0;
        return { date, count, level: levelOf(count, bounds) };
      }),
    ),
    total,
  };
}

/** The most recent `count` weeks; a narrow window shows fewer weeks instead of scrolling. */
export function recentWeeks(calendar: ActivityCalendar, count: number): ActivityCalendar["weeks"] {
  return calendar.weeks.slice(Math.max(0, calendar.weeks.length - Math.max(1, count)));
}

/** Weeks that fit into `width` pixels with cells of `cell` and gaps of `gap` pixels. */
export function weeksThatFit(width: number, cell: number, gap: number): number {
  if (!(width > 0)) return CALENDAR_WEEKS;
  return Math.max(1, Math.min(CALENDAR_WEEKS, Math.floor((width + gap) / (cell + gap))));
}

/**
 * Month labels above the columns: at each column whose first shown day starts a new month.
 * Labels that would crowd the previous one or the left edge are left out.
 */
export function monthLabels(weeks: readonly (readonly (ActivityDay | null)[])[]): MonthLabel[] {
  const labels: MonthLabel[] = [];
  let previousMonth: number | null = null;
  weeks.forEach((column, week) => {
    const first = column.find((day): day is ActivityDay => day !== null);
    if (first === undefined) return;
    const month = first.date.getFullYear() * 12 + first.date.getMonth();
    if (month === previousMonth) return;
    previousMonth = month;
    // Too close: the earlier label, usually a partial month at the left edge, gives way.
    const last = labels.at(-1);
    if (last !== undefined && week - last.week < MIN_LABEL_GAP) labels.pop();
    labels.push({ week, date: first.date });
  });
  return labels;
}
