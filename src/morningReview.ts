// Morning summary (ADR 0011): on the first start of a day, or on the first focus of the window
// once Hangar ran past midnight, a quiet notice sums up yesterday ("Yesterday: 3 projects, 5 h,
// 12 commits") and opens a short form of the daily review. Decides when it is due, from an
// injected clock and the day it was last offered for (kept by the backend, mainStore.ts), and
// reduces a review to the short form. Pure.

import { dayInputValue, shiftDay, startOfDay, type DailyReview } from "./dailyReview";
import { folderName } from "./folders";
import type { MessageKey, Translate } from "./i18n";

export interface MorningCheck {
  now: Date;
  /** The setting; on by default. */
  enabled: boolean;
  /** The day ("YYYY-MM-DD") whose summary was last offered; `null`: never. */
  offeredDay: string | null;
}

/**
 * The local day ("YYYY-MM-DD") whose summary is due: yesterday, unless it was offered already.
 * A stored day after yesterday (the clock was set back) counts as offered too, so a summary is
 * never offered twice.
 */
export function morningDue(check: MorningCheck): string | null {
  if (!check.enabled) return null;
  const yesterday = dayInputValue(shiftDay(startOfDay(check.now), -1));
  if (check.offeredDay !== null && check.offeredDay >= yesterday) return null;
  return yesterday;
}

/** One project in the short form: three lines. */
export interface MorningProject {
  path: string;
  name: string;
  activeMs: number;
  sessions: number;
  isRepo: boolean;
  commits: number;
  /** The newest commit's subject; `null` without commits. */
  lastSubject: string | null;
  /** Files uncommitted right now, over all work trees of the project. */
  openFiles: number;
}

export interface MorningSummary {
  projects: MorningProject[];
  activeMs: number;
  commits: number;
}

/** The short form of a review; `null` for a day without sessions, which is not offered. */
export function morningSummary(review: DailyReview): MorningSummary | null {
  if (review.projects.length === 0) return null;
  return {
    activeMs: review.activeMs,
    commits: review.commitCount,
    projects: review.projects.map((project) => ({
      path: project.path,
      name: folderName(project.path),
      activeMs: project.activeMs,
      sessions: project.sessions.length,
      isRepo: project.isRepo,
      commits: project.commits.length,
      // The backend lists commits newest first.
      lastSubject: project.commits[0]?.subject ?? null,
      openFiles: project.uncommitted.reduce((sum, tree) => sum + tree.count, 0),
    })),
  };
}

/** "1 project", "3 projects": the language's plural rule picks the text. */
export function countText(
  count: number,
  keys: { one: MessageKey; other: MessageKey },
  language: string,
  t: Translate,
): string {
  let rule: Intl.LDMLPluralRule = "other";
  try {
    rule = new Intl.PluralRules(language).select(count);
  } catch {
    // An unknown language tag: the plural text, with the number, reads right enough.
  }
  return t(rule === "one" ? keys.one : keys.other, { count });
}

export const PROJECT_COUNT = { one: "morning.projectOne", other: "morning.projectMany" } as const;
export const COMMIT_COUNT = { one: "morning.commitOne", other: "morning.commitMany" } as const;
export const SESSION_COUNT = { one: "morning.sessionOne", other: "morning.sessionMany" } as const;
export const FILE_COUNT = { one: "morning.fileOne", other: "morning.fileMany" } as const;
