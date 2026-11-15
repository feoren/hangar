import { afterAll, beforeAll, describe, expect, it } from "vitest";

import {
  clampDay,
  codeSpan,
  dayInputValue,
  dayRange,
  dayWindow,
  escapeMarkdown,
  formatClock,
  formatSpan,
  LIST_LIMIT,
  parseDailyReview,
  parseDayInput,
  PhaseRecorder,
  REVIEW_DAYS,
  reviewMarkdown,
  sessionName,
  shiftDay,
  type DailyReview,
} from "./dailyReview";
import { createTranslator } from "./i18n";
import { formatDuration } from "./statusLine";

// A zone with daylight saving time, so day boundaries around the change are tested for real.
const previousZone = process.env.TZ;
beforeAll(() => {
  process.env.TZ = "Europe/Berlin";
});
afterAll(() => {
  process.env.TZ = previousZone;
});

const en = createTranslator("en");
const de = createTranslator("de");
const MIN = 60_000;
const HOUR = 60 * MIN;

function local(year: number, month: number, day: number, hour = 0, minute = 0): number {
  return new Date(year, month - 1, day, hour, minute).getTime();
}

function sample(): DailyReview {
  const day = local(2026, 11, 14);
  return {
    start: day,
    end: day + 24 * HOUR,
    activeMs: 75 * MIN,
    runMs: 3 * HOUR,
    sessionCount: 2,
    commitCount: 1,
    truncated: false,
    projects: [
      {
        path: "C:\\Work\\my app",
        isRepo: true,
        activeMs: 75 * MIN,
        runMs: 3 * HOUR,
        sessions: [
          {
            key: "r-1",
            title: "Fix *login*",
            program: "Claude Code",
            cwd: "C:\\Work\\my app",
            worktree: false,
            first: day + 9 * HOUR,
            last: day + 11 * HOUR,
            activeMs: HOUR,
            runMs: 2 * HOUR,
            running: false,
          },
          {
            key: "r-2",
            title: "Codex",
            program: "Codex",
            cwd: "C:\\Work\\wt",
            worktree: true,
            first: day + 10 * HOUR,
            last: day + 12 * HOUR,
            activeMs: 30 * MIN,
            runMs: 2 * HOUR,
            running: true,
          },
        ],
        commits: [
          {
            hash: "0123456789abcdef0123456789abcdef01234567",
            subject: "feat: add [link](x)",
            time: day + 10 * HOUR + 31 * MIN,
            files: ["src/a.ts"],
            fileCount: 1,
          },
        ],
        files: ["src/a.ts"],
        fileCount: 1,
        uncommitted: [
          { path: "C:\\Work\\my app", worktree: false, files: ["notes.md"], count: 1 },
          { path: "C:\\Work\\wt", worktree: true, files: ["b.ts", "c.ts"], count: 3 },
        ],
      },
    ],
  };
}

describe("parseDailyReview", () => {
  it("reads what the backend sends", () => {
    const review = sample();
    expect(parseDailyReview(JSON.parse(JSON.stringify(review)))).toEqual(review);
  });

  it("refuses malformed answers", () => {
    expect(parseDailyReview(null)).toBeNull();
    expect(parseDailyReview([])).toBeNull();
    expect(parseDailyReview({ ...sample(), truncated: "no" })).toBeNull();
    const broken = sample();
    const project = broken.projects[0];
    if (!project) throw new Error("sample has a project");
    expect(
      parseDailyReview({ ...broken, projects: [{ ...project, sessions: [{ key: 1 }] }] }),
    ).toBeNull();
    expect(parseDailyReview({ ...broken, projects: [{ ...project, files: [1] }] })).toBeNull();
    expect(parseDailyReview({ ...broken, activeMs: Number.NaN })).toBeNull();
  });
});

describe("days", () => {
  it("is a local day, 23 or 25 hours across daylight saving changes", () => {
    const normal = dayWindow(new Date(2026, 10, 14, 15, 30));
    expect(normal).toEqual({ start: local(2026, 11, 14), end: local(2026, 11, 15) });
    expect(normal.end - normal.start).toBe(24 * HOUR);
    const spring = dayWindow(new Date(2026, 2, 29, 12));
    expect(spring.end - spring.start).toBe(23 * HOUR);
    const autumn = dayWindow(new Date(2026, 9, 25, 12));
    expect(autumn.end - autumn.start).toBe(25 * HOUR);
  });

  it("steps by calendar days, also over a change", () => {
    expect(shiftDay(new Date(2026, 9, 25), 1).getTime()).toBe(local(2026, 10, 26));
    expect(shiftDay(new Date(2026, 0, 1), -1).getTime()).toBe(local(2025, 12, 31));
  });

  it("stays within the kept days", () => {
    const now = new Date(2026, 10, 14, 18);
    const { first, last } = dayRange(now);
    expect(last.getTime()).toBe(local(2026, 11, 14));
    expect(Math.round((last.getTime() - first.getTime()) / (24 * HOUR))).toBe(REVIEW_DAYS - 1);
    expect(clampDay(new Date(2027, 0, 1), now).getTime()).toBe(last.getTime());
    expect(clampDay(new Date(2020, 0, 1), now).getTime()).toBe(first.getTime());
    expect(clampDay(new Date(2026, 10, 10, 9), now).getTime()).toBe(local(2026, 11, 10));
  });

  it("round-trips the date input", () => {
    expect(dayInputValue(new Date(2026, 1, 3))).toBe("2026-02-03");
    expect(parseDayInput("2026-02-03")?.getTime()).toBe(local(2026, 2, 3));
    expect(parseDayInput("2026-02-30")).toBeNull();
    expect(parseDayInput("")).toBeNull();
    expect(parseDayInput("3.2.2026")).toBeNull();
  });
});

describe("formatSpan", () => {
  it("joins hours and minutes from the duration formatter", () => {
    expect(formatSpan(2 * HOUR + 45 * MIN + 30_000, "en", en)).toBe(
      [formatDuration(2 * HOUR, "en"), formatDuration(45 * MIN, "en")].join(" "),
    );
    expect(formatSpan(3 * HOUR, "en", en)).toBe(formatDuration(3 * HOUR, "en"));
    expect(formatSpan(5 * MIN, "en", en)).toBe(formatDuration(5 * MIN, "en"));
    // Days stay hours: a review covers one day.
    expect(formatSpan(25 * HOUR, "en", en)).toBe(
      new Intl.NumberFormat("en", { style: "unit", unit: "hour", unitDisplay: "short" }).format(25),
    );
  });

  it("says so below a minute", () => {
    expect(formatSpan(20_000, "de", de)).toBe("unter 1 Min.");
    expect(formatSpan(0, "en", en)).toBe("under 1 min");
  });
});

describe("Markdown", () => {
  it("escapes markup and keeps code spans whole", () => {
    expect(escapeMarkdown("Fix *login* [x](y) <b>\n# no")).toBe(
      "Fix \\*login\\* \\[x\\](y) \\<b\\> \\# no",
    );
    expect(codeSpan("src/a.ts")).toBe("`src/a.ts`");
    expect(codeSpan("a`b")).toBe("``a`b``");
    expect(codeSpan("`x")).toBe("`` `x ``");
  });

  it("names a session by program and title", () => {
    expect(sessionName({ program: "Codex", title: "Codex" })).toBe("Codex");
    expect(sessionName({ program: "Codex", title: " " })).toBe("Codex");
    expect(sessionName({ program: "Claude Code", title: "Fix" })).toBe("Claude Code: Fix");
  });

  it("renders summary, sessions, commits, files and uncommitted changes", () => {
    const review = sample();
    const day = new Date(review.start);
    const text = reviewMarkdown(review, day, "en", en);
    const clock = (ms: number) => formatClock(ms, "en");
    const span = (ms: number) => formatSpan(ms, "en", en);
    expect(text.startsWith("# Daily review: Saturday, November 14, 2026\n\n")).toBe(true);
    expect(text).toContain(`- Active time: ${span(75 * MIN)}\n- Sessions: 2\n- Projects: 1\n`);
    expect(text).toContain("## my app\n\n`C:\\Work\\my app`\n");
    expect(text).toContain(
      `- Claude Code: Fix \\*login\\*, ${clock(review.start + 9 * HOUR)} to ${clock(review.start + 11 * HOUR)}: active ${span(HOUR)}, run time ${span(2 * HOUR)}\n`,
    );
    expect(text).toContain("(Worktree, still running)");
    expect(text).toContain(
      `- \`0123456\` feat: add \\[link\\](x) (${clock(review.start + 10 * HOUR + 31 * MIN)})`,
    );
    expect(text).toContain("### Changed files\n\n- `src/a.ts`\n");
    expect(text).toContain(
      "### Currently uncommitted\n\n- `notes.md`\n- Worktree: `C:\\Work\\wt`\n  - `b.ts`\n  - `c.ts`\n  - and 1 more\n",
    );
    expect(text.endsWith("\n")).toBe(true);
    expect(/[\u2013\u2014]/.test(text)).toBe(false);
  });

  it("says when a day had nothing and when parts are missing", () => {
    const empty: DailyReview = { ...sample(), projects: [], sessionCount: 0, truncated: true };
    const text = reviewMarkdown(empty, new Date(empty.start), "de", de);
    expect(text).toContain("An diesem Tag lief keine Session.");
    expect(text).toContain("_Nicht alles passte in den Rückblick, einiges fehlt._");
  });

  it("cuts long file lists", () => {
    const review = sample();
    const project = review.projects[0];
    if (!project) throw new Error("sample has a project");
    project.files = Array.from({ length: LIST_LIMIT + 5 }, (_, index) => `f${String(index)}.ts`);
    project.fileCount = LIST_LIMIT + 9;
    const text = reviewMarkdown(review, new Date(review.start), "en", en);
    expect(text).toContain(`- \`f${String(LIST_LIMIT - 1)}.ts\`\n- and 9 more\n`);
    expect(text).not.toContain(`f${String(LIST_LIMIT)}.ts`);
  });
});

describe("PhaseRecorder", () => {
  it("reports one phase per stretch of work, when it ends", () => {
    const recorder = new PhaseRecorder();
    expect(recorder.change(1, true, 1000)).toBeNull();
    // Repeated "working" changes nothing.
    expect(recorder.change(1, true, 2000)).toBeNull();
    // Quiet after the last output at 4000: the phase ends there, not when the quiet was noticed.
    expect(recorder.change(1, false, 9000, 4000)).toEqual({ id: 1, from: 1000, to: 4000 });
    expect(recorder.change(1, false, 9500)).toBeNull();
  });

  it("ends phases on close and on quit", () => {
    const recorder = new PhaseRecorder();
    recorder.change(1, true, 1000);
    recorder.change(2, true, 2000);
    expect(recorder.end(1, 5000, null)).toEqual({ id: 1, from: 1000, to: 5000 });
    expect(recorder.end(1, 6000)).toBeNull();
    recorder.change(3, true, 7000);
    expect(recorder.flush(8000)).toEqual([
      { id: 2, from: 2000, to: 8000 },
      { id: 3, from: 7000, to: 8000 },
    ]);
    expect(recorder.flush(9000)).toEqual([]);
  });

  it("drops empty phases and ignores an output time outside the phase", () => {
    const recorder = new PhaseRecorder();
    recorder.change(1, true, 1000);
    expect(recorder.change(1, false, 1000)).toBeNull();
    recorder.change(1, true, 2000);
    expect(recorder.change(1, false, 3000, 500)).toEqual({ id: 1, from: 2000, to: 3000 });
  });
});
