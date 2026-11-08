import { describe, expect, it } from "vitest";

import {
  clampFontSize,
  clampFooterHeight,
  clampNotesWidth,
  clampSidebarWidth,
  DEFAULT_LAYOUT,
  FOOTER_HEIGHT,
  isGroupOpen,
  NOTES_WIDTH,
  parseLayout,
  placeNotes,
  SESSION_LIST_MIN,
  SIDEBAR_WIDTH,
  TERMINAL_MIN_WIDTH,
  toggleGroup,
} from "./layout";

describe("limits", () => {
  it("sidebar width stays within the allowed range", () => {
    expect(clampSidebarWidth(50)).toBe(SIDEBAR_WIDTH.min);
    expect(clampSidebarWidth(5000)).toBe(SIDEBAR_WIDTH.max);
    expect(clampSidebarWidth(300.6)).toBe(301);
    expect(clampSidebarWidth(Number.NaN)).toBe(SIDEBAR_WIDTH.min);
  });

  it("profile area leaves room for the session list", () => {
    expect(clampFooterHeight(900, 800)).toBe(800 - SESSION_LIST_MIN);
    expect(clampFooterHeight(10, 800)).toBe(FOOTER_HEIGHT.min);
    // Very small window: the minimum wins over the maximum.
    expect(clampFooterHeight(300, 100)).toBe(FOOTER_HEIGHT.min);
  });

  it("font size stays within the allowed range", () => {
    expect(clampFontSize(2)).toBe(9);
    expect(clampFontSize(99)).toBe(28);
    expect(clampFontSize(15)).toBe(15);
  });

  it("notes panel leaves room for the terminal", () => {
    expect(clampNotesWidth(100)).toBe(NOTES_WIDTH.min);
    expect(clampNotesWidth(5000)).toBe(NOTES_WIDTH.max);
    expect(clampNotesWidth(600, 800)).toBe(480);
    // Very narrow window: the minimum wins.
    expect(clampNotesWidth(600, 300)).toBe(NOTES_WIDTH.min);
  });
});

describe("placeNotes", () => {
  it("docks next to the terminal while both minimum widths fit", () => {
    expect(placeNotes(380, 1200)).toEqual({ mode: "docked", width: 380 });
    // Preferred width too wide: the terminal keeps 320 px.
    expect(placeNotes(900, 800)).toEqual({ mode: "docked", width: 800 - TERMINAL_MIN_WIDTH });
    expect(placeNotes(380, TERMINAL_MIN_WIDTH + NOTES_WIDTH.min)).toEqual({
      mode: "docked",
      width: NOTES_WIDTH.min,
    });
  });

  it("overlays the terminal when both do not fit side by side", () => {
    const tight = TERMINAL_MIN_WIDTH + NOTES_WIDTH.min - 1;
    expect(placeNotes(380, tight)).toEqual({ mode: "overlay", width: 380 });
    expect(placeNotes(380, 300)).toEqual({ mode: "overlay", width: 300 });
    expect(placeNotes(100, 500)).toEqual({ mode: "overlay", width: NOTES_WIDTH.min });
  });

  it("tolerates nonsensical widths", () => {
    expect(placeNotes(380, -50)).toEqual({ mode: "overlay", width: 0 });
    expect(placeNotes(Number.NaN, 1000).mode).toBe("docked");
  });
});

describe("parseLayout", () => {
  it("returns defaults for missing or broken storage", () => {
    expect(parseLayout(null)).toEqual(DEFAULT_LAYOUT);
    expect(parseLayout("{broken")).toEqual(DEFAULT_LAYOUT);
    expect(parseLayout("42")).toEqual(DEFAULT_LAYOUT);
    expect(parseLayout("null")).toEqual(DEFAULT_LAYOUT);
  });

  it("takes valid values and clamps them", () => {
    const layout = parseLayout(
      JSON.stringify({ sidebarWidth: 9999, footerHeight: 250, collapsed: true, fontSize: 16 }),
    );
    expect(layout).toEqual({
      sidebarWidth: SIDEBAR_WIDTH.max,
      footerHeight: 250,
      collapsed: true,
      closedGroups: [],
      fontSize: 16,
      notesOpen: false,
      notesWidth: NOTES_WIDTH.default,
      snapLayout: "single",
    });
  });

  it("replaces wrong types individually", () => {
    const layout = parseLayout(
      JSON.stringify({ sidebarWidth: "wide", collapsed: "yes", notesOpen: 1, notesWidth: "x" }),
    );
    expect(layout.sidebarWidth).toBe(DEFAULT_LAYOUT.sidebarWidth);
    expect(layout.collapsed).toBe(false);
    expect(layout.notesOpen).toBe(false);
    expect(layout.notesWidth).toBe(NOTES_WIDTH.default);
  });

  it("remembers the notes panel", () => {
    const layout = parseLayout(JSON.stringify({ notesOpen: true, notesWidth: 520 }));
    expect(layout.notesOpen).toBe(true);
    expect(layout.notesWidth).toBe(520);
  });

  it("remembers the snap layout and ignores unknown ones", () => {
    expect(parseLayout(JSON.stringify({ snapLayout: "mainStack" })).snapLayout).toBe("mainStack");
    expect(parseLayout(JSON.stringify({ snapLayout: "triple" })).snapLayout).toBe("triple");
    expect(parseLayout(JSON.stringify({ snapLayout: "columns3" })).snapLayout).toBe("single");
    expect(parseLayout(JSON.stringify({ snapLayout: 2 })).snapLayout).toBe("single");
    expect(parseLayout(JSON.stringify({})).snapLayout).toBe("single");
  });

  it("migrates the split mode stored before 0.3", () => {
    expect(parseLayout(JSON.stringify({ splitMode: "single" })).snapLayout).toBe("single");
    expect(parseLayout(JSON.stringify({ splitMode: "columns" })).snapLayout).toBe("pair");
    expect(parseLayout(JSON.stringify({ splitMode: "grid" })).snapLayout).toBe("quad");
    expect(parseLayout(JSON.stringify({ splitMode: "quad" })).snapLayout).toBe("quad");
    expect(parseLayout(JSON.stringify({ splitMode: "rows" })).snapLayout).toBe("single");
    // A stored layout wins over the old field.
    expect(
      parseLayout(JSON.stringify({ splitMode: "grid", snapLayout: "triple" })).snapLayout,
    ).toBe("triple");
    expect("splitMode" in parseLayout(JSON.stringify({ splitMode: "grid" }))).toBe(false);
  });
});

describe("sidebar groups", () => {
  it("are all open initially", () => {
    expect(isGroupOpen(DEFAULT_LAYOUT, "sessions")).toBe(true);
    expect(isGroupOpen(DEFAULT_LAYOUT, "newSession")).toBe(true);
  });

  it("expand and collapse in a fixed order", () => {
    const one = toggleGroup(DEFAULT_LAYOUT, "newSession");
    expect(isGroupOpen(one, "newSession")).toBe(false);
    const both = toggleGroup(one, "sessions");
    expect(both.closedGroups).toEqual(["sessions", "newSession"]);
    const all = toggleGroup(both, "notes");
    expect(all.closedGroups).toEqual(["sessions", "notes", "newSession"]);
    expect(toggleGroup(both, "newSession").closedGroups).toEqual(["sessions"]);
  });

  it("are read robustly: unknown and duplicate entries are dropped", () => {
    const read = (closedGroups: unknown) =>
      parseLayout(JSON.stringify({ closedGroups })).closedGroups;
    expect(read(["newSession", "memos", "newSession", 3])).toEqual(["newSession"]);
    expect(read(["newSession", "notes"])).toEqual(["notes", "newSession"]);
    expect(read("sessions")).toEqual([]);
    expect(read(null)).toEqual([]);
    expect(parseLayout(JSON.stringify({ fontSize: 12 })).closedGroups).toEqual([]);
  });
});
