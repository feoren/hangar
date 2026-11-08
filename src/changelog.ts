// What's New: entries from src/changelog.json and the decision when the dialog appears on its
// own. The file is JSON rather than TypeScript because tools/release.py reads the same source
// without extra packages (notes for latest.json). It comes in as text (`?raw`) and is validated
// here as `unknown`; invalid entries are dropped instead of disturbing the app.

import { LANGUAGES, type Language } from "./i18n";
import { parseVersion } from "./version";

/** Texts per language; `de` and `en` are required, missing languages show English. */
export type ChangelogText = Readonly<Partial<Record<Language, string>>> & {
  readonly de: string;
  readonly en: string;
};

export interface ChangelogEntry {
  /** X.Y.Z without build number. */
  version: string;
  /** ISO date (YYYY-MM-DD) or `null`: the version is being prepared and not released yet. */
  date: string | null;
  items: ChangelogText[];
}

const BASE_VERSION = /^(\d+)\.(\d+)\.(\d+)$/;
const ISO_DATE = /^(\d{4})-(\d{2})-(\d{2})$/;
/** Longest allowed item; anything longer is no longer a short, clear sentence. */
export const MAX_ITEM_LENGTH = 160;

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function validDate(text: string): boolean {
  const match = ISO_DATE.exec(text);
  if (!match) return false;
  const [, y, m, d] = match.map(Number);
  if (y === undefined || m === undefined || d === undefined) return false;
  const date = new Date(Date.UTC(y, m - 1, d));
  return date.getUTCFullYear() === y && date.getUTCMonth() === m - 1 && date.getUTCDate() === d;
}

/** Compares X.Y.Z (build numbers do not count); anything unreadable counts as the lowest version. */
export function compareVersions(a: string, b: string): number {
  const left = parseVersion(a);
  const right = parseVersion(b);
  if (!left || !right) return (left ? 1 : 0) - (right ? 1 : 0);
  return left.major - right.major || left.minor - right.minor || left.revision - right.revision;
}

/** "0.2.0+57" gives "0.2.0"; anything unreadable gives `null`. */
export function baseVersion(text: string): string | null {
  const version = parseVersion(text);
  if (!version) return null;
  return `${String(version.major)}.${String(version.minor)}.${String(version.revision)}`;
}

function textProblems(value: unknown, where: string): string[] {
  if (!isRecord(value)) return [`${where}: not an object`];
  const problems: string[] = [];
  for (const required of ["de", "en"] as const) {
    if (!(required in value)) problems.push(`${where}: language ${required} missing`);
  }
  for (const [language, text] of Object.entries(value)) {
    const at = `${where}.${language}`;
    if (!LANGUAGES.some((code) => code === language)) problems.push(`${at}: unknown language`);
    if (typeof text !== "string" || text.trim() === "") {
      problems.push(`${at}: empty`);
      continue;
    }
    if (/[–—]/.test(text)) problems.push(`${at}: en or em dash`);
    if (text.length > MAX_ITEM_LENGTH)
      problems.push(`${at}: longer than ${String(MAX_ITEM_LENGTH)}`);
    if (text !== text.trim()) problems.push(`${at}: leading or trailing whitespace`);
  }
  return problems;
}

/**
 * Every schema violation as a readable list; empty means valid. Checks required languages,
 * versions, date, descending order, empty texts, dashes and length.
 */
export function changelogProblems(raw: unknown): string[] {
  if (!Array.isArray(raw)) return ["root is not a list"];
  const problems: string[] = [];
  let previous: string | null = null;
  raw.forEach((entry: unknown, index) => {
    const where = `entry ${String(index)}`;
    if (!isRecord(entry)) {
      problems.push(`${where}: not an object`);
      return;
    }
    const { version, date, items } = entry;
    if (typeof version !== "string" || !BASE_VERSION.test(version)) {
      problems.push(`${where}: version must be X.Y.Z`);
    } else {
      if (previous !== null && compareVersions(previous, version) <= 0) {
        problems.push(`${where}: ${version} is not in descending order after ${previous}`);
      }
      previous = version;
    }
    if (date !== null && (typeof date !== "string" || !validDate(date))) {
      problems.push(`${where}: date must be YYYY-MM-DD or null`);
    }
    if (date === null && index > 0)
      problems.push(`${where}: only the newest version may have no date`);
    // After a release, tools/release.py --next creates the upcoming version empty; items are
    // added along with the work. Released versions need items.
    const emptyAllowed = date === null && index === 0;
    if (!Array.isArray(items) || (items.length === 0 && !emptyAllowed)) {
      problems.push(`${where}: no items`);
      return;
    }
    items.forEach((item: unknown, itemIndex) => {
      problems.push(...textProblems(item, `${where}.items[${String(itemIndex)}]`));
    });
  });
  return problems;
}

function parseText(value: unknown): ChangelogText | null {
  if (!isRecord(value)) return null;
  const { de, en } = value;
  if (typeof de !== "string" || typeof en !== "string" || !de.trim() || !en.trim()) return null;
  const text: Partial<Record<Language, string>> = {};
  for (const language of LANGUAGES) {
    const candidate = value[language];
    if (typeof candidate === "string" && candidate.trim()) text[language] = candidate.trim();
  }
  return { ...text, de: de.trim(), en: en.trim() };
}

/**
 * Reads the changelog leniently: invalid entries and items are dropped, newest first. A freshly
 * created version without items is dropped too; the dialog would have nothing to say about it.
 */
export function parseChangelog(text: string): ChangelogEntry[] {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return [];
  }
  if (!Array.isArray(raw)) return [];
  const entries: ChangelogEntry[] = [];
  for (const entry of raw) {
    if (!isRecord(entry)) continue;
    const { version, date, items } = entry;
    if (typeof version !== "string" || !BASE_VERSION.test(version)) continue;
    if (!Array.isArray(items)) continue;
    const texts = items.map(parseText).filter((item): item is ChangelogText => item !== null);
    if (texts.length === 0) continue;
    entries.push({
      version,
      date: typeof date === "string" && validDate(date) ? date : null,
      items: texts,
    });
  }
  return entries.sort((a, b) => compareVersions(b.version, a.version));
}

/** Text in the UI language, otherwise English. */
export function itemText(item: ChangelogText, language: Language): string {
  return item[language] ?? item.en;
}

/** "7 November 2026" in the UI language; the date has no time zone. */
export function formatReleaseDate(iso: string, language: Language): string {
  if (!validDate(iso)) return iso;
  const [y, m, d] = iso.split("-").map(Number);
  if (y === undefined || m === undefined || d === undefined) return iso;
  return new Intl.DateTimeFormat(language, { dateStyle: "long", timeZone: "UTC" }).format(
    new Date(Date.UTC(y, m - 1, d)),
  );
}

export interface EntryHeading {
  /**
   * Version for `VersionText`: for the entry of the running version its full form with build
   * number (as in the sidebar, e.g. "0.2.0+57", shown as "0.2.0-57"), otherwise X.Y.Z.
   */
  version: string;
  /** Date in the UI language; `null` as long as the version is not released. */
  date: string | null;
  /** The entry belongs to the running version (compared by X.Y.Z). */
  current: boolean;
}

/** Header line of an entry. `current` is the running version, or `null` while unknown. */
export function entryHeading(
  entry: ChangelogEntry,
  current: string | null,
  language: Language,
): EntryHeading {
  const isCurrent = current !== null && baseVersion(current) === entry.version;
  return {
    version: isCurrent ? current : entry.version,
    date: entry.date === null ? null : formatReleaseDate(entry.date, language),
    current: isCurrent,
  };
}

/** Entries newer than `lastSeen` and not newer than the running version. */
export function entriesSince(
  entries: readonly ChangelogEntry[],
  lastSeen: string,
  current: string,
): ChangelogEntry[] {
  return entries.filter(
    (entry) =>
      compareVersions(entry.version, lastSeen) > 0 && compareVersions(entry.version, current) <= 0,
  );
}

/**
 * Which version the dialog expands initially: the highest version of the whole changelog
 * (not the installed one), if it is shown. If the dialog shows only a slice without it, the
 * highest one shown is expanded, so everything is never collapsed. `null` if at most one
 * version is shown: then there is nothing to collapse.
 */
export function initiallyOpenVersion(
  all: readonly ChangelogEntry[],
  shown: readonly ChangelogEntry[],
): string | null {
  if (shown.length < 2) return null;
  const highest = (entries: readonly ChangelogEntry[]): string | null =>
    entries.reduce<string | null>(
      (best, entry) =>
        best === null || compareVersions(entry.version, best) > 0 ? entry.version : best,
      null,
    );
  const newest = highest(all);
  if (newest !== null && shown.some((entry) => entry.version === newest)) return newest;
  return highest(shown);
}

/** Expands or collapses a version; returns a new set. */
export function toggleVersion(open: ReadonlySet<string>, version: string): ReadonlySet<string> {
  const next = new Set(open);
  if (next.has(version)) next.delete(version);
  else next.add(version);
  return next;
}

export interface WhatsNewDecision {
  /** Show the dialog on its own? */
  show: boolean;
  /** Show from this version on (exclusive); `null` means all. */
  since: string | null;
  /** Remember this version as last seen (`null`: save nothing). */
  remember: string | null;
}

/**
 * Decides at startup. `lastSeen` is the saved version or `null`. `knownUser` is true if Hangar
 * already ran before this feature existed (other settings are present): then the oldest changelog
 * version counts as seen, so existing users do not miss their first update with a changelog.
 * On the very first start nothing appears; the version is only remembered.
 */
export function decideWhatsNew(
  entries: readonly ChangelogEntry[],
  currentVersion: string,
  lastSeen: string | null,
  knownUser: boolean,
): WhatsNewDecision {
  const current = baseVersion(currentVersion);
  if (current === null) return { show: false, since: null, remember: null };
  const oldest = entries[entries.length - 1]?.version ?? null;
  const seen = lastSeen !== null && baseVersion(lastSeen) !== null ? lastSeen : null;
  const reference = seen ?? (knownUser ? oldest : null);
  if (reference === null) return { show: false, since: null, remember: current };
  if (compareVersions(current, reference) <= 0) {
    // Same or older (downgrade): show nothing, keep the remembered version only when equal.
    return { show: false, since: null, remember: seen === null ? current : null };
  }
  const show = entriesSince(entries, reference, current).length > 0;
  return { show, since: reference, remember: current };
}

const LAST_SEEN_KEY = "hangar.lastSeenVersion";

export function loadLastSeen(): string | null {
  try {
    return localStorage.getItem(LAST_SEEN_KEY);
  } catch {
    return null;
  }
}

export function saveLastSeen(version: string): void {
  try {
    localStorage.setItem(LAST_SEEN_KEY, version);
  } catch {
    // Without storage, What's New does not appear on its own; the menu entry remains.
  }
}
