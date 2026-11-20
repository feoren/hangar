import { describe, expect, it } from "vitest";

import {
  CHIME_VOLUME,
  defaultSettings,
  gateActive,
  BACKEND_KEYS,
  DICTATION_KEY,
  legacyNotices,
  legacyProtected,
  legacySwitches,
  NOTICE_KEYS,
  noticesOf,
  parseSettings,
  PROTECTED_KEYS,
  QUIET_SECONDS,
  SCROLLBACK_LINES,
  SECURITY_SWITCH_KEYS,
  withoutBackendKeys,
} from "./settings";
import { DEFAULT_QUIET_HOURS } from "./quietHours";
import { DEFAULT_ROW_LAYOUT, presetLayout, ROW_PRESETS } from "./rowLayout";
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
      rowLayout: presetLayout("ring"),
      rowLayoutBefore: presetLayout("tiles"),
      treeFlow: { enabled: true, style: "pulse", showRate: false },
      quietSeconds: 10,
      stallMinutes: 12,
      waitMinutes: 20,
      taskbarFlash: false,
      notifyDesktop: false,
      chime: false,
      chimeVolume: 70,
      quietHours: { enabled: true, from: 1380, to: 360 },
      showResources: false,
      powerSaving: false,
      keepInTray: true,
      showVersions: false,
      showIcons: false,
      autoUpdate: false,
      checkProgramUpdates: false,
      restoreSessions: false,
      scrollbackLines: 2000,
      subAgentExcerpts: false,
      crewColumn: false,
      claimsEnabled: false,
      gateEnabled: false,
      commandGuardEnabled: false,
      hangarMd: false,
      worktreeUpstreamCheck: false,
      warnMultilinePaste: false,
      programs: { order: ["b", "a"], hidden: ["a"], icons: {} },
      terminalScheme: "sundial",
      terminalFont: "JetBrains Mono",
      cursorStyle: "bar",
      cursorBlink: false,
      viewerTheme: "harbor",
      sessionGrouping: "project",
      snippets: [{ id: "mine", text: "Fix the build" }],
      dailyBudget: 12.5,
      dictation: { enabled: true, model: "small", language: "de" },
    };
    expect(parseSettings(JSON.stringify(stored), "de")).toEqual({
      ...stored,
      // The security switches, the protected and the notice settings come from the backend
      // only (mainStore.ts), never from storage.
      attentionEnabled: true,
      taskbarFlash: true,
      notifyDesktop: true,
      chime: true,
      quietHours: DEFAULT_QUIET_HOURS,
      restoreSessions: true,
      dailyBudget: null,
      claimsEnabled: true,
      gateEnabled: true,
      commandGuardEnabled: true,
      autoUpdate: true,
      hangarMd: true,
      warnMultilinePaste: true,
      snippets: defaultSnippets("en"),
      // The dictation key too; model and language stay in storage.
      dictationEnabled: false,
      dictation: { model: "small", language: "de" },
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
      showResources: "off",
      powerSaving: "yes",
      keepInTray: 1,
      showVersions: "no",
      showIcons: null,
      autoUpdate: 1,
      checkProgramUpdates: "yes",
      restoreSessions: 0,
      scrollbackLines: "many",
      subAgentExcerpts: "off",
      crewColumn: "off",
      claimsEnabled: "no",
      gateEnabled: 0,
      commandGuardEnabled: "on",
      warnMultilinePaste: "no",
      dotAnimation: "spin",
      rowStyle: "fancy",
      rowLayout: "grid",
      treeFlow: "on",
      sessionGrouping: "folder",
    };
    expect(parseSettings(JSON.stringify(stored), "de")).toEqual(defaultSettings("de"));
  });

  it("warns before a multi-line paste by default, also for older settings", () => {
    expect(defaultSettings("de").warnMultilinePaste).toBe(true);
    expect(parseSettings(JSON.stringify({ language: "en" }), "de").warnMultilinePaste).toBe(true);
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
      JSON.stringify({ terminalScheme: "velvet", cursorStyle: "underline" }),
      "de",
    );
    expect(valid.terminalScheme).toBe("velvet");
    expect(valid.cursorStyle).toBe("underline");
  });

  it("reads the file viewer theme and falls back to Meridian", () => {
    expect(defaultSettings("de").viewerTheme).toBe("meridian");
    expect(parseSettings(JSON.stringify({ language: "en" }), "de").viewerTheme).toBe("meridian");
    for (const theme of ["harbor", "studio", "anvil", "sundial", "velvet", "ember"]) {
      expect(parseSettings(JSON.stringify({ viewerTheme: theme }), "de").viewerTheme).toBe(theme);
    }
    for (const wrong of ["GitHub", "atom", 3, null]) {
      expect(parseSettings(JSON.stringify({ viewerTheme: wrong }), "de").viewerTheme).toBe(
        "meridian",
      );
    }
  });

  it("carries a viewer theme saved by 0.6.0 over to its new name", () => {
    const renamed = {
      oneDarkPro: "meridian",
      github: "harbor",
      vscode: "studio",
      darcula: "anvil",
      solarized: "sundial",
      catppuccin: "velvet",
      monokai: "ember",
    };
    for (const [old, now] of Object.entries(renamed)) {
      expect(parseSettings(JSON.stringify({ viewerTheme: old }), "de").viewerTheme, old).toBe(now);
    }
  });

  it("keeps saved color schemes and reads the new ones", () => {
    for (const scheme of ["hangar", "dusk", "harbor", "meridian", "nocturne", "heather"]) {
      expect(parseSettings(JSON.stringify({ terminalScheme: scheme }), "de").terminalScheme).toBe(
        scheme,
      );
    }
    expect(parseSettings(JSON.stringify({ terminalScheme: "Harbor" }), "de").terminalScheme).toBe(
      "hangar",
    );
  });

  it("carries a color scheme saved by an older version over to its new name", () => {
    const renamed = {
      oneDarkPro: "meridian",
      github: "harbor",
      vscode: "studio",
      darcula: "anvil",
      solarized: "sundial",
      catppuccin: "velvet",
      monokai: "ember",
      dracula: "nocturne",
      nord: "fjord",
      gruvbox: "ochre",
      tokyoNight: "neon",
      rosePine: "heather",
      // Keys from before 0.6.0, current again.
      harbor: "harbor",
      meridian: "meridian",
    };
    for (const [old, now] of Object.entries(renamed)) {
      expect(parseSettings(JSON.stringify({ terminalScheme: old }), "de").terminalScheme, old).toBe(
        now,
      );
    }
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

  it("migrates snippets in the stored language and keeps the user's list", () => {
    const old = legacyProtected(JSON.stringify({ language: "en" }), "de");
    expect(old.snippets).toEqual(defaultSnippets("en"));
    expect(legacyProtected(null, "de").snippets).toEqual(defaultSnippets("de"));
    const none = legacyProtected(JSON.stringify({ snippets: [] }), "de");
    expect(none.snippets).toEqual([]);
    const own = legacyProtected(JSON.stringify({ snippets: [{ id: "a", text: "Hi" }] }), "de");
    expect(own.snippets).toEqual([{ id: "a", text: "Hi" }]);
  });

  it("reads the protected settings an older version stored, once, for the migration", () => {
    const stored = JSON.stringify({ warnMultilinePaste: false, autoUpdate: false, hangarMd: "no" });
    expect(legacyProtected(stored, "en")).toEqual({
      warnMultilinePaste: false,
      autoUpdate: false,
      hangarMd: true,
      snippets: defaultSnippets("en"),
    });
    for (const broken of ["{broken", "[false]", "42"]) {
      expect(legacyProtected(broken, "en")).toEqual({
        warnMultilinePaste: true,
        autoUpdate: true,
        hangarMd: true,
        snippets: defaultSnippets("en"),
      });
    }
  });

  // The panel and file viewer windows share this storage: whatever stands there is ignored.
  it("never reads warnMultilinePaste from localStorage", () => {
    const settings = parseSettings(JSON.stringify({ warnMultilinePaste: false }), "de");
    expect(settings.warnMultilinePaste).toBe(true);
  });

  it("never reads autoUpdate from localStorage", () => {
    expect(parseSettings(JSON.stringify({ autoUpdate: false }), "de").autoUpdate).toBe(true);
  });

  it("never reads hangarMd from localStorage", () => {
    expect(parseSettings(JSON.stringify({ hangarMd: false }), "de").hangarMd).toBe(true);
  });

  it("never reads snippets from localStorage", () => {
    const planted = [{ id: "x", text: "HANGAR-MARKER" }];
    const settings = parseSettings(JSON.stringify({ language: "en", snippets: planted }), "de");
    expect(settings.snippets).toEqual(defaultSnippets("en"));
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

  it("hints at a stuck agent after five minutes by default, 0 turns it off", () => {
    expect(defaultSettings("de").stallMinutes).toBe(5);
    expect(parseSettings(JSON.stringify({ language: "en" }), "de").stallMinutes).toBe(5);
    const parse = (stallMinutes: unknown) =>
      parseSettings(JSON.stringify({ stallMinutes }), "de").stallMinutes;
    expect(parse(0)).toBe(0);
    expect(parse(12)).toBe(12);
    expect(parse(999)).toBe(30);
    expect(parse("7")).toBe(5);
  });

  it("hints at a long wait after ten minutes by default, 0 turns it off", () => {
    expect(defaultSettings("de").waitMinutes).toBe(10);
    expect(parseSettings(JSON.stringify({ language: "en" }), "de").waitMinutes).toBe(10);
    const parse = (waitMinutes: unknown) =>
      parseSettings(JSON.stringify({ waitMinutes }), "de").waitMinutes;
    expect(parse(0)).toBe(0);
    expect(parse(25)).toBe(25);
    expect(parse(999)).toBe(60);
    expect(parse("7")).toBe(10);
  });

  it("flashes the taskbar by default, also for older settings", () => {
    expect(defaultSettings("de").taskbarFlash).toBe(true);
    expect(parseSettings(JSON.stringify({ language: "en" }), "de").taskbarFlash).toBe(true);
    // Stored by an older version, it is taken over once by the migration, never read here.
    expect(parseSettings(JSON.stringify({ taskbarFlash: false }), "de").taskbarFlash).toBe(true);
    expect(legacyNotices(JSON.stringify({ taskbarFlash: false })).taskbarFlash).toBe(false);
  });

  it("never reads or writes the notice settings in localStorage", () => {
    const stored = {
      restoreSessions: false,
      attentionEnabled: false,
      taskbarFlash: false,
      notifyDesktop: false,
      chime: false,
      quietHours: { enabled: true, from: 1380, to: 360 },
      dailyBudget: 12.5,
    };
    expect(noticesOf(parseSettings(JSON.stringify(stored), "de"))).toEqual(
      noticesOf(defaultSettings("de")),
    );
    expect(legacyNotices(JSON.stringify(stored))).toEqual(stored);
    const kept = withoutBackendKeys({ ...defaultSettings("de"), ...stored });
    for (const key of NOTICE_KEYS) expect(kept).not.toHaveProperty(key);
    expect(legacyNotices(null)).toEqual(noticesOf(defaultSettings("de")));
    expect(legacyNotices("{broken")).toEqual(noticesOf(defaultSettings("de")));
    expect(legacyNotices(JSON.stringify({ chime: "no", quietHours: 5 }))).toEqual(
      noticesOf(defaultSettings("de")),
    );
  });

  it("notifies by default and keeps running in the notification area only when chosen", () => {
    const old = parseSettings(JSON.stringify({ language: "en" }), "de");
    expect(old.notifyDesktop).toBe(true);
    expect(old.keepInTray).toBe(false);
    expect(defaultSettings("de").notifyDesktop).toBe(true);
    expect(defaultSettings("de").keepInTray).toBe(false);
  });

  it("shows the sessions' resources by default, also for older settings", () => {
    expect(defaultSettings("de").showResources).toBe(true);
    expect(parseSettings(JSON.stringify({ language: "en" }), "de").showResources).toBe(true);
    expect(parseSettings(JSON.stringify({ showResources: false }), "de").showResources).toBe(false);
  });

  it("saves energy on battery by default, also for older settings", () => {
    expect(defaultSettings("de").powerSaving).toBe(true);
    expect(parseSettings(JSON.stringify({ language: "en" }), "de").powerSaving).toBe(true);
    expect(parseSettings(JSON.stringify({ powerSaving: false }), "de").powerSaving).toBe(false);
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

  it("shows the crew column by default, also for older settings", () => {
    expect(defaultSettings("de").crewColumn).toBe(true);
    expect(parseSettings(JSON.stringify({ language: "en" }), "de").crewColumn).toBe(true);
    expect(parseSettings(JSON.stringify({ crewColumn: false }), "de").crewColumn).toBe(false);
  });

  it("never reads the security switches from localStorage", () => {
    // The panel and file viewer windows share this storage: whatever stands there is ignored.
    const stored = JSON.stringify({
      claimsEnabled: false,
      gateEnabled: false,
      commandGuardEnabled: false,
    });
    const settings = parseSettings(stored, "de");
    expect(settings.claimsEnabled).toBe(true);
    expect(settings.gateEnabled).toBe(true);
    expect(settings.commandGuardEnabled).toBe(true);
    expect(defaultSettings("de").claimsEnabled).toBe(true);
    expect(defaultSettings("de").gateEnabled).toBe(true);
    expect(defaultSettings("de").commandGuardEnabled).toBe(true);
  });

  it("never writes the security switches or the protected settings to localStorage", () => {
    const settings = {
      ...defaultSettings("de"),
      gateEnabled: false,
      commandGuardEnabled: false,
      dictationEnabled: true,
    };
    const stored = withoutBackendKeys(settings);
    for (const key of [...SECURITY_SWITCH_KEYS, ...PROTECTED_KEYS, ...NOTICE_KEYS, DICTATION_KEY]) {
      expect(key in stored).toBe(false);
      expect(BACKEND_KEYS).toContain(key);
    }
    expect(stored.language).toBe("de");
  });

  it("reads the switches an older version stored, once, for the migration", () => {
    expect(legacySwitches(JSON.stringify({ gateEnabled: false, claimsEnabled: "no" }))).toEqual({
      claimsEnabled: true,
      gateEnabled: false,
      commandGuardEnabled: true,
    });
    const on = { claimsEnabled: true, gateEnabled: true, commandGuardEnabled: true };
    expect(legacySwitches(null)).toEqual(on);
    expect(legacySwitches("{broken")).toEqual(on);
    expect(legacySwitches("[false]")).toEqual(on);
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
    expect(parsed.rowLayout).toEqual(DEFAULT_ROW_LAYOUT);
  });

  it("turns a row style of an earlier version into its layout once", () => {
    for (const preset of ROW_PRESETS) {
      const parsed = parseSettings(JSON.stringify({ rowStyle: preset }), "de");
      expect(parsed.rowLayout).toEqual(presetLayout(preset));
      // Saved again, only the layout is stored.
      const saved = withoutBackendKeys(parsed);
      expect(saved).not.toHaveProperty("rowStyle");
      expect(parseSettings(JSON.stringify(saved), "de").rowLayout).toEqual(presetLayout(preset));
    }
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
    // The switch itself lives in the backend (mainStore.ts); the old value migrates once.
    expect(parsed.chime).toBe(true);
    expect(legacyNotices('{"chime":false}').chime).toBe(false);
    expect(parsed.chimeVolume).toBe(60);
    expect(parseSettings('{"chimeVolume":0}', "de").chimeVolume).toBe(CHIME_VOLUME.min);
    expect(parseSettings('{"chimeVolume":900}', "de").chimeVolume).toBe(CHIME_VOLUME.max);
    expect(parseSettings('{"chimeVolume":"loud"}', "de").chimeVolume).toBe(CHIME_VOLUME.default);
  });
});
