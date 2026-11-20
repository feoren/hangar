// @vitest-environment happy-dom

// When the morning summary shows: on the first start of a day, on the first focus after midnight,
// once per day, never while it is turned off, and again after a failed load.

import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import type { DailyReview } from "./dailyReview";
import { createTranslator, I18nContext } from "./i18n";
import { MorningNotice } from "./MorningNotice";

Reflect.set(globalThis, "IS_REACT_ACT_ENVIRONMENT", true);

const t = createTranslator("en");
const HOUR = 3_600_000;

const REVIEW: DailyReview = {
  start: 0,
  end: 24 * HOUR,
  activeMs: 5 * HOUR,
  runMs: 6 * HOUR,
  sessionCount: 1,
  commitCount: 1,
  truncated: false,
  projects: [
    {
      path: "/work/hangar",
      isRepo: true,
      activeMs: 5 * HOUR,
      runMs: 6 * HOUR,
      sessions: [],
      commits: [{ hash: "a".repeat(40), subject: "fix: login", time: 1, files: [], fileCount: 0 }],
      files: [],
      fileCount: 0,
      uncommitted: [],
    },
  ],
};

const EMPTY: DailyReview = { ...REVIEW, projects: [], commitCount: 0, activeMs: 0 };

let host: HTMLElement;
let root: Root | null = null;
let clock = new Date(2026, 10, 20, 8, 0);
const now = () => clock;

beforeEach(() => {
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
  clock = new Date(2026, 10, 20, 8, 0);
});

afterEach(() => {
  act(() => {
    root?.unmount();
  });
  root = null;
  host.remove();
  document.body.replaceChildren();
});

interface Harness {
  loads: { start: number; end: number }[];
  offered: string[];
  opened: Date[];
  answer: () => Promise<unknown>;
}

function harness(answer: () => Promise<unknown>): Harness {
  return { loads: [], offered: [], opened: [], answer };
}

async function show(h: Harness, enabled: boolean, offeredDay: string | null) {
  await act(async () => {
    root?.render(
      <I18nContext.Provider value={t}>
        <MorningNotice
          language="en"
          enabled={enabled}
          offeredDay={offeredDay}
          onOffered={(day) => {
            h.offered.push(day);
          }}
          load={(start, end) => {
            h.loads.push({ start, end });
            return h.answer();
          }}
          onOpenReview={(day) => {
            h.opened.push(day);
          }}
          now={now}
        />
      </I18nContext.Provider>,
    );
    await Promise.resolve();
  });
}

async function focus() {
  await act(async () => {
    window.dispatchEvent(new Event("focus"));
    await Promise.resolve();
  });
}

function notice(): Element | null {
  return document.querySelector(".morning-notice");
}

describe("MorningNotice", () => {
  it("sums up yesterday on the first start of the day and stores the day", async () => {
    const h = harness(() => Promise.resolve(REVIEW));
    await show(h, true, "2026-11-18");
    expect(h.loads).toEqual([
      {
        start: new Date(2026, 10, 19).getTime(),
        end: new Date(2026, 10, 20).getTime(),
      },
    ]);
    expect(h.offered).toEqual(["2026-11-19"]);
    expect(notice()?.textContent).toContain("Yesterday: 1 project, 5 hr, 1 commit");
    // The parent stored the day: a focus later that morning loads nothing.
    await show(h, true, "2026-11-19");
    await focus();
    expect(h.loads).toHaveLength(1);
  });

  it("opens the short form and from there the full review of yesterday", async () => {
    const h = harness(() => Promise.resolve(REVIEW));
    await show(h, true, null);
    const view = [...document.querySelectorAll("button")].find(
      (button) => button.textContent === "View",
    );
    await act(async () => {
      view?.click();
      await Promise.resolve();
    });
    const dialog = document.querySelector(".dialog.morning-review");
    expect(dialog?.textContent).toContain("1 commit, last: fix: login");
    expect(dialog?.textContent).toContain("Nothing open, all committed");
    expect(notice()).toBeNull();
    const full = [...document.querySelectorAll("button")].find(
      (button) => button.textContent === "Full daily review",
    );
    await act(async () => {
      full?.click();
      await Promise.resolve();
    });
    expect(h.opened.map((day) => day.getTime())).toEqual([new Date(2026, 10, 19).getTime()]);
    expect(document.querySelector(".dialog.morning-review")).toBeNull();
  });

  it("stores the day but shows nothing when yesterday had no sessions", async () => {
    const h = harness(() => Promise.resolve(EMPTY));
    await show(h, true, null);
    expect(h.offered).toEqual(["2026-11-19"]);
    expect(notice()).toBeNull();
  });

  it("waits for the first focus after midnight when Hangar ran through the night", async () => {
    const h = harness(() => Promise.resolve(REVIEW));
    await show(h, true, "2026-11-19");
    expect(h.loads).toHaveLength(0);
    clock = new Date(2026, 10, 21, 0, 5);
    expect(h.loads).toHaveLength(0);
    await focus();
    expect(h.loads).toHaveLength(1);
    expect(h.offered).toEqual(["2026-11-20"]);
    expect(notice()).not.toBeNull();
  });

  it("does nothing while turned off or before the preferences are read", async () => {
    const h = harness(() => Promise.resolve(REVIEW));
    await show(h, false, null);
    await focus();
    expect(h.loads).toHaveLength(0);
    expect(notice()).toBeNull();
  });

  it("tries again on the next focus after a failed load", async () => {
    let fail = true;
    const h = harness(() => (fail ? Promise.reject(new Error("no git")) : Promise.resolve(REVIEW)));
    await show(h, true, null);
    expect(h.offered).toEqual([]);
    fail = false;
    await focus();
    expect(h.loads).toHaveLength(2);
    expect(h.offered).toEqual(["2026-11-19"]);
  });
});
