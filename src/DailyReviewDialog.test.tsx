import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { DailyReviewDialog, ReviewContent } from "./DailyReviewDialog";
import type { DailyReview } from "./dailyReview";
import { createTranslator, I18nContext } from "./i18n";

const english = createTranslator("en");
const HOUR = 3_600_000;
const DAY = new Date(2026, 10, 14).getTime();

function render(element: ReactElement): string {
  return renderToStaticMarkup(
    <I18nContext.Provider value={english}>{element}</I18nContext.Provider>,
  );
}

const REVIEW: DailyReview = {
  start: DAY,
  end: DAY + 24 * HOUR,
  activeMs: HOUR,
  runMs: 2 * HOUR,
  sessionCount: 1,
  commitCount: 1,
  truncated: false,
  projects: [
    {
      path: "/work/hangar",
      isRepo: true,
      activeMs: HOUR,
      runMs: 2 * HOUR,
      sessions: [
        {
          key: "r-1",
          title: "Fix login",
          program: "Claude Code",
          cwd: "/work/hangar",
          worktree: true,
          first: DAY + 9 * HOUR,
          last: DAY + 11 * HOUR,
          activeMs: HOUR,
          runMs: 2 * HOUR,
          running: true,
        },
      ],
      commits: [
        {
          hash: "abcdef0123456789abcdef0123456789abcdef01",
          subject: "fix: <login> redirect",
          time: DAY + 10 * HOUR,
          files: ["src/login.ts"],
          fileCount: 1,
        },
      ],
      files: ["src/login.ts"],
      fileCount: 1,
      uncommitted: [{ path: "/work/hangar", worktree: false, files: ["a.ts"], count: 1 }],
    },
  ],
};

describe("DailyReviewDialog", () => {
  it("opens on today, loading, with export disabled until the review is there", () => {
    const now = () => new Date(2026, 10, 14, 18, 0);
    const html = render(
      <DailyReviewDialog
        language="en"
        load={() => new Promise(() => undefined)}
        onCopy={() => Promise.resolve()}
        onToNotes={() => Promise.resolve()}
        onClose={() => undefined}
        now={now}
      />,
    );
    expect(html).toContain("Daily review");
    expect(html).toContain('value="2026-11-14"');
    expect(html).toContain('max="2026-11-14"');
    expect(html).toContain('min="2026-08-17"');
    expect(html).toContain("Loading the review");
    // Next day and "Today" are off on today; copying and saving wait for the review.
    expect(html.match(/disabled=""/g)?.length).toBe(4);
  });
});

describe("ReviewContent", () => {
  it("shows the summary and per project the sessions, commits and files", () => {
    const html = render(<ReviewContent review={REVIEW} language="en" t={english} />);
    expect(html).toContain("<dt>Active time</dt>");
    expect(html).toContain("<dt>Commits</dt><dd>1</dd>");
    expect(html).toContain('<h3 title="/work/hangar">hangar</h3>');
    expect(html).toContain("Claude Code: Fix login");
    expect(html).toContain("Worktree");
    expect(html).toContain("still running");
    expect(html).toContain("<code>abcdef0</code>");
    // Text, never markup.
    expect(html).toContain("fix: &lt;login&gt; redirect");
    expect(html).toContain("Changed files (1)");
    expect(html).toContain("Currently uncommitted (1)");
  });

  it("says so for a day without sessions", () => {
    const html = render(
      <ReviewContent review={{ ...REVIEW, projects: [] }} language="en" t={english} />,
    );
    expect(html).toContain("No session ran on this day.");
    expect(html).not.toContain("<dl");
  });
});
