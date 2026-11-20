import { describe, expect, it } from "vitest";

import type { DailyReview, ReviewProject } from "./dailyReview";
import { createTranslator } from "./i18n";
import {
  COMMIT_COUNT,
  countText,
  morningDue,
  morningSummary,
  PROJECT_COUNT,
} from "./morningReview";
import { noticeText, projectLines } from "./MorningNotice";

const HOUR = 3_600_000;
const english = createTranslator("en");
const german = createTranslator("de");

function project(overrides: Partial<ReviewProject> = {}): ReviewProject {
  return {
    path: "/work/hangar",
    isRepo: true,
    activeMs: 2 * HOUR,
    runMs: 3 * HOUR,
    sessions: [
      {
        key: "r-1",
        title: "Fix login",
        program: "Claude Code",
        cwd: "/work/hangar",
        worktree: false,
        first: 0,
        last: HOUR,
        activeMs: HOUR,
        runMs: HOUR,
        running: false,
      },
      {
        key: "r-2",
        title: "Docs",
        program: "Codex CLI",
        cwd: "/work/hangar/.claude/worktrees/docs",
        worktree: true,
        first: 0,
        last: HOUR,
        activeMs: HOUR,
        runMs: HOUR,
        running: false,
      },
    ],
    commits: [
      { hash: "b".repeat(40), subject: "feat: newest", time: 2, files: [], fileCount: 0 },
      { hash: "a".repeat(40), subject: "fix: older", time: 1, files: [], fileCount: 0 },
    ],
    files: [],
    fileCount: 0,
    uncommitted: [
      { path: "/work/hangar", worktree: false, files: ["a.ts", "b.ts"], count: 2 },
      { path: "/work/hangar/.claude/worktrees/docs", worktree: true, files: ["c.md"], count: 1 },
    ],
    ...overrides,
  };
}

function review(projects: ReviewProject[]): DailyReview {
  return {
    start: 0,
    end: 24 * HOUR,
    activeMs: 5 * HOUR,
    runMs: 6 * HOUR,
    sessionCount: 3,
    commitCount: 12,
    projects,
    truncated: false,
  };
}

describe("morningDue", () => {
  const now = new Date(2026, 10, 20, 8, 15);

  it("offers yesterday once, then not again that day", () => {
    expect(morningDue({ now, enabled: true, offeredDay: null })).toBe("2026-11-19");
    expect(morningDue({ now, enabled: true, offeredDay: "2026-11-18" })).toBe("2026-11-19");
    expect(morningDue({ now, enabled: true, offeredDay: "2026-11-19" })).toBeNull();
    // Late in the evening of the same day it stays offered.
    const evening = new Date(2026, 10, 20, 23, 59);
    expect(morningDue({ now: evening, enabled: true, offeredDay: "2026-11-19" })).toBeNull();
  });

  it("is due again right after midnight when Hangar ran through the night", () => {
    const beforeMidnight = new Date(2026, 10, 20, 23, 59, 59);
    const afterMidnight = new Date(2026, 10, 21, 0, 0, 1);
    const offered = morningDue({ now: beforeMidnight, enabled: true, offeredDay: "2026-11-19" });
    expect(offered).toBeNull();
    expect(morningDue({ now: afterMidnight, enabled: true, offeredDay: "2026-11-19" })).toBe(
      "2026-11-20",
    );
  });

  it("crosses months, years and daylight saving days by the calendar", () => {
    expect(morningDue({ now: new Date(2027, 0, 1, 9), enabled: true, offeredDay: null })).toBe(
      "2026-12-31",
    );
    expect(morningDue({ now: new Date(2026, 2, 1, 9), enabled: true, offeredDay: null })).toBe(
      "2026-02-28",
    );
    // The day after the change to summer time in Europe (29 March 2026).
    expect(morningDue({ now: new Date(2026, 2, 30, 0, 30), enabled: true, offeredDay: null })).toBe(
      "2026-03-29",
    );
  });

  it("stays quiet when turned off or when the clock was set back", () => {
    expect(morningDue({ now, enabled: false, offeredDay: null })).toBeNull();
    expect(morningDue({ now, enabled: true, offeredDay: "2026-11-25" })).toBeNull();
  });
});

describe("morningSummary", () => {
  it("has three facts per project: time and sessions, commits, what is open", () => {
    const summary = morningSummary(
      review([project(), project({ path: "/notes", isRepo: false, commits: [], uncommitted: [] })]),
    );
    expect(summary).not.toBeNull();
    expect(summary?.commits).toBe(12);
    expect(summary?.activeMs).toBe(5 * HOUR);
    expect(summary?.projects[0]).toEqual({
      path: "/work/hangar",
      name: "hangar",
      activeMs: 2 * HOUR,
      sessions: 2,
      isRepo: true,
      commits: 2,
      lastSubject: "feat: newest",
      openFiles: 3,
    });
    expect(summary?.projects[1]).toMatchObject({ isRepo: false, lastSubject: null, openFiles: 0 });
  });

  it("offers nothing for a day without sessions", () => {
    expect(morningSummary(review([]))).toBeNull();
  });

  it("reads as the notice and the short form say it", () => {
    const summary = morningSummary(
      review([
        project(),
        project({ path: "/b" }),
        project({ path: "/c", isRepo: false, commits: [], uncommitted: [] }),
      ]),
    );
    if (summary === null) throw new Error("no summary");
    expect(noticeText(summary, "en", english)).toBe("Yesterday: 3 projects, 5 hr, 12 commits");
    expect(noticeText(summary, "de", german)).toBe("Gestern: 3 Projekte, 5 Std., 12 Commits");
    const [first, , third] = summary.projects;
    if (first === undefined || third === undefined) throw new Error("missing project");
    expect(projectLines(first, "en", english)).toEqual([
      "Active 2 hr · 2 sessions",
      "2 commits, last: feat: newest",
      "Still open: 3 files uncommitted",
    ]);
    expect(
      projectLines({ ...first, commits: 0, lastSubject: null, openFiles: 0 }, "en", english),
    ).toEqual(["Active 2 hr · 2 sessions", "No commits", "Nothing open, all committed"]);
    expect(projectLines(third, "en", english)).toEqual([
      "Active 2 hr · 2 sessions",
      "Not a git repository",
      null,
    ]);
  });
});

describe("countText", () => {
  it("picks the singular by the language's plural rule", () => {
    expect(countText(1, PROJECT_COUNT, "en", english)).toBe("1 project");
    expect(countText(0, PROJECT_COUNT, "en", english)).toBe("0 projects");
    expect(countText(1, COMMIT_COUNT, "de", german)).toBe("1 Commit");
    expect(countText(12, COMMIT_COUNT, "de", german)).toBe("12 Commits");
    expect(countText(2, PROJECT_COUNT, "not a tag", english)).toBe("2 projects");
  });
});
