import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import {
  formatShortcut,
  ignoresRepeat,
  isReloadKey,
  matchShortcut,
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
    expect(matchShortcut(key("N", "KeyN", { shift: true }))).toBe("toggleNotes");
    expect(matchShortcut(key("n", "KeyN"))).toBeNull();
    expect(matchShortcut(key("N", "KeyN", { shift: true, alt: true }))).toBeNull();
    expect(formatShortcut(SHORTCUT_LABELS.toggleNotes, createTranslator("de"))).toBe(
      "Strg+Umschalt+N",
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
