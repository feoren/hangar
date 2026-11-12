import { describe, expect, it } from "vitest";

import {
  CHIME_VOLUME,
  defaultSettings,
  gateActive,
  parseSettings,
  QUIET_SECONDS,
  SCROLLBACK_LINES,
} from "./settings";
import { defaultSnippets } from "./snippets";
import { DEFAULT_LOOKS } from "./stateLook";

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
      looks: {
        working: { color: "#8CC8FF", pulse: "subtle", seconds: 1.2 },
        done: { color: null, pulse: "strong", seconds: 2 },
        asks: { color: "#f0b429", pulse: "off", seconds: 4 },
        idle: { color: null, pulse: "off", seconds: 2.8 },
        failed: { color: "#f5a3c7", pulse: "subtle", seconds: 1.6 },
      },
      dotAnimation: "wave",
      quietSeconds: 10,
      taskbarFlash: false,
      notifyDesktop: false,
      chime: false,
      chimeVolume: 70,
      quietHours: { enabled: true, from: 1380, to: 360 },
      keepInTray: true,
      showVersions: false,
      showIcons: false,
      autoUpdate: false,
      checkProgramUpdates: false,
      restoreSessions: false,
      scrollbackLines: 2000,
      subAgentExcerpts: false,
      claimsEnabled: false,
      gateEnabled: false,
      programs: { order: ["b", "a"], hidden: ["a"], icons: {} },
      terminalScheme: "solarized",
      terminalFont: "JetBrains Mono",
      cursorStyle: "bar",
      cursorBlink: false,
      viewerTheme: "github",
      sessionGrouping: "project",
      snippets: [{ id: "mine", text: "Fix the build" }],
    };
    expect(parseSettings(JSON.stringify(stored), "de")).toEqual({
      ...stored,
      looks: { ...stored.looks, working: { ...stored.looks.working, color: "#8cc8ff" } },
    });
  });

  it("replaces wrong types individually", () => {
    const stored = {
      language: "xx",
      theme: "auto",
      attentionEnabled: "yes",
      looks: 123,
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
      subAgentExcerpts: "off",
      claimsEnabled: "no",
      gateEnabled: 0,
      dotAnimation: "spin",
      sessionGrouping: "folder",
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
        terminalScheme: "midnight",
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

  it("reads the file viewer theme and falls back to One Dark Pro", () => {
    expect(defaultSettings("de").viewerTheme).toBe("oneDarkPro");
    expect(parseSettings(JSON.stringify({ language: "en" }), "de").viewerTheme).toBe("oneDarkPro");
    for (const theme of ["github", "vscode", "darcula", "solarized", "catppuccin", "monokai"]) {
      expect(parseSettings(JSON.stringify({ viewerTheme: theme }), "de").viewerTheme).toBe(theme);
    }
    for (const wrong of ["GitHub", "atom", 3, null]) {
      expect(parseSettings(JSON.stringify({ viewerTheme: wrong }), "de").viewerTheme).toBe(
        "oneDarkPro",
      );
    }
  });

  it("keeps saved color schemes from older versions and reads the new ones", () => {
    for (const scheme of ["hangar", "catppuccin", "solarized", "dusk", "dracula", "rosePine"]) {
      expect(parseSettings(JSON.stringify({ terminalScheme: scheme }), "de").terminalScheme).toBe(
        scheme,
      );
    }
    expect(parseSettings(JSON.stringify({ terminalScheme: "Harbor" }), "de").terminalScheme).toBe(
      "hangar",
    );
  });

  it("moves the renamed schemes Harbor and Meridian to their origins", () => {
    expect(parseSettings(JSON.stringify({ terminalScheme: "harbor" }), "de").terminalScheme).toBe(
      "github",
    );
    expect(parseSettings(JSON.stringify({ terminalScheme: "meridian" }), "de").terminalScheme).toBe(
      "oneDarkPro",
    );
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

  it("shows sub-agent excerpts by default, also for older settings", () => {
    expect(defaultSettings("de").subAgentExcerpts).toBe(true);
    expect(parseSettings(JSON.stringify({ language: "en" }), "de").subAgentExcerpts).toBe(true);
    expect(parseSettings(JSON.stringify({ subAgentExcerpts: false }), "de").subAgentExcerpts).toBe(
      false,
    );
  });

  it("detects overlaps between sessions by default, also for older settings", () => {
    expect(defaultSettings("de").claimsEnabled).toBe(true);
    expect(parseSettings(JSON.stringify({ language: "en" }), "de").claimsEnabled).toBe(true);
    expect(parseSettings(JSON.stringify({ claimsEnabled: false }), "de").claimsEnabled).toBe(false);
  });

  it("pauses agents before overlapping edits by default, also for older settings", () => {
    expect(defaultSettings("de").gateEnabled).toBe(true);
    expect(parseSettings(JSON.stringify({ language: "en" }), "de").gateEnabled).toBe(true);
    expect(parseSettings(JSON.stringify({ gateEnabled: false }), "de").gateEnabled).toBe(false);
  });

  it("turns the gate on only together with the overlap detection", () => {
    expect(gateActive({ claimsEnabled: true, gateEnabled: true })).toBe(true);
    expect(gateActive({ claimsEnabled: false, gateEnabled: true })).toBe(false);
    expect(gateActive({ claimsEnabled: true, gateEnabled: false })).toBe(false);
  });

  it("keeps the scrollback per session within limits and steps", () => {
    const parse = (scrollbackLines: number) =>
      parseSettings(JSON.stringify({ scrollbackLines }), "de").scrollbackLines;
    expect(parse(0)).toBe(SCROLLBACK_LINES.min);
    expect(parse(1e9)).toBe(SCROLLBACK_LINES.max);
    expect(parse(2240)).toBe(2000);
    expect(parse(2260)).toBe(2500);
  });

  it("reads looks stored before failed had its own look, and bounces the dots", () => {
    const old = {
      looks: {
        working: { color: "#8cc8ff", pulse: "off", seconds: 2.8 },
        done: { color: null, pulse: "medium", seconds: 2.8 },
        asks: { color: null, pulse: "medium", seconds: 2.8 },
        idle: { color: null, pulse: "off", seconds: 2.8 },
      },
    };
    const parsed = parseSettings(JSON.stringify(old), "de");
    expect(parsed.looks.working.color).toBe("#8cc8ff");
    expect(parsed.looks.failed).toEqual(DEFAULT_LOOKS.failed);
    expect(parsed.dotAnimation).toBe("bounce");
  });

  it("drops invalid looks", () => {
    for (const looks of [null, "red", [], { done: { color: "url(x)", pulse: "wild" } }]) {
      expect(parseSettings(JSON.stringify({ looks }), "de").looks).toEqual(DEFAULT_LOOKS);
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

describe("attention color of older versions", () => {
  it("carries over to both waiting states and leaves the rest as before", () => {
    const looks = parseSettings('{"attentionColor":"#123456"}', "de").looks;
    expect(looks.done).toEqual({ ...DEFAULT_LOOKS.done, color: "#123456" });
    expect(looks.asks).toEqual({ ...DEFAULT_LOOKS.asks, color: "#123456" });
    expect(looks.working).toEqual(DEFAULT_LOOKS.working);
    expect(looks.idle).toEqual(DEFAULT_LOOKS.idle);
  });

  it("keeps the default for cream and invalid colors and upgrades the former mint", () => {
    for (const attentionColor of ["#f2e3c6", "red", "#12345", "url(x)", "#f2e3c6; x", 123]) {
      expect(parseSettings(JSON.stringify({ attentionColor }), "de").looks).toEqual(DEFAULT_LOOKS);
    }
    expect(parseSettings('{"attentionColor":"#7ee0b5"}', "de").looks.done.color).toBe("#9ee8dc");
  });

  it("is ignored once looks are stored", () => {
    const text = JSON.stringify({ attentionColor: "#123456", looks: DEFAULT_LOOKS });
    expect(parseSettings(text, "de").looks).toEqual(DEFAULT_LOOKS);
  });
});

describe("chime", () => {
  it("is on and quiet by default", () => {
    const settings = defaultSettings("de");
    expect(settings.chime).toBe(true);
    expect(settings.chimeVolume).toBe(CHIME_VOLUME.default);
  });

  it("takes a stored choice, rounds the volume to its step and clamps it", () => {
    const parsed = parseSettings('{"chime":false,"chimeVolume":62}', "de");
    expect(parsed.chime).toBe(false);
    expect(parsed.chimeVolume).toBe(60);
    expect(parseSettings('{"chimeVolume":0}', "de").chimeVolume).toBe(CHIME_VOLUME.min);
    expect(parseSettings('{"chimeVolume":900}', "de").chimeVolume).toBe(CHIME_VOLUME.max);
    expect(parseSettings('{"chimeVolume":"loud"}', "de").chimeVolume).toBe(CHIME_VOLUME.default);
  });
});
