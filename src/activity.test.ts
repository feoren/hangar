import { afterAll, beforeAll, describe, expect, it } from "vitest";
import {
  activityFolders,
  buildCalendar,
  CALENDAR_WEEKS,
  countByDay,
  firstDayOfWeek,
  levelBounds,
  levelOf,
  MAX_ACTIVITY_FOLDERS,
  monthLabels,
  parseGitActivity,
  recentWeeks,
  weeksThatFit,
  type ActivityDay,
} from "./activity";

// A zone with daylight saving time, so day boundaries around the change are tested for real.
const previousZone = process.env.TZ;
beforeAll(() => {
  process.env.TZ = "Europe/Berlin";
});
afterAll(() => {
  process.env.TZ = previousZone;
});

/** Seconds since the epoch of a local time. */
function at(year: number, month: number, day: number, hour = 12, minute = 0): number {
  return new Date(year, month - 1, day, hour, minute).getTime() / 1000;
}

function days(weeks: readonly (readonly (ActivityDay | null)[])[]): ActivityDay[] {
  return weeks.flat().filter((day): day is ActivityDay => day !== null);
}

// Thursday, 12 Nov 2026, afternoon.
const NOW = new Date(2026, 10, 12, 15, 30);

describe("parseGitActivity", () => {
  it("keeps the numeric times and refuses malformed answers", () => {
    expect(
      parseGitActivity({ commits: [1, 2.5, "3", null, Number.NaN], truncated: false }),
    ).toEqual([1, 2.5]);
    expect(parseGitActivity({ commits: [] })).toEqual([]);
    expect(parseGitActivity(null)).toBeNull();
    expect(parseGitActivity([1, 2])).toBeNull();
    expect(parseGitActivity({ commits: "1" })).toBeNull();
  });
});

describe("activityFolders", () => {
  it("lists each folder once, in order, up to the limit", () => {
    expect(
      activityFolders(["C:\\Work\\hangar", "D:\\x"], ["c:/work/hangar/", " ", "D:\\y"], ["D:\\x"]),
    ).toEqual(["C:\\Work\\hangar", "D:\\x", "D:\\y"]);
    const many = Array.from({ length: 40 }, (_, index) => `/home/me/p${String(index)}`);
    expect(activityFolders(many)).toHaveLength(MAX_ACTIVITY_FOLDERS);
    expect(activityFolders()).toEqual([]);
  });
});

describe("firstDayOfWeek", () => {
  it("follows the locale's week, else Monday", () => {
    expect(firstDayOfWeek("de-DE")).toBe(1);
    expect(firstDayOfWeek("en-US")).toBe(0);
    expect(firstDayOfWeek(undefined)).toBe(1);
    expect(firstDayOfWeek("not a tag!")).toBe(1);
  });
});

describe("buildCalendar", () => {
  it("ends with today's week and starts a year ago", () => {
    const calendar = buildCalendar([], NOW, 1);
    expect(calendar.weeks).toHaveLength(CALENDAR_WEEKS);
    for (const column of calendar.weeks) expect(column).toHaveLength(7);
    // Monday first: the last column runs Mon 9 to Sun 15 Nov; Friday on is the future.
    const last = calendar.weeks.at(-1) ?? [];
    expect(last[0]?.date).toEqual(new Date(2026, 10, 9));
    expect(last[3]?.date).toEqual(new Date(2026, 10, 12));
    expect(last.slice(4)).toEqual([null, null, null]);
    const all = days(calendar.weeks);
    expect(all[0]?.date).toEqual(new Date(2025, 10, 13));
    expect(all).toHaveLength(365);
    expect(calendar.total).toBe(0);
    expect(all.every((day) => day.level === 0)).toBe(true);
  });

  it("starts columns on Sunday for such locales", () => {
    const last = buildCalendar([], NOW, 0).weeks.at(-1) ?? [];
    expect(last[0]?.date).toEqual(new Date(2026, 10, 8));
    expect(last[4]?.date).toEqual(new Date(2026, 10, 12));
    expect(last.slice(5)).toEqual([null, null]);
  });

  it("counts commits on local days within the year only", () => {
    const calendar = buildCalendar(
      [
        at(2026, 11, 11, 23, 59),
        at(2026, 11, 12, 0, 0),
        at(2026, 11, 12, 15, 0),
        at(2025, 11, 13, 0, 1),
        // A year and a day ago, and tomorrow (a clock that is off): not in the calendar.
        at(2025, 11, 12, 22, 0),
        at(2026, 11, 13, 9, 0),
      ],
      NOW,
      1,
    );
    const byDay = new Map(days(calendar.weeks).map((day) => [day.date.getTime(), day.count]));
    expect(byDay.get(new Date(2026, 10, 11).getTime())).toBe(1);
    expect(byDay.get(new Date(2026, 10, 12).getTime())).toBe(2);
    expect(byDay.get(new Date(2025, 10, 13).getTime())).toBe(1);
    expect(calendar.total).toBe(4);
  });

  it("keeps whole days across daylight saving changes", () => {
    // The test is only meaningful if the zone really changes its offset on 29 Mar 2026.
    expect(new Date(2026, 2, 29, 12).getTimezoneOffset()).not.toBe(
      new Date(2026, 2, 28, 12).getTimezoneOffset(),
    );
    const calendar = buildCalendar(
      [at(2026, 3, 29, 0, 30), at(2026, 3, 29, 23, 30), at(2026, 10, 25, 23, 30)],
      NOW,
      1,
    );
    const all = days(calendar.weeks);
    all.forEach((day, index) => {
      expect(day.date.getHours()).toBe(0);
      const next = all[index + 1];
      if (next === undefined) return;
      const expected = new Date(
        day.date.getFullYear(),
        day.date.getMonth(),
        day.date.getDate() + 1,
      );
      expect(next.date).toEqual(expected);
    });
    const counts = countByDay([at(2026, 3, 29, 0, 30), at(2026, 3, 29, 23, 30)]);
    expect([...counts.entries()]).toEqual([[20260329, 2]]);
    expect(calendar.total).toBe(3);
  });

  it("grades days by the quartiles of the days with commits", () => {
    const times = [1, 2, 3, 4, 8].flatMap((count, index) =>
      Array.from({ length: count }, () => at(2026, 10, 1 + index)),
    );
    const levels = days(buildCalendar(times, NOW, 1).weeks)
      .filter((day) => day.count > 0)
      .map((day) => [day.count, day.level]);
    expect(levels).toEqual([
      [1, 1],
      [2, 1],
      [3, 2],
      [4, 3],
      [8, 4],
    ]);
  });
});

describe("levels", () => {
  it("bounds come from the days with commits only", () => {
    expect(levelBounds([])).toEqual([0, 0, 0]);
    expect(levelBounds([0, 0, 5])).toEqual([5, 5, 5]);
    expect(levelBounds([0, 1, 2, 3, 4, 5])).toEqual([2, 3, 4]);
  });

  it("maps counts to five levels", () => {
    const bounds: [number, number, number] = [2, 3, 4];
    expect([0, 1, 2, 3, 4, 5, 99].map((count) => levelOf(count, bounds))).toEqual([
      0, 1, 1, 2, 3, 4, 4,
    ]);
    // All days alike: every day with commits is the lightest step.
    expect(levelOf(1, levelBounds([1, 1, 1]))).toBe(1);
  });
});

describe("narrow windows", () => {
  it("shows the most recent weeks that fit", () => {
    expect(weeksThatFit(53 * 12 - 2, 10, 2)).toBe(53);
    expect(weeksThatFit(2000, 10, 2)).toBe(53);
    expect(weeksThatFit(118, 10, 2)).toBe(10);
    expect(weeksThatFit(5, 10, 2)).toBe(1);
    expect(weeksThatFit(0, 10, 2)).toBe(CALENDAR_WEEKS);
    const calendar = buildCalendar([], NOW, 1);
    const recent = recentWeeks(calendar, 10);
    expect(recent).toHaveLength(10);
    expect(recent.at(-1)).toBe(calendar.weeks.at(-1));
    expect(recentWeeks(calendar, 0)).toHaveLength(1);
  });
});

describe("monthLabels", () => {
  it("labels each month at its first column, with room between labels", () => {
    const labels = monthLabels(buildCalendar([], NOW, 1).weeks);
    const months = labels.map((label) => label.date.getMonth());
    // The partial November at the start has three columns before December: room for its label.
    expect(months).toEqual([10, 11, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    // Starting on the 20th leaves two columns: that label gives way.
    const later = monthLabels(buildCalendar([], new Date(2026, 10, 19), 1).weeks);
    expect(later.map((label) => label.date.getMonth()).slice(0, 2)).toEqual([11, 0]);
    labels.forEach((label, index) => {
      const previous = labels[index - 1];
      if (previous !== undefined) expect(label.week - previous.week).toBeGreaterThanOrEqual(3);
    });
    expect(monthLabels([])).toEqual([]);
  });
});
