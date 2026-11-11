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
  OVERLAY_BELOW,
  OVERLAY_GAP,
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
    expect(placeNotes(380, 1200, 1472)).toEqual({ mode: "docked", width: 380 });
    // Preferred width too wide: the terminal keeps 320 px.
    expect(placeNotes(900, 800, 1072)).toEqual({ mode: "docked", width: 800 - TERMINAL_MIN_WIDTH });
    expect(placeNotes(380, TERMINAL_MIN_WIDTH + NOTES_WIDTH.min, 1000)).toEqual({
      mode: "docked",
      width: NOTES_WIDTH.min,
    });
  });

  it("lies over the window below 900 px or when both do not fit", () => {
    expect(placeNotes(380, 628, OVERLAY_BELOW - 1)).toEqual({ mode: "overlay", width: 380 });
    const tight = TERMINAL_MIN_WIDTH + NOTES_WIDTH.min - 1;
    expect(placeNotes(380, tight, 1200)).toEqual({ mode: "overlay", width: 380 });
    expect(placeNotes(380, 200, 300)).toEqual({ mode: "overlay", width: 300 - OVERLAY_GAP });
    expect(placeNotes(100, 500, 720)).toEqual({ mode: "overlay", width: NOTES_WIDTH.min });
  });

  it("tolerates nonsensical widths", () => {
    expect(placeNotes(380, -50, 0)).toEqual({ mode: "overlay", width: 0 });
    expect(placeNotes(Number.NaN, 1000, 1300).mode).toBe("docked");
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
      sideOpen: false,
      sideTab: "notes",
      notesWidth: NOTES_WIDTH.default,
      snapLayout: "single",
    });
  });

  it("replaces wrong types individually", () => {
    const layout = parseLayout(
      JSON.stringify({ sidebarWidth: "wide", collapsed: "yes", sideOpen: 1, notesWidth: "x" }),
    );
    expect(layout.sidebarWidth).toBe(DEFAULT_LAYOUT.sidebarWidth);
    expect(layout.collapsed).toBe(false);
    expect(layout.sideOpen).toBe(false);
    expect(layout.notesWidth).toBe(NOTES_WIDTH.default);
  });

  it("remembers the right panel and reads the older notes flag", () => {
    const layout = parseLayout(JSON.stringify({ notesOpen: true, notesWidth: 520 }));
    expect(layout.sideOpen).toBe(true);
    expect(layout.sideTab).toBe("notes");
    expect(layout.notesWidth).toBe(520);
    const changes = parseLayout(JSON.stringify({ sideOpen: true, sideTab: "changes" }));
    expect(changes.sideTab).toBe("changes");
    expect(parseLayout(JSON.stringify({ sideTab: "files" })).sideTab).toBe("files");
    expect(parseLayout(JSON.stringify({ sideTab: "tree" })).sideTab).toBe("notes");
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
    expect(toggleGroup(both, "newSession").closedGroups).toEqual(["sessions"]);
  });

  it("are read robustly: unknown and duplicate entries are dropped", () => {
    const read = (closedGroups: unknown) =>
      parseLayout(JSON.stringify({ closedGroups })).closedGroups;
    expect(read(["newSession", "memos", "newSession", 3])).toEqual(["newSession"]);
    // The notes group of 0.2 is gone: the right panel is the one place for notes.
    expect(read(["newSession", "notes", "sessions"])).toEqual(["sessions", "newSession"]);
    // The team group of 0.3 moved into the Team window.
    expect(read(["team", "sessions"])).toEqual(["sessions"]);
    expect(read("sessions")).toEqual([]);
    expect(read(null)).toEqual([]);
    expect(parseLayout(JSON.stringify({ fontSize: 12 })).closedGroups).toEqual([]);
  });

  it("keep the worktree group between sessions and new session", () => {
    const closed = toggleGroup(toggleGroup(DEFAULT_LAYOUT, "newSession"), "worktrees");
    expect(closed.closedGroups).toEqual(["worktrees", "newSession"]);
    expect(
      parseLayout(JSON.stringify({ closedGroups: ["newSession", "worktrees"] })).closedGroups,
    ).toEqual(["worktrees", "newSession"]);
  });

  it("load a layout saved before the worktree group existed: it starts open", () => {
    const old = parseLayout(
      JSON.stringify({ sidebarWidth: 300, closedGroups: ["sessions", "newSession", "team"] }),
    );
    expect(old.sidebarWidth).toBe(300);
    expect(old.closedGroups).toEqual(["sessions", "newSession"]);
    expect(isGroupOpen(old, "worktrees")).toBe(true);
  });
});
