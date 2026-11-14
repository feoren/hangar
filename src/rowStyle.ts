// How a session row in the sidebar lays out its facts: the row style, chosen in the settings.
// Groups, tree guides and sub-agent rows stay the same in every style; a style changes only the
// inside of a row (and, for "counters", the small counts on each group header). Most styles are
// pure CSS on the same markup (styles.css, "Row styles", keyed on `data-rows`); the few that need
// more markup (an avatar, a ring, a chevron, segments, columns) read it from `rowParts`. Pure.

import type { ResourceParts } from "./resources";
import type { ChipKind } from "./sessionChip";

/**
 * - `standard`: four lines, the state as one word (default)
 * - `pill`: the state word in a tinted pill with its dot
 * - `avatar`: the program's icon at the start, the state dot on its corner
 * - `accordion`: one line; the active row (or a chevron click) shows the other three
 * - `tags`: folder and branch as quiet tags, the load as outlined pills
 * - `ring`: the context as a ring at the start, three lines
 * - `tiles`: CPU, RAM and GPU as three labeled tiles
 * - `edge`: the row tinted in its state's color
 * - `band`: a thin band in the state's color on top, the state in a solid pill
 * - `segments`: the context as ten still segments
 * - `compact`: two lines; load and tokens only in the peek
 * - `counters`: no state word; the group header counts the states, the run time takes its place
 * - `columns`: dense rows with aligned columns, named once above the list
 * - `card`: avatar, pill and ring together on a raised card
 */
export const ROW_STYLES = [
  "standard",
  "pill",
  "avatar",
  "accordion",
  "tags",
  "ring",
  "tiles",
  "edge",
  "band",
  "segments",
  "compact",
  "counters",
  "columns",
  "card",
] as const;
export type RowStyle = (typeof ROW_STYLES)[number];

export const DEFAULT_ROW_STYLE: RowStyle = "standard";

export function isRowStyle(value: unknown): value is RowStyle {
  return typeof value === "string" && ROW_STYLES.some((style) => style === value);
}

/** Stored value, or the default for anything unknown (also settings from before the choice). */
export function parseRowStyle(value: unknown): RowStyle {
  return isRowStyle(value) ? value : DEFAULT_ROW_STYLE;
}

/** The markup a style needs beyond the standard row's. */
export interface RowParts {
  /** The program's badge at the row's start: with the state dot on its corner, or plain. */
  avatar: "dot" | "plain" | null;
  /** The context as a ring, at the start or the end of the row. */
  ring: "start" | "end" | null;
  /** Lines 2 to 4 fold behind a chevron. */
  expander: boolean;
  /** How the fourth line shows the context; "none" keeps only the words for screen readers. */
  meter: "bar" | "segments" | "none";
  /** A small context bar on the title line or on the folder line. */
  mini: "title" | "meta" | null;
  /** The load as the third line, as tiles, or not on the row at all. */
  load: "line" | "tiles" | "none";
  /** Where the run time shows. */
  runtime: "context" | "load" | "title" | "columns" | "none";
  /** What sits at the title line's end: the state word, the run time, or the columns. */
  end: "word" | "runtime" | "columns";
  /** Each group header counts its sessions per state. */
  counters: boolean;
  /** The peek spells out the load and the context the row leaves out. */
  peekFacts: boolean;
}

const STANDARD: RowParts = {
  avatar: null,
  ring: null,
  expander: false,
  meter: "bar",
  mini: null,
  load: "line",
  runtime: "context",
  end: "word",
  counters: false,
  peekFacts: false,
};

const PARTS: Record<RowStyle, RowParts> = {
  standard: STANDARD,
  pill: STANDARD,
  avatar: { ...STANDARD, avatar: "dot" },
  accordion: { ...STANDARD, expander: true, mini: "title" },
  tags: STANDARD,
  ring: { ...STANDARD, ring: "start", meter: "none", runtime: "load" },
  tiles: { ...STANDARD, load: "tiles" },
  edge: STANDARD,
  band: STANDARD,
  segments: { ...STANDARD, meter: "segments" },
  compact: {
    ...STANDARD,
    meter: "none",
    mini: "meta",
    load: "none",
    runtime: "none",
    peekFacts: true,
  },
  counters: { ...STANDARD, runtime: "title", end: "runtime", counters: true },
  columns: {
    ...STANDARD,
    meter: "none",
    load: "none",
    runtime: "columns",
    end: "columns",
    peekFacts: true,
  },
  card: { ...STANDARD, avatar: "plain", ring: "end", meter: "none", runtime: "load" },
};

export function rowParts(style: RowStyle): RowParts {
  return PARTS[style];
}

/** The order in which a group header counts its states: what needs you after what works. */
export const COUNTED_CHIPS: readonly ChipKind[] = [
  "working",
  "asks",
  "done",
  "failed",
  "paused",
  "ended",
];

export interface StateCount {
  kind: ChipKind;
  count: number;
}

/** How many sessions of a group are in each state; states without a session are left out. */
export function stateCounts(chips: readonly (ChipKind | null)[]): StateCount[] {
  return COUNTED_CHIPS.map((kind) => ({
    kind,
    count: chips.filter((chip) => chip === kind).length,
  })).filter((entry) => entry.count > 0);
}

/**
 * Whether a folding row shows its details: the row on screen does, unless it was folded by
 * hand; any other row only once it was unfolded by hand.
 */
export function rowExpanded(
  overrides: ReadonlyMap<number, boolean>,
  id: number,
  active: boolean,
): boolean {
  return overrides.get(id) ?? active;
}

/** The overrides after a chevron click on a row. */
export function toggleRow(
  overrides: ReadonlyMap<number, boolean>,
  id: number,
  active: boolean,
): Map<number, boolean> {
  const next = new Map(overrides);
  next.set(id, !rowExpanded(overrides, id, active));
  return next;
}

/** Names of the load's parts in the UI language. */
export interface LoadNames {
  cpu: string;
  memory: string;
  gpu: string;
}

/** "CPU 12 % · RAM 476 MB · GPU 30 % · 1.2 GB" for the peek of a row that leaves the load out. */
export function loadSummary(usage: ResourceParts, names: LoadNames): string {
  return [
    usage.cpu === null ? null : `${names.cpu} ${usage.cpu}`,
    `${names.memory} ${usage.memory}`,
    usage.gpu === null ? null : `${names.gpu} ${usage.gpu}`,
  ]
    .filter((part) => part !== null)
    .join(" · ");
}
