// How a session row in the sidebar lays out its facts: the row builder in the settings. The user
// places each element (title, state, folder, ...) on one of four lines, in the line's left or
// right part, in any order, and picks its form (the state as a word or a pill, the context as a
// bar or a ring, ...). An element on no line is hidden. Groups, tree guides and sub-agent rows
// stay the same whatever the layout; it changes only the inside of a row (SessionRow.tsx,
// styles.css "Row layout"). The fourteen fixed row styles of earlier versions are presets now, and
// a stored style becomes its layout once (`parseRowLayout`). Pure.

import type { ResourceParts } from "./resources";

/** Everything a row can show, in the order the editor lists hidden elements. */
export const ROW_ELEMENTS = [
  "title",
  "state",
  "program",
  "signs",
  "folder",
  "branch",
  "load",
  "runtime",
  "context",
  "tokens",
] as const;
export type RowElement = (typeof ROW_ELEMENTS)[number];

export function isRowElement(value: unknown): value is RowElement {
  return typeof value === "string" && ROW_ELEMENTS.some((element) => element === value);
}

/**
 * - `word`: the state as one word with its dot
 * - `pill`: the word in a pill tinted in the state's color
 * - `symbol`: only the state's shape (dots, bell, check, ...), no word
 * - `edge`: the word, the whole row tinted in the state's color
 * - `band`: the word in a solid pill, a thin band in the state's color on top of the row
 */
export const STATE_FORMS = ["word", "pill", "symbol", "edge", "band"] as const;
export type StateForm = (typeof STATE_FORMS)[number];

/** The context window: striped bar, small bar, ring, ten segments or only the share. */
export const CONTEXT_FORMS = ["bar", "mini", "ring", "segments", "number"] as const;
export type ContextForm = (typeof CONTEXT_FORMS)[number];

/** CPU, RAM and GPU: plain values, outlined pills or labeled tiles. */
export const LOAD_FORMS = ["text", "pills", "tiles"] as const;
export type LoadForm = (typeof LOAD_FORMS)[number];

/**
 * The program: a small icon (only where the group header does not name the program already) or
 * an avatar with the state dot on its corner (always).
 */
export const PROGRAM_FORMS = ["icon", "avatar"] as const;
export type ProgramForm = (typeof PROGRAM_FORMS)[number];

/** Folder and branch: plain text or a quiet tag. */
export const PLACE_FORMS = ["text", "tag"] as const;
export type PlaceForm = (typeof PLACE_FORMS)[number];

export interface RowForms {
  state: StateForm;
  program: ProgramForm;
  folder: PlaceForm;
  branch: PlaceForm;
  load: LoadForm;
  context: ContextForm;
}

/** The elements that come in more than one form. */
export type FormElement = keyof RowForms;

export const ELEMENT_FORMS: { readonly [K in FormElement]: readonly RowForms[K][] } = {
  state: STATE_FORMS,
  program: PROGRAM_FORMS,
  folder: PLACE_FORMS,
  branch: PLACE_FORMS,
  load: LOAD_FORMS,
  context: CONTEXT_FORMS,
};

export function hasForms(element: RowElement): element is FormElement {
  return element in ELEMENT_FORMS;
}

/** The row on the sidebar's background, or on a raised card. */
export const ROW_FRAMES = ["flat", "card"] as const;
export type RowFrame = (typeof ROW_FRAMES)[number];

/** How close the lines below the title sit. */
export const ROW_DENSITIES = ["normal", "compact"] as const;
export type RowDensity = (typeof ROW_DENSITIES)[number];

export const ROW_SIDES = ["left", "right"] as const;
export type RowSide = (typeof ROW_SIDES)[number];

/** A line's elements from its left edge, then those that sit at its right edge. */
export interface RowLine {
  left: readonly RowElement[];
  right: readonly RowElement[];
}

/** A row has this many lines; empty ones take no room. */
export const ROW_LINES = 4;

export interface RowLayout {
  /** Always `ROW_LINES` lines; each element sits on at most one of them. */
  lines: readonly RowLine[];
  /** The form of each element, kept while it is hidden. */
  forms: RowForms;
  frame: RowFrame;
  density: RowDensity;
}

const DEFAULT_FORMS: RowForms = {
  state: "word",
  program: "icon",
  folder: "text",
  branch: "text",
  load: "text",
  context: "bar",
};

function layout(
  lines: readonly (readonly [readonly RowElement[], readonly RowElement[]])[],
  forms: Partial<RowForms> = {},
  frame: RowFrame = "flat",
): RowLayout {
  const filled = Array.from({ length: ROW_LINES }, (_, index) => {
    const line = lines[index];
    return { left: line?.[0] ?? [], right: line?.[1] ?? [] };
  });
  return { lines: filled, forms: { ...DEFAULT_FORMS, ...forms }, frame, density: "normal" };
}

const TITLE_LINE = ["program", "signs", "title"] as const;
const WHERE_LINE = [["folder"], ["branch"]] as const;

/**
 * The long-standing row: title and state; folder and branch; the load; run time, context bar
 * and tokens.
 */
export const DEFAULT_ROW_LAYOUT: RowLayout = layout([
  [TITLE_LINE, ["state"]],
  WHERE_LINE,
  [["load"], []],
  [["runtime", "context"], ["tokens"]],
]);

/** The row styles of earlier versions, now starting points for the builder. */
export const ROW_PRESETS = [
  "hangar",
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
export type RowPreset = (typeof ROW_PRESETS)[number];

export function isRowPreset(value: unknown): value is RowPreset {
  return typeof value === "string" && ROW_PRESETS.some((preset) => preset === value);
}

const STANDARD_LINES = DEFAULT_ROW_LAYOUT.lines.map((line) => [line.left, line.right] as const);

/**
 * Each preset as a layout. Three cannot be built exactly from elements and come as the closest
 * layout: "accordion" (one line; no chevron, the peek shows the rest), "counters" (the state
 * symbol before the title and the run time at its end; no counts on the group header) and
 * "columns" (share and run time at the title line's end; no column names above the list).
 */
const PRESETS: Record<RowPreset, RowLayout> = {
  // The row the owner built in 0.8.0: state as a pill beside the title, branch and run time,
  // load as pills with the tokens, the context in segments at the bottom.
  hangar: layout(
    [
      [["title", "signs"], ["state"]],
      [["folder"], ["branch", "runtime"]],
      [["load"], ["tokens"]],
      [["context"], []],
    ],
    { state: "pill", program: "avatar", load: "pills", context: "segments" },
  ),
  standard: DEFAULT_ROW_LAYOUT,
  pill: layout(STANDARD_LINES, { state: "pill" }),
  avatar: layout(STANDARD_LINES, { program: "avatar" }),
  accordion: layout([[TITLE_LINE, ["context", "state"]]], { context: "mini" }),
  tags: layout(STANDARD_LINES, { folder: "tag", branch: "tag", load: "pills" }),
  ring: layout(
    [[["context", "program", "signs", "title"], ["state"]], WHERE_LINE, [["load"], ["runtime"]]],
    { context: "ring" },
  ),
  tiles: layout(STANDARD_LINES, { load: "tiles" }),
  edge: layout(STANDARD_LINES, { state: "edge" }),
  band: layout(STANDARD_LINES, { state: "band" }),
  segments: layout(STANDARD_LINES, { context: "segments" }),
  compact: layout(
    [
      [TITLE_LINE, ["state"]],
      [["folder"], ["branch", "context"]],
    ],
    {
      context: "mini",
    },
  ),
  counters: layout(
    [
      [["program", "signs", "state", "title"], ["runtime"]],
      WHERE_LINE,
      [["load"], []],
      [["context"], ["tokens"]],
    ],
    { state: "symbol" },
  ),
  columns: layout(
    [
      [
        ["program", "signs", "state", "title"],
        ["context", "runtime"],
      ],
      WHERE_LINE,
    ],
    { state: "symbol", context: "number" },
  ),
  card: layout(
    [
      [
        ["program", "signs", "title"],
        ["state", "context"],
      ],
      WHERE_LINE,
      [["load"], ["runtime"]],
    ],
    { state: "pill", program: "avatar", context: "ring" },
    "card",
  ),
};

export function presetLayout(preset: RowPreset): RowLayout {
  return PRESETS[preset];
}

/** The preset a layout equals, for the editor's "load template" choice; `null` for one's own. */
export function matchingPreset(value: RowLayout): RowPreset | null {
  return ROW_PRESETS.find((preset) => sameLayout(PRESETS[preset], value)) ?? null;
}

export function sameLayout(a: RowLayout, b: RowLayout): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** More entries than a slot could ever hold are not even looked at. */
const MAX_SLOT_ENTRIES = 32;

function pick<T extends string>(options: readonly T[], value: unknown, fallback: T): T {
  return options.find((option) => option === value) ?? fallback;
}

function field(value: object, key: string): unknown {
  return key in value ? Reflect.get(value, key) : undefined;
}

/**
 * The stored layout, checked strictly: unknown elements are dropped, each element shows at most
 * once (its first place wins), lines beyond four are dropped and missing ones are empty, unknown
 * forms fall back to the default. The title always shows: a layout without it gets it at the end
 * of the first line's left part. Without a stored layout, a row style of an earlier version
 * (`legacyStyle`) becomes its preset; anything else gives the standard row.
 */
export function parseRowLayout(value: unknown, legacyStyle?: unknown): RowLayout {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return isRowPreset(legacyStyle) ? presetLayout(legacyStyle) : DEFAULT_ROW_LAYOUT;
  }
  const seen = new Set<RowElement>();
  const slot = (entries: unknown): RowElement[] => {
    if (!Array.isArray(entries)) return [];
    const kept: RowElement[] = [];
    for (const entry of entries.slice(0, MAX_SLOT_ENTRIES)) {
      if (!isRowElement(entry) || seen.has(entry)) continue;
      seen.add(entry);
      kept.push(entry);
    }
    return kept;
  };
  const rawLines = field(value, "lines");
  const stored = Array.isArray(rawLines) ? rawLines.slice(0, ROW_LINES) : [];
  const lines: RowLine[] = Array.from({ length: ROW_LINES }, (_, index) => {
    const line: unknown = stored[index];
    if (typeof line !== "object" || line === null) return { left: [], right: [] };
    return { left: slot(field(line, "left")), right: slot(field(line, "right")) };
  });
  const first = lines[0];
  if (!seen.has("title") && first !== undefined) {
    lines[0] = { left: [...first.left, "title"], right: first.right };
  }
  const rawForms = field(value, "forms");
  const forms: object = typeof rawForms === "object" && rawForms !== null ? rawForms : {};
  return {
    lines,
    forms: {
      state: pick(STATE_FORMS, field(forms, "state"), DEFAULT_FORMS.state),
      program: pick(PROGRAM_FORMS, field(forms, "program"), DEFAULT_FORMS.program),
      folder: pick(PLACE_FORMS, field(forms, "folder"), DEFAULT_FORMS.folder),
      branch: pick(PLACE_FORMS, field(forms, "branch"), DEFAULT_FORMS.branch),
      load: pick(LOAD_FORMS, field(forms, "load"), DEFAULT_FORMS.load),
      context: pick(CONTEXT_FORMS, field(forms, "context"), DEFAULT_FORMS.context),
    },
    frame: pick(ROW_FRAMES, field(value, "frame"), "flat"),
    density: pick(ROW_DENSITIES, field(value, "density"), "normal"),
  };
}

/** Where an element sits: its line, the line's part and its place in that part. */
export interface RowSpot {
  line: number;
  side: RowSide;
  index: number;
}

export function findElement(value: RowLayout, element: RowElement): RowSpot | null {
  for (const [line, entry] of value.lines.entries()) {
    for (const side of ROW_SIDES) {
      const index = entry[side].indexOf(element);
      if (index >= 0) return { line, side, index };
    }
  }
  return null;
}

export function isPlaced(value: RowLayout, element: RowElement): boolean {
  return findElement(value, element) !== null;
}

/** The elements on no line, in the editor's order. */
export function hiddenElements(value: RowLayout): RowElement[] {
  return ROW_ELEMENTS.filter((element) => !isPlaced(value, element));
}

function withoutElement(value: RowLayout, element: RowElement): RowLine[] {
  return value.lines.map((line) => ({
    left: line.left.filter((entry) => entry !== element),
    right: line.right.filter((entry) => entry !== element),
  }));
}

/**
 * Puts `element` at `spot`, from wherever it was (or from the hidden ones). `spot.index` counts
 * the part's elements as they are now, so "before the element at index 2" stays that, also when
 * the element moves within its own part. Out of range places are clamped.
 */
export function placeElement(value: RowLayout, element: RowElement, spot: RowSpot): RowLayout {
  const lineIndex = Math.min(Math.max(Math.trunc(spot.line), 0), ROW_LINES - 1);
  const target = value.lines[lineIndex]?.[spot.side] ?? [];
  let index = Math.min(Math.max(Math.trunc(spot.index), 0), target.length);
  const from = findElement(value, element);
  if (from !== null && from.line === lineIndex && from.side === spot.side && from.index < index) {
    index -= 1;
  }
  const lines = withoutElement(value, element);
  const line = lines[lineIndex];
  if (line === undefined) return value;
  const entries = [...line[spot.side]];
  entries.splice(index, 0, element);
  lines[lineIndex] = { ...line, [spot.side]: entries };
  return { ...value, lines };
}

/** Takes an element off the row; the title always stays. */
export function hideElement(value: RowLayout, element: RowElement): RowLayout {
  if (element === "title" || !isPlaced(value, element)) return value;
  return { ...value, lines: withoutElement(value, element) };
}

/** Shows a hidden element where the standard row has it, at the end of that part. */
export function showElement(value: RowLayout, element: RowElement): RowLayout {
  if (isPlaced(value, element)) return value;
  const home = findElement(DEFAULT_ROW_LAYOUT, element) ?? { line: 0, side: "left", index: 0 };
  const length = value.lines[home.line]?.[home.side].length ?? 0;
  return placeElement(value, element, { ...home, index: length });
}

export type RowStep = "left" | "right" | "up" | "down";

/**
 * One step for the editor's arrow buttons. Left and right walk the line as one sequence: past
 * the left part's end into the right part and back. Up and down move to the same part of the
 * line above or below, at the same place where it has one. At an edge nothing changes.
 */
export function stepElement(value: RowLayout, element: RowElement, step: RowStep): RowLayout {
  const from = findElement(value, element);
  if (from === null) return value;
  const line = value.lines[from.line];
  if (line === undefined) return value;
  const length = line[from.side].length;
  switch (step) {
    case "left":
      if (from.index > 0) return placeElement(value, element, { ...from, index: from.index - 1 });
      if (from.side === "right") {
        return placeElement(value, element, {
          line: from.line,
          side: "left",
          index: line.left.length,
        });
      }
      return value;
    case "right":
      if (from.index < length - 1) {
        // "Before the element after the next one" in the part as it is now.
        return placeElement(value, element, { ...from, index: from.index + 2 });
      }
      if (from.side === "left") {
        return placeElement(value, element, { line: from.line, side: "right", index: 0 });
      }
      return value;
    case "up":
    case "down": {
      const target = from.line + (step === "up" ? -1 : 1);
      if (target < 0 || target >= ROW_LINES) return value;
      return placeElement(value, element, { line: target, side: from.side, index: from.index });
    }
  }
}

export function setForm<K extends FormElement>(
  value: RowLayout,
  element: K,
  form: RowForms[K],
): RowLayout {
  return { ...value, forms: { ...value.forms, [element]: form } };
}

/** A form chosen by its name (the editor's picker); an unknown name changes nothing. */
export function setFormByName(value: RowLayout, element: FormElement, form: string): RowLayout {
  switch (element) {
    case "state":
      return setForm(value, element, pick(STATE_FORMS, form, value.forms.state));
    case "program":
      return setForm(value, element, pick(PROGRAM_FORMS, form, value.forms.program));
    case "folder":
    case "branch":
      return setForm(value, element, pick(PLACE_FORMS, form, value.forms[element]));
    case "load":
      return setForm(value, element, pick(LOAD_FORMS, form, value.forms.load));
    case "context":
      return setForm(value, element, pick(CONTEXT_FORMS, form, value.forms.context));
  }
}

/** One line as the row draws it. */
export interface PlannedLine {
  left: readonly RowElement[];
  right: readonly RowElement[];
  /** The title sits in its left part: what stands before it moves behind it for screen readers. */
  titleLeft: boolean;
  /** Holds the title. */
  title: boolean;
  /** Beside an avatar or ring that spans the first two lines: 1 or 2; `null` across the row. */
  beside: 1 | 2 | null;
}

/** A layout made ready for drawing, once per layout rather than per row and render. */
export interface RowPlan {
  layout: RowLayout;
  /** The lines that hold anything, without the parts that span them. */
  lines: readonly PlannedLine[];
  /** An avatar or ring at the first line's start, beside the first two lines. */
  lead: "avatar" | "ring" | null;
  /** An avatar or ring at the first line's end, beside the first two lines. */
  trail: "avatar" | "ring" | null;
  /** The state's form; `null` while it is hidden. */
  state: StateForm | null;
  /** The program shows as an avatar, whose corner dot says the state. */
  avatar: boolean;
  /** The peek spells out the load the row leaves out. */
  peekLoad: boolean;
  /** The peek spells out the context the row leaves out (or shows without its tokens). */
  peekContext: boolean;
}

function spanning(value: RowLayout, element: RowElement | undefined): "avatar" | "ring" | null {
  if (element === "program" && value.forms.program === "avatar") return "avatar";
  if (element === "context" && value.forms.context === "ring") return "ring";
  return null;
}

export function planRow(value: RowLayout): RowPlan {
  const filled = value.lines.filter((line) => line.left.length + line.right.length > 0);
  const first = filled[0];
  const lead = spanning(value, first?.left[0]);
  const trail = spanning(value, first?.right[first.right.length - 1]);
  const spans = lead !== null || trail !== null;
  const lines = filled.map((line, index): PlannedLine => {
    const left = index === 0 && lead !== null ? line.left.slice(1) : line.left;
    const right = index === 0 && trail !== null ? line.right.slice(0, -1) : line.right;
    return {
      left,
      right,
      titleLeft: left.includes("title"),
      title: left.includes("title") || right.includes("title"),
      beside: spans && index < 2 ? (index === 0 ? 1 : 2) : null,
    };
  });
  return {
    layout: value,
    lines,
    lead,
    trail,
    state: isPlaced(value, "state") ? value.forms.state : null,
    avatar: isPlaced(value, "program") && value.forms.program === "avatar",
    peekLoad: !isPlaced(value, "load"),
    peekContext: !isPlaced(value, "context") || !isPlaced(value, "tokens"),
  };
}

/**
 * The row-wide looks as attributes on the list (and the settings' preview), for styles.css:
 * the state's tint or band, the card and the density.
 */
export function rowAttributes(plan: RowPlan): Record<string, string> {
  return {
    "data-row-state": plan.state ?? "none",
    "data-row-frame": plan.layout.frame,
    "data-row-density": plan.layout.density,
  };
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
