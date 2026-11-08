import { describe, expect, it } from "vitest";

import {
  ATTENTION_PRESETS,
  DEFAULT_ATTENTION_COLOR,
  defaultSettings,
  normalizeColor,
  parseSettings,
  QUIET_SECONDS,
  SCROLLBACK_LINES,
} from "./settings";
import { defaultSnippets } from "./snippets";

describe("parseSettings", () => {
  it("uses defaults without stored content", () => {
    expect(parseSettings(null, "de")).toEqual(defaultSettings("de"));
    expect(parseSettings("", "en")).toEqual(defaultSettings("en"));
  });

  it("survives invalid JSON and wrong root types", () => {
    expect(parseSettings("{broken", "de")).toEqual(defaultSettings("de"));
    expect(parseSettings("42", "de")).toEqual(defaultSettings("de"));
    expect(parseSettings("null", "de")).toEqual(defaultSettings("de"));
    expect(parseSettings("[1,2]", "de")).toEqual(defaultSettings("de"));
  });

  it("takes valid values", () => {
    const stored = {
      language: "en",
      theme: "light",
      attentionEnabled: false,
      attentionColor: "#7EC0B5",
      quietSeconds: 10,
      taskbarFlash: false,
      notifyDesktop: false,
      keepInTray: true,
      showVersions: false,
      showIcons: false,
      autoUpdate: false,
      checkProgramUpdates: false,
      restoreSessions: false,
      scrollbackLines: 2000,
      programs: { order: ["b", "a"], hidden: ["a"], icons: {} },
      terminalScheme: "solarized",
      terminalFont: "JetBrains Mono",
      cursorStyle: "bar",
      cursorBlink: false,
      snippets: [{ id: "mine", text: "Fix the build" }],
    };
    expect(parseSettings(JSON.stringify(stored), "de")).toEqual({
      ...stored,
      attentionColor: "#7ec0b5",
    });
  });

  it("replaces wrong types individually", () => {
    const stored = {
      language: "xx",
      theme: "auto",
      attentionEnabled: "yes",
      attentionColor: 123,
      quietSeconds: "5",
      taskbarFlash: "yes",
      notifyDesktop: "no",
      keepInTray: 1,
      showVersions: "no",
      showIcons: null,
      autoUpdate: 1,
      checkProgramUpdates: "yes",
      restoreSessions: 0,
      scrollbackLines: "many",
    };
    expect(parseSettings(JSON.stringify(stored), "de")).toEqual(defaultSettings("de"));
  });

  it("keeps the previous terminal look by default, also for older settings", () => {
    for (const settings of [
      defaultSettings("de"),
      parseSettings(JSON.stringify({ language: "en" }), "de"),
    ]) {
      expect(settings.terminalScheme).toBe("hangar");
      expect(settings.terminalFont).toBe("default");
      expect(settings.cursorStyle).toBe("block");
      expect(settings.cursorBlink).toBe(true);
    }
  });

  it("replaces invalid terminal look values individually", () => {
    const parsed = parseSettings(
      JSON.stringify({
        terminalScheme: "dracula",
        terminalFont: "Comic Sans MS",
        cursorStyle: 2,
        cursorBlink: "no",
      }),
      "de",
    );
    expect(parsed.terminalScheme).toBe("hangar");
    expect(parsed.terminalFont).toBe("default");
    expect(parsed.cursorStyle).toBe("block");
    expect(parsed.cursorBlink).toBe(true);
    const valid = parseSettings(
      JSON.stringify({ terminalScheme: "catppuccin", cursorStyle: "underline" }),
      "de",
    );
    expect(valid.terminalScheme).toBe("catppuccin");
    expect(valid.cursorStyle).toBe("underline");
  });

  it("follows the system by default and replaces invalid themes", () => {
    expect(defaultSettings("de").theme).toBe("system");
    for (const theme of ["dark", "light", "system"]) {
      expect(parseSettings(JSON.stringify({ theme }), "de").theme).toBe(theme);
    }
    for (const theme of ["Dark", "", 1, null, true]) {
      expect(parseSettings(JSON.stringify({ theme }), "de").theme).toBe("system");
    }
  });

  it("shows versions and icons by default", () => {
    const settings = defaultSettings("de");
    expect(settings.showVersions).toBe(true);
    expect(settings.showIcons).toBe(true);
    // Older stored settings without the new fields.
    const old = parseSettings(JSON.stringify({ language: "en", autoUpdate: false }), "de");
    expect(old.showVersions).toBe(true);
    expect(old.showIcons).toBe(true);
  });

  it("seeds snippets in the stored language and keeps the user's list", () => {
    const old = parseSettings(JSON.stringify({ language: "en" }), "de");
    expect(old.snippets).toEqual(defaultSnippets("en"));
    const none = parseSettings(JSON.stringify({ snippets: [] }), "de");
    expect(none.snippets).toEqual([]);
    const own = parseSettings(JSON.stringify({ snippets: [{ id: "a", text: "Hi" }] }), "de");
    expect(own.snippets).toEqual([{ id: "a", text: "Hi" }]);
  });

  it("replaces invalid values for versions and icons", () => {
    for (const value of [0, 1, "false", null, {}, []]) {
      const parsed = parseSettings(JSON.stringify({ showVersions: value, showIcons: value }), "de");
      expect(parsed.showVersions).toBe(true);
      expect(parsed.showIcons).toBe(true);
    }
  });

  it("keeps the quiet time within its limits", () => {
    const parse = (quietSeconds: number) =>
      parseSettings(JSON.stringify({ quietSeconds }), "de").quietSeconds;
    expect(parse(0)).toBe(QUIET_SECONDS.min);
    expect(parse(-5)).toBe(QUIET_SECONDS.min);
    expect(parse(999)).toBe(QUIET_SECONDS.max);
    expect(parse(4.6)).toBe(5);
  });

  it("flashes the taskbar by default, also for older settings", () => {
    expect(defaultSettings("de").taskbarFlash).toBe(true);
    expect(parseSettings(JSON.stringify({ language: "en" }), "de").taskbarFlash).toBe(true);
    expect(parseSettings(JSON.stringify({ taskbarFlash: false }), "de").taskbarFlash).toBe(false);
  });

  it("notifies by default and keeps running in the notification area only when chosen", () => {
    const old = parseSettings(JSON.stringify({ language: "en" }), "de");
    expect(old.notifyDesktop).toBe(true);
    expect(old.keepInTray).toBe(false);
    expect(defaultSettings("de").notifyDesktop).toBe(true);
    expect(defaultSettings("de").keepInTray).toBe(false);
  });

  it("remembers sessions and checks for program updates by default", () => {
    const old = parseSettings(JSON.stringify({ language: "en" }), "de");
    expect(old.restoreSessions).toBe(true);
    expect(old.checkProgramUpdates).toBe(true);
    expect(old.scrollbackLines).toBe(SCROLLBACK_LINES.default);
  });

  it("keeps the scrollback per session within limits and steps", () => {
    const parse = (scrollbackLines: number) =>
      parseSettings(JSON.stringify({ scrollbackLines }), "de").scrollbackLines;
    expect(parse(0)).toBe(SCROLLBACK_LINES.min);
    expect(parse(1e9)).toBe(SCROLLBACK_LINES.max);
    expect(parse(2240)).toBe(2000);
    expect(parse(2260)).toBe(2500);
  });

  it("drops invalid colors", () => {
    for (const attentionColor of ["red", "#12345", "#ggg000", "url(x)", "#f2e3c6; x"]) {
      expect(parseSettings(JSON.stringify({ attentionColor }), "de").attentionColor).toBe(
        DEFAULT_ATTENTION_COLOR,
      );
    }
  });
});

describe("terminals and agents", () => {
  it("starts without custom preferences and replaces nonsense", () => {
    expect(defaultSettings("de").programs).toEqual({ order: [], hidden: [], icons: {} });
    for (const programs of [null, 1, "x", []]) {
      expect(parseSettings(JSON.stringify({ programs }), "de").programs).toEqual(
        defaultSettings("de").programs,
      );
    }
  });

  it("keeps unknown IDs", () => {
    const parsed = parseSettings(
      JSON.stringify({ programs: { order: ["uninstalled", "pwsh"], hidden: ["old"] } }),
      "de",
    );
    expect(parsed.programs.order).toEqual(["uninstalled", "pwsh"]);
    expect(parsed.programs.hidden).toEqual(["old"]);
  });
});

describe("normalizeColor", () => {
  it("normalizes to #rrggbb", () => {
    expect(normalizeColor("#ABC")).toBe("#aabbcc");
    expect(normalizeColor("#F2E3C6")).toBe("#f2e3c6");
    expect(normalizeColor("f2e3c6")).toBeNull();
    expect(normalizeColor(null)).toBeNull();
  });
});

describe("mint preset", () => {
  it("upgrades the former mint color to the new one and keeps custom colors", () => {
    const mint = ATTENTION_PRESETS.find((preset) => preset.id === "mint")?.color;
    expect(mint).toBe("#9ee8dc");
    expect(parseSettings('{"attentionColor":"#7ee0b5"}', "de").attentionColor).toBe(mint);
    expect(parseSettings('{"attentionColor":"#123456"}', "de").attentionColor).toBe("#123456");
  });
});
