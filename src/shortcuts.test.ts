import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import {
  focusCycleStep,
  nextFocusZone,
  formatShortcut,
  ignoresRepeat,
  isReloadKey,
  matchShortcut,
  rowMoveStep,
  SHORTCUT_LABELS,
} from "./shortcuts";

function key(
  keyName: string,
  code: string,
  mods: Partial<{ ctrl: boolean; shift: boolean; alt: boolean; meta: boolean }> = {},
) {
  return {
    key: keyName,
    code,
    ctrlKey: mods.ctrl ?? true,
    shiftKey: mods.shift ?? false,
    altKey: mods.alt ?? false,
    metaKey: mods.meta ?? false,
  };
}

describe("matchShortcut", () => {
  it("switches sessions with Ctrl+Tab and Ctrl+Shift+Tab", () => {
    expect(matchShortcut(key("Tab", "Tab"))).toBe("nextSession");
    expect(matchShortcut(key("Tab", "Tab", { shift: true }))).toBe("prevSession");
  });

  it("sidebar and close only with Shift", () => {
    expect(matchShortcut(key("B", "KeyB", { shift: true }))).toBe("toggleSidebar");
    expect(matchShortcut(key("W", "KeyW", { shift: true }))).toBe("closeSession");
  });

  it("notes with Ctrl+Shift+N, Ctrl+N stays with the process", () => {
    expect(matchShortcut(key("N", "KeyN", { shift: true }))).toBe("quickNote");
    expect(matchShortcut(key("n", "KeyN"))).toBeNull();
    expect(matchShortcut(key("N", "KeyN", { shift: true, alt: true }))).toBeNull();
    expect(formatShortcut(SHORTCUT_LABELS.quickNote, createTranslator("de"))).toBe(
      "Strg+Umschalt+N",
    );
  });

  it("changes with Ctrl+Shift+D, Ctrl+D (end of input) stays with the shell", () => {
    expect(matchShortcut(key("D", "KeyD", { shift: true }))).toBe("toggleChanges");
    expect(matchShortcut(key("d", "KeyD"))).toBeNull();
    expect(matchShortcut(key("D", "KeyD", { shift: true, alt: true }))).toBeNull();
    expect(ignoresRepeat("toggleChanges")).toBe(true);
    expect(formatShortcut(SHORTCUT_LABELS.toggleChanges, createTranslator("en"))).toBe(
      "Ctrl+Shift+D",
    );
  });

  it("waiting session and new session only with Shift", () => {
    expect(matchShortcut(key("A", "KeyA", { shift: true }))).toBe("nextWaiting");
    expect(matchShortcut(key("T", "KeyT", { shift: true }))).toBe("newSession");
    expect(matchShortcut(key("a", "KeyA"))).toBeNull(); // start of line
    expect(matchShortcut(key("t", "KeyT"))).toBeNull(); // transpose characters, fzf
    expect(matchShortcut(key("A", "KeyA", { shift: true, alt: true }))).toBeNull();
    const de = createTranslator("de");
    expect(formatShortcut(SHORTCUT_LABELS.nextWaiting, de)).toBe("Strg+Umschalt+A");
    expect(formatShortcut(SHORTCUT_LABELS.newSession, de)).toBe("Strg+Umschalt+T");
  });

  it("search with Ctrl+Shift+F, Ctrl+F stays with the shell", () => {
    expect(matchShortcut(key("F", "KeyF", { shift: true }))).toBe("find");
    expect(matchShortcut(key("f", "KeyF"))).toBeNull(); // cursor forward
    expect(matchShortcut(key("F", "KeyF", { shift: true, alt: true }))).toBeNull();
    expect(ignoresRepeat("find")).toBe(true);
    expect(formatShortcut(SHORTCUT_LABELS.find, createTranslator("de"))).toBe("Strg+Umschalt+F");
  });

  it("quick access with Ctrl+Shift+P, Ctrl+P stays with the shell", () => {
    expect(matchShortcut(key("P", "KeyP", { shift: true }))).toBe("launcher");
    expect(matchShortcut(key("p", "KeyP"))).toBeNull(); // history back
    expect(matchShortcut(key("P", "KeyP", { shift: true, alt: true }))).toBeNull();
    expect(ignoresRepeat("launcher")).toBe(true);
    expect(formatShortcut(SHORTCUT_LABELS.launcher, createTranslator("en"))).toBe("Ctrl+Shift+P");
  });

  it("search across sessions with Ctrl+Shift+S, Ctrl+S stays with the shell", () => {
    expect(matchShortcut(key("S", "KeyS", { shift: true }))).toBe("searchAll");
    expect(matchShortcut(key("s", "KeyS"))).toBeNull(); // forward history search
    expect(matchShortcut(key("S", "KeyS", { shift: true, alt: true }))).toBeNull();
    expect(ignoresRepeat("searchAll")).toBe(true);
    expect(formatShortcut(SHORTCUT_LABELS.searchAll, createTranslator("de"))).toBe(
      "Strg+Umschalt+S",
    );
  });

  it("overview with Ctrl+Shift+M, Ctrl+M (Enter) and Ctrl+Tab keep their meaning", () => {
    expect(matchShortcut(key("M", "KeyM", { shift: true }))).toBe("overview");
    expect(matchShortcut(key("m", "KeyM"))).toBeNull(); // Enter
    expect(matchShortcut(key("Tab", "Tab"))).toBe("nextSession");
    expect(ignoresRepeat("overview")).toBe(true);
    expect(formatShortcut(SHORTCUT_LABELS.overview, createTranslator("en"))).toBe("Ctrl+Shift+M");
  });

  it("repeats held keys only where repeating is wanted", () => {
    expect(ignoresRepeat("newSession")).toBe(true);
    expect(ignoresRepeat("toggleSidebar")).toBe(true);
    expect(ignoresRepeat("nextSession")).toBe(false);
    expect(ignoresRepeat("fontBigger")).toBe(false);
  });

  it("assigns no key combination twice", () => {
    const labels = Object.values(SHORTCUT_LABELS).map((s) => `${s.mods.join("+")}+${s.key}`);
    expect(new Set(labels).size).toBe(labels.length);
  });

  it("passes shortcuts of agent CLIs and shells through", () => {
    expect(matchShortcut(key("b", "KeyB"))).toBeNull(); // Claude Code, tmux
    expect(matchShortcut(key("c", "KeyC"))).toBeNull(); // interrupt
    expect(matchShortcut(key("w", "KeyW"))).toBeNull(); // delete word
    expect(matchShortcut(key("r", "KeyR"))).toBeNull(); // history search
    expect(matchShortcut(key("Tab", "Tab", { ctrl: false }))).toBeNull(); // completion
  });

  it("font size on German and US keyboards", () => {
    expect(matchShortcut(key("+", "BracketRight"))).toBe("fontBigger"); // DE
    expect(matchShortcut(key("=", "Equal"))).toBe("fontBigger"); // US
    expect(matchShortcut(key("+", "NumpadAdd"))).toBe("fontBigger");
    expect(matchShortcut(key("-", "Slash"))).toBe("fontSmaller"); // DE
    expect(matchShortcut(key("-", "NumpadSubtract"))).toBe("fontSmaller");
    expect(matchShortcut(key("0", "Digit0"))).toBe("fontReset");
  });

  it("settings with Ctrl+Comma", () => {
    expect(matchShortcut(key(",", "Comma"))).toBe("openSettings");
    expect(matchShortcut(key(",", "Comma", { ctrl: false }))).toBeNull();
    expect(matchShortcut(key(";", "Comma", { shift: true }))).toBeNull();
  });

  it("ignores Alt and Meta", () => {
    expect(matchShortcut(key("Tab", "Tab", { alt: true }))).toBeNull();
    expect(matchShortcut(key("B", "KeyB", { shift: true, meta: true }))).toBeNull();
  });
});

describe("formatShortcut", () => {
  it("names modifiers in the UI language", () => {
    const de = createTranslator("de");
    const en = createTranslator("en");
    expect(formatShortcut(SHORTCUT_LABELS.toggleSidebar, de)).toBe("Strg+Umschalt+B");
    expect(formatShortcut(SHORTCUT_LABELS.toggleSidebar, en)).toBe("Ctrl+Shift+B");
    expect(formatShortcut(SHORTCUT_LABELS.openSettings, de)).toBe("Strg+,");
    expect(formatShortcut(SHORTCUT_LABELS.openSettings, en)).toBe("Ctrl+,");
  });
});

describe("isReloadKey", () => {
  it("recognizes the reload keys of the WebView", () => {
    expect(isReloadKey(key("F5", "F5", { ctrl: false }))).toBe(true);
    expect(isReloadKey(key("F5", "F5"))).toBe(true);
    expect(isReloadKey(key("r", "KeyR"))).toBe(true);
    expect(isReloadKey(key("R", "KeyR", { shift: true }))).toBe(true);
  });

  it("leaves other keys alone", () => {
    expect(isReloadKey(key("r", "KeyR", { ctrl: false }))).toBe(false);
    expect(isReloadKey(key("r", "KeyR", { alt: true }))).toBe(false);
    expect(isReloadKey(key("F6", "F6", { ctrl: false }))).toBe(false);
  });

  it("is never also an app shortcut", () => {
    expect(matchShortcut(key("r", "KeyR"))).toBeNull();
    expect(matchShortcut(key("R", "KeyR", { shift: true }))).toBeNull();
  });
});

describe("rowMoveStep", () => {
  const alt = { ctrl: false, alt: true };

  it("moves a session row with Alt+Up and Alt+Down", () => {
    expect(rowMoveStep(key("ArrowUp", "ArrowUp", alt))).toBe(-1);
    expect(rowMoveStep(key("ArrowDown", "ArrowDown", alt))).toBe(1);
  });

  it("ignores plain arrows and other modifiers", () => {
    expect(rowMoveStep(key("ArrowUp", "ArrowUp", { ctrl: false }))).toBeNull();
    expect(rowMoveStep(key("ArrowUp", "ArrowUp", { ctrl: true, alt: true }))).toBeNull();
    expect(
      rowMoveStep(key("ArrowDown", "ArrowDown", { ctrl: false, alt: true, shift: true })),
    ).toBeNull();
    expect(rowMoveStep(key("ArrowLeft", "ArrowLeft", alt))).toBeNull();
  });

  it("is never also an app shortcut", () => {
    expect(matchShortcut(key("ArrowUp", "ArrowUp", alt))).toBeNull();
  });
});

describe("focus cycle", () => {
  it("is F6 forward and Shift+F6 back, without other modifiers", () => {
    expect(focusCycleStep(key("F6", "F6", { ctrl: false }))).toBe(1);
    expect(focusCycleStep(key("F6", "F6", { ctrl: false, shift: true }))).toBe(-1);
    expect(focusCycleStep(key("F6", "F6"))).toBeNull();
    expect(focusCycleStep(key("F6", "F6", { ctrl: false, alt: true }))).toBeNull();
    expect(focusCycleStep(key("F5", "F5", { ctrl: false }))).toBeNull();
  });

  it("goes round terminal, list and editor", () => {
    const zones = ["terminal", "list", "editor"] as const;
    expect(nextFocusZone(zones, "terminal", 1)).toBe("list");
    expect(nextFocusZone(zones, "editor", 1)).toBe("terminal");
    expect(nextFocusZone(zones, "terminal", -1)).toBe("editor");
    expect(nextFocusZone(zones, null, 1)).toBe("terminal");
    expect(nextFocusZone(["terminal", "list"], "editor", 1)).toBe("terminal");
    expect(nextFocusZone([], "list", 1)).toBeNull();
  });
});
