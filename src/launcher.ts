// Launcher (Ctrl+Shift+P): one field, one list. Pure ranking of the entries; the dialog builds the
// entries (sessions, program in folder, files, notes, commits, snippets, settings, commands) and
// runs the chosen one.

import { fuzzyMatch } from "./fuzzy";

export const LAUNCHER_SECTIONS = [
  "sessions",
  "start",
  "files",
  "notes",
  "commits",
  "snippets",
  "settings",
  "commands",
] as const;
export type LauncherSection = (typeof LAUNCHER_SECTIONS)[number];

/** Entries shown without a query; more would turn the list into reading instead of picking. */
export const LAUNCHER_LIMIT = 8;

/** With a query: at most this many per section, so thousands of files cannot push out the rest. */
export const SECTION_LIMIT = 5;

/** With a query: at most this many in all. */
export const MATCH_LIMIT = 24;

export interface RankLimits {
  /** Without a query. */
  idle: number;
  /** With a query, per section. */
  perSection: number;
  /** With a query, in all. */
  total: number;
}

const DEFAULT_LIMITS: RankLimits = {
  idle: LAUNCHER_LIMIT,
  perSection: SECTION_LIMIT,
  total: MATCH_LIMIT,
};

export interface LauncherEntry {
  key: string;
  section: LauncherSection;
  label: string;
  /** Dimmed second part, e.g. the full path. Searched as well. */
  detail?: string;
  /** Words that are searched but not shown, e.g. the program name of a session. */
  keywords?: string;
  /** Shown without a query; the others (e.g. every program in every folder) only when searched. */
  primary?: boolean;
  disabled?: boolean;
}

export interface RankedEntry<T extends LauncherEntry> {
  entry: T;
  /** Matched characters of the label, for highlighting. */
  positions: number[];
}

export interface RankedSection<T extends LauncherEntry> {
  section: LauncherSection;
  items: RankedEntry<T>[];
}

/** Searched text per entry, built once: the file index alone can hold thousands of entries. */
const searchTexts = new WeakMap<LauncherEntry, string>();

function searchText(entry: LauncherEntry): string {
  let text = searchTexts.get(entry);
  if (text === undefined) {
    text = [entry.label, entry.detail ?? "", entry.keywords ?? ""].join(" ");
    searchTexts.set(entry, text);
  }
  return text;
}

/**
 * The entries to show for this query, grouped by section. Without a query: the primary entries in
 * their given order, at most `idle`. With a query: the best matches, at most `perSection` per
 * section and `total` in all; sections appear in the order of their best entry, entries within a
 * section by score. Ties keep the given order.
 */
export function rankEntries<T extends LauncherEntry>(
  entries: readonly T[],
  query: string,
  limits: Partial<RankLimits> = {},
): RankedSection<T>[] {
  const { idle, perSection, total } = { ...DEFAULT_LIMITS, ...limits };
  const searching = query.trim() !== "";
  const ranked: { entry: T; positions: number[]; score: number; index: number }[] = [];
  if (!searching) {
    entries.forEach((entry, index) => {
      if (entry.primary === true) ranked.push({ entry, positions: [], score: 0, index });
    });
  } else {
    entries.forEach((entry, index) => {
      const match = fuzzyMatch(query, searchText(entry));
      if (match === null) return;
      const positions = match.positions.filter((position) => position < entry.label.length);
      ranked.push({ entry, positions, score: match.score, index });
    });
    ranked.sort((a, b) => b.score - a.score || a.index - b.index);
  }
  const cap = searching ? total : idle;
  const sections: RankedSection<T>[] = [];
  let shown = 0;
  for (const item of ranked) {
    if (shown >= cap) break;
    let group = sections.find((section) => section.section === item.entry.section);
    if (!group) {
      group = { section: item.entry.section, items: [] };
      sections.push(group);
    }
    if (searching && group.items.length >= perSection) continue;
    group.items.push({ entry: item.entry, positions: item.positions });
    shown += 1;
  }
  return sections;
}

/** Entries in display order, for keyboard navigation across sections. */
export function flatEntries<T extends LauncherEntry>(sections: readonly RankedSection<T>[]): T[] {
  return sections.flatMap((section) => section.items.map((item) => item.entry));
}

/**
 * Next selectable index for arrow keys; wraps around and skips disabled entries. `-1` when no
 * entry can be selected.
 */
export function moveSelection(
  entries: readonly Pick<LauncherEntry, "disabled">[],
  current: number,
  step: 1 | -1,
): number {
  const count = entries.length;
  for (let offset = 1; offset <= count; offset += 1) {
    const index = (((current + step * offset) % count) + count) % count;
    if (entries[index]?.disabled !== true) return index;
  }
  return -1;
}

/** First selectable entry, `-1` if there is none. */
export function firstSelectable(entries: readonly Pick<LauncherEntry, "disabled">[]): number {
  return entries.findIndex((entry) => entry.disabled !== true);
}
