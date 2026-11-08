// Adjustable layout: sidebar width, height of the profile area, collapsed state, collapsed groups,
// font size, notes panel.
// Remembered per machine in localStorage; without storage, the defaults apply.

export const SIDEBAR_WIDTH = { min: 200, max: 560, default: 272 } as const;
export const SIDEBAR_RAIL_WIDTH = 52;
export const FOOTER_HEIGHT = { min: 110, default: 330 } as const;
/** At least this much room stays for the session list above the profile area. */
export const SESSION_LIST_MIN = 140;
export const FONT_SIZE = { min: 9, max: 28, default: 14 } as const;
/** Width of the notes panel to the right of the terminal. */
export const NOTES_WIDTH = { min: 240, max: 900, default: 380 } as const;

const STORAGE_KEY = "hangar.layout";

/** Collapsible groups of the sidebar, top to bottom. New ones are added here. */
export const SIDEBAR_GROUPS = ["sessions", "notes", "newSession"] as const;
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
  /** Notes panel visible to the right of the terminal. */
  notesOpen: boolean;
  notesWidth: number;
}

export const DEFAULT_LAYOUT: Layout = {
  sidebarWidth: SIDEBAR_WIDTH.default,
  footerHeight: FOOTER_HEIGHT.default,
  collapsed: false,
  closedGroups: [],
  fontSize: FONT_SIZE.default,
  notesOpen: false,
  notesWidth: NOTES_WIDTH.default,
};

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

export interface NotesPlacement {
  /** `docked`: next to the terminal; `overlay`: on top of the terminal, which keeps its width. */
  mode: "docked" | "overlay";
  width: number;
}

/**
 * Where the notes panel sits and how wide it is. `available` is the width right of the sidebar. If
 * terminal (320 px) and notes (240 px) do not fit side by side, the notes lie on top of the terminal
 * instead of squeezing it to 0 px; the remembered preferred width stays untouched.
 */
export function placeNotes(desired: number, available: number): NotesPlacement {
  const room = Math.max(0, Math.floor(available));
  if (room - TERMINAL_MIN_WIDTH >= NOTES_WIDTH.min) {
    return { mode: "docked", width: clampNotesWidth(desired, room) };
  }
  const width = Math.min(room, clamp(desired, NOTES_WIDTH.min, NOTES_WIDTH.max));
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
  const notesOpen: unknown = "notesOpen" in raw ? raw.notesOpen : undefined;
  return {
    sidebarWidth: clampSidebarWidth(number("sidebarWidth", DEFAULT_LAYOUT.sidebarWidth)),
    footerHeight: Math.max(FOOTER_HEIGHT.min, number("footerHeight", DEFAULT_LAYOUT.footerHeight)),
    collapsed: typeof collapsed === "boolean" ? collapsed : DEFAULT_LAYOUT.collapsed,
    closedGroups: Array.isArray(closed)
      ? SIDEBAR_GROUPS.filter((group) => closed.includes(group))
      : DEFAULT_LAYOUT.closedGroups,
    fontSize: clampFontSize(number("fontSize", DEFAULT_LAYOUT.fontSize)),
    notesOpen: typeof notesOpen === "boolean" ? notesOpen : DEFAULT_LAYOUT.notesOpen,
    notesWidth: clampNotesWidth(number("notesWidth", DEFAULT_LAYOUT.notesWidth)),
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
