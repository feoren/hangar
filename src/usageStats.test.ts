import { describe, expect, it, vi } from "vitest";

import catalogSource from "../src-tauri/src/telemetry_catalog.rs?raw";
import { DOT_ANIMATIONS } from "./dotAnimation";
import { LANGUAGES } from "./i18n";
import { defaultSettings } from "./settings";
import { COLOR_SCHEMES, CURSOR_STYLES, TERMINAL_FONTS } from "./terminalLook";
import { THEME_SETTINGS } from "./theme";
import { countFeature, countFeatureNow, parseUsageConsent, usageSettings } from "./usageStats";
import { VIEWER_THEMES } from "./viewerThemes";

const backend = vi.hoisted(() => ({
  count: vi.fn<(feature: string) => Promise<null>>(() => Promise.resolve(null)),
}));
vi.mock("./api", () => ({ api: { telemetryCount: backend.count } }));

/** The quoted names of one `pub const NAME` list in the Rust allowlists. */
function rustList(name: string): string[] {
  const start = catalogSource.indexOf(`pub const ${name}:`);
  const end = catalogSource.indexOf("];", start);
  return [...catalogSource.slice(start, end).matchAll(/"([^"]+)"/g)].map((match) => match[1] ?? "");
}

describe("usage statistics", () => {
  it("reads the stored answer and nothing else", () => {
    expect(parseUsageConsent(null)).toBeNull();
    expect(parseUsageConsent(true)).toBe(true);
    expect(parseUsageConsent(false)).toBe(false);
    expect(parseUsageConsent("yes")).toBeUndefined();
    expect(parseUsageConsent(undefined)).toBeUndefined();
  });

  it("takes the snapshot from the settings and the layout", () => {
    const settings = { ...defaultSettings("de"), theme: "dark" as const, restoreSessions: false };
    expect(usageSettings(settings, { fontSize: 15, collapsed: true }, true)).toEqual({
      language: "de",
      theme: "dark",
      terminalScheme: "hangar",
      terminalFont: "default",
      terminalFontSize: 15,
      cursorStyle: "block",
      viewerTheme: "meridian",
      dotAnimation: "bounce",
      attention: true,
      restoreSessions: false,
      sidePanelDetached: true,
      sidebarCollapsed: true,
    });
  });

  // A new scheme, font or language of the app needs its row in the Rust allowlists, or every
  // report would name it "other".
  it("has every value the app offers in the backend's allowlists", () => {
    const schemes = rustList("TERMINAL_SCHEMES");
    for (const scheme of COLOR_SCHEMES) expect(schemes).toContain(scheme);
    const viewers = rustList("VIEWER_THEMES");
    for (const theme of VIEWER_THEMES) expect(viewers).toContain(theme);
    expect(rustList("TERMINAL_FONTS")).toEqual(["default", ...TERMINAL_FONTS]);
    expect(rustList("CURSOR_STYLES")).toEqual([...CURSOR_STYLES]);
    expect(rustList("DOT_ANIMATIONS")).toEqual([...DOT_ANIMATIONS]);
    expect(rustList("LANGUAGES")).toEqual([...LANGUAGES]);
    expect(rustList("THEMES")).toEqual([...THEME_SETTINGS]);
  });

  it("counts a feature without ever failing the action", () => {
    countFeature("broadcastSent");
    expect(backend.count).toHaveBeenCalledWith("broadcastSent");
    backend.count.mockImplementationOnce(() => {
      throw new Error("no backend");
    });
    expect(() => {
      countFeature("pathMentioned");
    }).not.toThrow();
    backend.count.mockImplementationOnce(() => Promise.reject(new Error("refused")));
    expect(() => {
      countFeature("folderRevealed");
    }).not.toThrow();
  });

  it("waits for an update's count and never rejects", async () => {
    await countFeatureNow("updateInstalled");
    expect(backend.count).toHaveBeenLastCalledWith("updateInstalled");
    backend.count.mockImplementationOnce(() => Promise.reject(new Error("refused")));
    await expect(countFeatureNow("updateInstalled")).resolves.toBeUndefined();
  });
});
