import { describe, expect, it } from "vitest";

import {
  firstIndex,
  fitInViewport,
  INITIAL_SUBMENU,
  lastIndex,
  menuCommand,
  pointerMoved,
  secondaryCommand,
  stepIndex,
  submenuAfterFocus,
  typeaheadIndex,
  type NavEntry,
} from "./menuNav";

const action = (label: string, enabled = true): NavEntry => ({ label, enabled, submenu: false });
const sub = (label: string, enabled = true): NavEntry => ({ label, enabled, submenu: true });

// Copy, (separator), Paste (disabled), Select all, File ›
const ENTRIES: NavEntry[] = [
  action("Copy"),
  null,
  action("Paste", false),
  action("Select all"),
  sub("File"),
];

describe("stepIndex", () => {
  it("skips separators and disabled entries", () => {
    expect(stepIndex(ENTRIES, 0, 1)).toBe(3);
    expect(stepIndex(ENTRIES, 3, -1)).toBe(0);
  });

  it("wraps around at the ends", () => {
    expect(stepIndex(ENTRIES, 4, 1)).toBe(0);
    expect(stepIndex(ENTRIES, 0, -1)).toBe(4);
  });

  it("finds the first or last entry without a current one", () => {
    expect(firstIndex(ENTRIES)).toBe(0);
    expect(lastIndex(ENTRIES)).toBe(4);
    expect(firstIndex([null, action("A", false)])).toBe(-1);
    expect(firstIndex([])).toBe(-1);
  });
});

describe("typeaheadIndex", () => {
  it("jumps to the next entry with the first letter", () => {
    expect(typeaheadIndex(ENTRIES, 0, "s")).toBe(3);
    expect(typeaheadIndex(ENTRIES, 3, "C")).toBe(0);
    expect(typeaheadIndex(ENTRIES, 0, "f")).toBe(4);
  });

  it("skips disabled entries and returns -1 without a match", () => {
    expect(typeaheadIndex(ENTRIES, 0, "p")).toBe(-1);
    expect(typeaheadIndex(ENTRIES, 0, "x")).toBe(-1);
  });

  it("cycles through entries with the same first letter", () => {
    const entries = [action("Font larger"), action("Font smaller"), action("Fullscreen")];
    expect(typeaheadIndex(entries, 0, "f")).toBe(1);
    expect(typeaheadIndex(entries, 1, "f")).toBe(2);
    expect(typeaheadIndex(entries, 2, "f")).toBe(0);
  });
});

describe("menuCommand", () => {
  const key = (k: string, mods: { ctrlKey?: boolean } = {}) => ({ key: k, ...mods });

  it("arrows, Home and End move the focus", () => {
    expect(menuCommand(key("ArrowDown"), ENTRIES, 0, false)).toEqual({ kind: "focus", index: 3 });
    expect(menuCommand(key("ArrowUp"), ENTRIES, 0, false)).toEqual({ kind: "focus", index: 4 });
    expect(menuCommand(key("Home"), ENTRIES, 4, false)).toEqual({ kind: "focus", index: 0 });
    expect(menuCommand(key("End"), ENTRIES, 0, false)).toEqual({ kind: "focus", index: 4 });
  });

  it("right only opens a submenu, left only closes a submenu", () => {
    expect(menuCommand(key("ArrowRight"), ENTRIES, 4, false)).toEqual({ kind: "openSubmenu" });
    expect(menuCommand(key("ArrowRight"), ENTRIES, 0, false)).toEqual({ kind: "none" });
    expect(menuCommand(key("ArrowLeft"), ENTRIES, 0, true)).toEqual({ kind: "closeSubmenu" });
    expect(menuCommand(key("ArrowLeft"), ENTRIES, 0, false)).toEqual({ kind: "none" });
  });

  it("Enter and Space activate or open", () => {
    expect(menuCommand(key("Enter"), ENTRIES, 0, false)).toEqual({ kind: "activate" });
    expect(menuCommand(key(" "), ENTRIES, 3, true)).toEqual({ kind: "activate" });
    expect(menuCommand(key("Enter"), ENTRIES, 4, false)).toEqual({ kind: "openSubmenu" });
    expect(menuCommand(key("Enter"), ENTRIES, 2, false)).toEqual({ kind: "none" });
  });

  it("Escape closes one level, Tab the whole menu", () => {
    expect(menuCommand(key("Escape"), ENTRIES, 0, true)).toEqual({ kind: "closeSubmenu" });
    expect(menuCommand(key("Escape"), ENTRIES, 0, false)).toEqual({ kind: "closeAll" });
    expect(menuCommand(key("Tab"), ENTRIES, 0, true)).toEqual({ kind: "closeAll" });
  });

  it("a first letter jumps, but not with Ctrl", () => {
    expect(menuCommand(key("s"), ENTRIES, 0, false)).toEqual({ kind: "focus", index: 3 });
    expect(menuCommand(key("s", { ctrlKey: true }), ENTRIES, 0, false)).toEqual({ kind: "none" });
    expect(menuCommand(key("F2"), ENTRIES, 0, false)).toEqual({ kind: "none" });
  });
});

describe("fitInViewport", () => {
  const viewport = { width: 800, height: 600 };
  const size = { width: 200, height: 150 };

  it("keeps menus that fit at the pointer", () => {
    expect(fitInViewport({ x: 100, y: 120 }, size, viewport)).toEqual({ x: 100, y: 120 });
  });

  it("pushes menus at the edge into the window", () => {
    expect(fitInViewport({ x: 750, y: 580 }, size, viewport)).toEqual({ x: 592, y: 442 });
    expect(fitInViewport({ x: -5, y: 2 }, size, viewport)).toEqual({ x: 8, y: 8 });
  });

  it("keeps the top left margin in windows that are too small", () => {
    expect(fitInViewport({ x: 50, y: 50 }, { width: 900, height: 700 }, viewport)).toEqual({
      x: 8,
      y: 8,
    });
  });
});

describe("secondaryCommand", () => {
  it("moves between a row and its second button", () => {
    expect(secondaryCommand({ key: "ArrowRight" }, true, false)).toBe("enter");
    expect(secondaryCommand({ key: "ArrowLeft" }, true, true)).toBe("leave");
    expect(secondaryCommand({ key: "Enter" }, true, true)).toBe("run");
    expect(secondaryCommand({ key: " " }, true, true)).toBe("run");
  });

  it("runs it from the row with Shift+Enter only", () => {
    expect(secondaryCommand({ key: "Enter", shiftKey: true }, true, false)).toBe("run");
    expect(secondaryCommand({ key: "Enter" }, true, false)).toBeNull();
  });

  it("leaves rows without a second button alone", () => {
    expect(secondaryCommand({ key: "ArrowRight" }, false, false)).toBeNull();
    expect(secondaryCommand({ key: "ArrowDown" }, true, true)).toBeNull();
  });
});

describe("submenus open only on purpose", () => {
  it("opens the menu with every submenu closed", () => {
    expect(INITIAL_SUBMENU).toBeNull();
  });

  it("closes an open submenu when the keyboard moves on, without opening the next one", () => {
    expect(submenuAfterFocus(null, 1)).toBeNull();
    expect(submenuAfterFocus(0, 1)).toBeNull();
    expect(submenuAfterFocus(1, 1)).toBe(1);
  });

  it("ignores a pointer that rests where the menu appeared", () => {
    expect(pointerMoved(null, { x: 10, y: 20 })).toBe(false);
    expect(pointerMoved({ x: 10, y: 20 }, { x: 10, y: 20 })).toBe(false);
  });

  it("counts real movement as hover", () => {
    expect(pointerMoved({ x: 10, y: 20 }, { x: 11, y: 20 })).toBe(true);
    expect(pointerMoved({ x: 10, y: 20 }, { x: 10, y: 18 })).toBe(true);
  });
});
