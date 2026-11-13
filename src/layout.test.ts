import { describe, expect, it } from "vitest";

import {
  clampFontSize,
  CLOSED_AGENT_GROUPS_MAX,
  clampFooterHeight,
  clampNotesWidth,
  clampSidebarWidth,
  DEFAULT_LAYOUT,
  FOOTER_HEIGHT,
  isGroupOpen,
  openGroup,
  NOTES_WIDTH,
  OVERLAY_BELOW,
  OVERLAY_GAP,
  parseLayout,
  placeNotes,
  SESSION_LIST_MIN,
  SIDEBAR_WIDTH,
  TERMINAL_MIN_WIDTH,
  toggleAgentGroup,
  toggleGroup,
} from "./layout";

describe("limits", () => {
  it("sidebar width stays within the allowed range", () => {
    expect(SIDEBAR_WIDTH.min).toBe(305);
    expect(SIDEBAR_WIDTH.max).toBe(505);
    expect(clampSidebarWidth(50)).toBe(305);
    expect(clampSidebarWidth(304)).toBe(305);
    expect(clampSidebarWidth(5000)).toBe(505);
    expect(clampSidebarWidth(320.6)).toBe(321);
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
    expect(NOTES_WIDTH.min).toBe(305);
    expect(NOTES_WIDTH.max).toBe(505);
    expect(clampNotesWidth(100)).toBe(305);
    expect(clampNotesWidth(5000)).toBe(505);
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
    expect(placeNotes(380, 200, 400)).toEqual({ mode: "overlay", width: 400 - OVERLAY_GAP });
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
      closedAgentGroups: [],
      fontSize: 16,
      sideOpen: false,
      sideTab: "notes",
      notesWidth: NOTES_WIDTH.default,
      snapLayout: "single",
      paneRatios: {},
      subAgentRatios: null,
    });
  });

  it("clamps remembered widths from before the 305 to 505 px range", () => {
    const narrow = parseLayout(JSON.stringify({ sidebarWidth: 220, notesWidth: 240 }));
    expect(narrow.sidebarWidth).toBe(305);
    expect(narrow.notesWidth).toBe(305);
    const wide = parseLayout(JSON.stringify({ sidebarWidth: 560, notesWidth: 900 }));
    expect(wide.sidebarWidth).toBe(505);
    expect(wide.notesWidth).toBe(505);
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
    const layout = parseLayout(JSON.stringify({ notesOpen: true, notesWidth: 420 }));
    expect(layout.sideOpen).toBe(true);
    expect(layout.sideTab).toBe("notes");
    expect(layout.notesWidth).toBe(420);
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

  it("remembers the pane sizes per snap layout and drops invalid ones", () => {
    const layout = parseLayout(
      JSON.stringify({
        snapLayout: "pair",
        paneRatios: { pair: [0.7], quad: [0.4, "x"], triple: [0.6, 0.5], mainStack: [0.5, 0.3] },
      }),
    );
    expect(layout.paneRatios).toEqual({ pair: [0.7], mainStack: [0.5, 0.3] });
    expect(parseLayout(JSON.stringify({ paneRatios: "wide" })).paneRatios).toEqual({});
    expect(parseLayout(JSON.stringify({})).paneRatios).toEqual({});
  });

  it("remembers the sub-agent view's width apart from the split view", () => {
    const layout = parseLayout(
      JSON.stringify({ paneRatios: { pair: [0.4] }, subAgentRatios: [0.62] }),
    );
    expect(layout.subAgentRatios).toEqual([0.62]);
    expect(layout.paneRatios).toEqual({ pair: [0.4] });
    expect(parseLayout(JSON.stringify({ subAgentRatios: [1.2] })).subAgentRatios).toBeNull();
    expect(parseLayout(JSON.stringify({})).subAgentRatios).toBeNull();
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
    expect(isGroupOpen(DEFAULT_LAYOUT, "newSession")).toBe(true);
    expect(DEFAULT_LAYOUT.closedAgentGroups).toEqual([]);
  });

  it("expand and collapse", () => {
    const one = toggleGroup(DEFAULT_LAYOUT, "newSession");
    expect(isGroupOpen(one, "newSession")).toBe(false);
    expect(one.closedGroups).toEqual(["newSession"]);
    expect(toggleGroup(one, "newSession").closedGroups).toEqual([]);
  });

  it("are read robustly: unknown and duplicate entries are dropped", () => {
    const read = (closedGroups: unknown) =>
      parseLayout(JSON.stringify({ closedGroups })).closedGroups;
    expect(read(["newSession", "memos", "newSession", 3])).toEqual(["newSession"]);
    // The notes group of 0.2 is gone: the right panel is the one place for notes.
    expect(read(["newSession", "notes"])).toEqual(["newSession"]);
    // The team group of 0.3 moved into the Team window.
    expect(read(["team", "newSession"])).toEqual(["newSession"]);
    expect(read("newSession")).toEqual([]);
    expect(read(null)).toEqual([]);
    expect(parseLayout(JSON.stringify({ fontSize: 12 })).closedGroups).toEqual([]);
  });

  it("load a layout from the time of the Sessions and Worktrees groups", () => {
    // Both made way for one group per program: their folds are dropped, the programs start open.
    const old = parseLayout(
      JSON.stringify({ sidebarWidth: 330, closedGroups: ["sessions", "worktrees", "newSession"] }),
    );
    expect(old.sidebarWidth).toBe(330);
    expect(old.closedGroups).toEqual(["newSession"]);
    expect(old.closedAgentGroups).toEqual([]);
  });

  it("fold program groups by key and remember them", () => {
    const one = toggleAgentGroup(DEFAULT_LAYOUT, "program:claude");
    const two = toggleAgentGroup(one, "program:pwsh");
    expect(two.closedAgentGroups).toEqual(["program:claude", "program:pwsh"]);
    expect(toggleAgentGroup(two, "program:claude").closedAgentGroups).toEqual(["program:pwsh"]);
    expect(parseLayout(JSON.stringify(two)).closedAgentGroups).toEqual([
      "program:claude",
      "program:pwsh",
    ]);
  });

  it("migrate folds of the agent groups nested in Sessions and Worktrees", () => {
    const read = (closedAgentGroups: unknown) =>
      parseLayout(JSON.stringify({ closedAgentGroups })).closedAgentGroups;
    // Folded among the ordinary sessions: stays folded.
    expect(read(["sessions:claude"])).toEqual(["program:claude"]);
    // A worktree fold alone does not fold the whole program now.
    expect(read(["worktrees:claude"])).toEqual([]);
    expect(read(["worktrees:codex", "sessions:codex", "program:codex"])).toEqual(["program:codex"]);
    // A profile id may hold a colon itself.
    expect(read(["sessions:a:b"])).toEqual(["program:a:b"]);
    expect(read(["sessions:", "program:", "claude", "team:x"])).toEqual([]);
    // Migrating twice changes nothing.
    expect(read(read(["sessions:claude", "program:pwsh"]))).toEqual([
      "program:claude",
      "program:pwsh",
    ]);
  });

  it("keep folds of project groups next to those of program groups", () => {
    const read = (closedAgentGroups: unknown) =>
      parseLayout(JSON.stringify({ closedAgentGroups })).closedAgentGroups;
    expect(read(["project:c:\\src\\hangar", "program:claude", "project:/home/ada/x"])).toEqual([
      "project:c:\\src\\hangar",
      "program:claude",
      "project:/home/ada/x",
    ]);
    expect(read(["project:"])).toEqual([]);
  });

  it("read folded program groups robustly and keep at most the newest ones", () => {
    const read = (closedAgentGroups: unknown) =>
      parseLayout(JSON.stringify({ closedAgentGroups })).closedAgentGroups;
    // Layouts saved before program groups existed start with every group open.
    expect(parseLayout(JSON.stringify({ sidebarWidth: 300 })).closedAgentGroups).toEqual([]);
    expect(read("program:claude")).toEqual([]);
    expect(read(["program:a", 3, null, "", "program:a", "program:" + "x".repeat(201)])).toEqual([
      "program:a",
    ]);
    const many = Array.from(
      { length: CLOSED_AGENT_GROUPS_MAX + 5 },
      (_, i) => `program:${String(i)}`,
    );
    const kept = read(many);
    expect(kept).toHaveLength(CLOSED_AGENT_GROUPS_MAX);
    expect(kept[0]).toBe("program:5");
    let layout = DEFAULT_LAYOUT;
    for (const key of many) layout = toggleAgentGroup(layout, key);
    expect(layout.closedAgentGroups).toHaveLength(CLOSED_AGENT_GROUPS_MAX);
  });
});

describe("openGroup", () => {
  it("opens a closed group and leaves an open one as it is", () => {
    const closed = toggleGroup(DEFAULT_LAYOUT, "newSession");
    expect(openGroup(closed, "newSession").closedGroups).toEqual([]);
    expect(openGroup(DEFAULT_LAYOUT, "newSession")).toBe(DEFAULT_LAYOUT);
  });
});
