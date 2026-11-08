// Launcher (Ctrl+Shift+P): one field, one list. Pure ranking of the entries; the dialog builds the
// entries (sessions, program in folder, notes, snippets, commands) and runs the chosen one.

import { fuzzyMatch } from "./fuzzy";

export const LAUNCHER_SECTIONS = ["sessions", "start", "notes", "snippets", "commands"] as const;
export type LauncherSection = (typeof LAUNCHER_SECTIONS)[number];

/** Entries shown at most; more would turn the list into reading instead of picking. */
export const LAUNCHER_LIMIT = 8;

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

/**
 * The entries to show for this query, grouped by section. Without a query: the primary entries in
 * their given order. With a query: the best matches; sections appear in the order of their best
 * entry, entries within a section by score. Ties keep the given order.
 */
export function rankEntries<T extends LauncherEntry>(
  entries: readonly T[],
  query: string,
  limit: number = LAUNCHER_LIMIT,
): RankedSection<T>[] {
  const ranked: { entry: T; positions: number[]; score: number; index: number }[] = [];
  if (query.trim() === "") {
    entries.forEach((entry, index) => {
      if (entry.primary === true) ranked.push({ entry, positions: [], score: 0, index });
    });
  } else {
    entries.forEach((entry, index) => {
      const text = [entry.label, entry.detail ?? "", entry.keywords ?? ""].join(" ");
      const match = fuzzyMatch(query, text);
      if (match === null) return;
      const positions = match.positions.filter((position) => position < entry.label.length);
      ranked.push({ entry, positions, score: match.score, index });
    });
    ranked.sort((a, b) => b.score - a.score || a.index - b.index);
  }
  const sections: RankedSection<T>[] = [];
  for (const item of ranked.slice(0, limit)) {
    let group = sections.find((section) => section.section === item.entry.section);
    if (!group) {
      group = { section: item.entry.section, items: [] };
      sections.push(group);
    }
    group.items.push({ entry: item.entry, positions: item.positions });
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
