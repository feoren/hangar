// Adjustable layout: sidebar width, height of the profile area, collapsed state, collapsed groups,
// font size, right panel (notes and changes), split view and the sizes of its panes.
// Remembered per machine in localStorage; without storage, the defaults apply.

export const SIDEBAR_WIDTH = { min: 200, max: 560, default: 272 } as const;
/** Same value as --sidebar-rail in styles.css. */
export const SIDEBAR_RAIL_WIDTH = 52;
export const FOOTER_HEIGHT = { min: 110, default: 330 } as const;
/** At least this much room stays for the session list above the profile area. */
export const SESSION_LIST_MIN = 140;
export const FONT_SIZE = { min: 9, max: 28, default: 14 } as const;
/** Width of the right panel (notes, changes) next to the terminal. */
export const NOTES_WIDTH = { min: 240, max: 900, default: 380 } as const;

import { parsePaneRatios, type PaneRatios } from "./paneRatios";
import { migrateLayoutId, type LayoutId } from "./snapLayouts";

const STORAGE_KEY = "hangar.layout";

/** Collapsible groups of the sidebar, top to bottom. New ones are added here. */
export const SIDEBAR_GROUPS = ["sessions", "worktrees", "newSession"] as const;
export type SidebarGroup = (typeof SIDEBAR_GROUPS)[number];

export function isSidebarGroup(value: unknown): value is SidebarGroup {
  return SIDEBAR_GROUPS.some((group) => group === value);
}

export interface Layout {
  sidebarWidth: number;
  footerHeight: number;
  collapsed: boolean;
  /** Collapsed groups; only their title row remains. */
  closedGroups: SidebarGroup[];
  fontSize: number;
  /** Right panel visible next to the terminal. */
  sideOpen: boolean;
  /** Which tab the right panel shows. */
  sideTab: SideTab;
  notesWidth: number;
  /** Snap layout of the panes area (snapLayouts.ts). */
  snapLayout: LayoutId;
  /** Splitter positions per snap layout, kept when switching layouts (paneRatios.ts). */
  paneRatios: PaneRatios;
}

export const DEFAULT_LAYOUT: Layout = {
  sidebarWidth: SIDEBAR_WIDTH.default,
  footerHeight: FOOTER_HEIGHT.default,
  collapsed: false,
  closedGroups: [],
  fontSize: FONT_SIZE.default,
  sideOpen: false,
  sideTab: "notes",
  notesWidth: NOTES_WIDTH.default,
  snapLayout: "single",
  paneRatios: {},
};

export type SideTab = "notes" | "changes";

export function isGroupOpen(layout: Layout, group: SidebarGroup): boolean {
  return !layout.closedGroups.includes(group);
}

/** Expands or collapses a group; the order of the list follows SIDEBAR_GROUPS. */
export function toggleGroup(layout: Layout, group: SidebarGroup): Layout {
  const closed = isGroupOpen(layout, group)
    ? [...layout.closedGroups, group]
    : layout.closedGroups.filter((other) => other !== group);
  return { ...layout, closedGroups: SIDEBAR_GROUPS.filter((id) => closed.includes(id)) };
}

export function clamp(value: number, min: number, max: number): number {
  if (!Number.isFinite(value)) return min;
  return Math.min(Math.max(Math.round(value), min), Math.max(min, max));
}

export function clampSidebarWidth(width: number): number {
  return clamp(width, SIDEBAR_WIDTH.min, SIDEBAR_WIDTH.max);
}

/** The profile area must never push the session list out entirely. */
export function clampFooterHeight(height: number, sidebarHeight: number): number {
  return clamp(height, FOOTER_HEIGHT.min, sidebarHeight - SESSION_LIST_MIN);
}

export function clampFontSize(size: number): number {
  return clamp(size, FONT_SIZE.min, FONT_SIZE.max);
}

/** At least this much width stays for the terminal next to the notes panel. */
export const TERMINAL_MIN_WIDTH = 320;

/** `available`: width of the area right of the sidebar; the terminal keeps at least 320 px. */
export function clampNotesWidth(width: number, available = Infinity): number {
  return clamp(width, NOTES_WIDTH.min, Math.min(NOTES_WIDTH.max, available - TERMINAL_MIN_WIDTH));
}

/** Below this window width the right panel lies over the terminal, with a scrim. */
export const OVERLAY_BELOW = 900;
/** In overlay mode this much of the window stays visible left of the panel (click to close). */
export const OVERLAY_GAP = 48;

export interface NotesPlacement {
  /** `docked`: next to the terminal; `overlay`: over the window with a scrim. */
  mode: "docked" | "overlay";
  width: number;
}

/**
 * Where the right panel sits and how wide it is. `available` is the width right of the
 * sidebar, `windowWidth` the whole window. In a narrow window (below 900 px), or when terminal
 * (320 px) and panel (240 px) do not fit side by side, the panel lies over the window instead of
 * squeezing the terminal to a few columns; the remembered preferred width stays untouched.
 */
export function placeNotes(
  desired: number,
  available: number,
  windowWidth: number = Infinity,
): NotesPlacement {
  const room = Math.max(0, Math.floor(available));
  if (windowWidth >= OVERLAY_BELOW && room - TERMINAL_MIN_WIDTH >= NOTES_WIDTH.min) {
    return { mode: "docked", width: clampNotesWidth(desired, room) };
  }
  const window = Number.isFinite(windowWidth) ? Math.max(0, Math.floor(windowWidth)) : room;
  const limit = Math.max(Math.min(NOTES_WIDTH.min, window), window - OVERLAY_GAP);
  const width = Math.min(limit, clamp(desired, NOTES_WIDTH.min, NOTES_WIDTH.max));
  return { mode: "overlay", width };
}

/** Reads the stored layout and replaces missing or invalid values with defaults. */
export function parseLayout(text: string | null): Layout {
  if (!text) return DEFAULT_LAYOUT;
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return DEFAULT_LAYOUT;
  }
  if (typeof raw !== "object" || raw === null) return DEFAULT_LAYOUT;
  const number = (key: string, fallback: number): number => {
    const value: unknown = key in raw ? Reflect.get(raw, key) : undefined;
    return typeof value === "number" && Number.isFinite(value) ? value : fallback;
  };
  const collapsed: unknown = "collapsed" in raw ? raw.collapsed : undefined;
  const closed: unknown = "closedGroups" in raw ? raw.closedGroups : undefined;
  // Before 0.3 the panel only held notes and was stored as `notesOpen`.
  const sideOpen: unknown =
    "sideOpen" in raw ? raw.sideOpen : "notesOpen" in raw ? raw.notesOpen : undefined;
  const sideTab: unknown = "sideTab" in raw ? raw.sideTab : undefined;
  // Before 0.3 the layout was stored as `splitMode` ("single", "columns", "grid").
  const snapLayout =
    migrateLayoutId("snapLayout" in raw ? raw.snapLayout : undefined) ??
    migrateLayoutId("splitMode" in raw ? raw.splitMode : undefined);
  return {
    sidebarWidth: clampSidebarWidth(number("sidebarWidth", DEFAULT_LAYOUT.sidebarWidth)),
    footerHeight: Math.max(FOOTER_HEIGHT.min, number("footerHeight", DEFAULT_LAYOUT.footerHeight)),
    collapsed: typeof collapsed === "boolean" ? collapsed : DEFAULT_LAYOUT.collapsed,
    closedGroups: Array.isArray(closed)
      ? SIDEBAR_GROUPS.filter((group) => closed.includes(group))
      : DEFAULT_LAYOUT.closedGroups,
    fontSize: clampFontSize(number("fontSize", DEFAULT_LAYOUT.fontSize)),
    sideOpen: typeof sideOpen === "boolean" ? sideOpen : DEFAULT_LAYOUT.sideOpen,
    sideTab: sideTab === "changes" ? "changes" : "notes",
    notesWidth: clampNotesWidth(number("notesWidth", DEFAULT_LAYOUT.notesWidth)),
    snapLayout: snapLayout ?? DEFAULT_LAYOUT.snapLayout,
    paneRatios: parsePaneRatios("paneRatios" in raw ? raw.paneRatios : undefined),
  };
}

export function loadLayout(): Layout {
  try {
    return parseLayout(localStorage.getItem(STORAGE_KEY));
  } catch {
    return DEFAULT_LAYOUT;
  }
}

export function saveLayout(layout: Layout): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(layout));
  } catch {
    // Without storage, the default layout applies again on the next start.
  }
}
