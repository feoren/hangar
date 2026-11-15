import { describe, expect, it } from "vitest";

import { contrastRatio, TERMINAL_THEMES, type Theme } from "./theme";
import {
  COLOR_SCHEMES,
  isColorScheme,
  parseColorScheme,
  SCHEME_GROUPS,
  schemeColors,
  schemePalette,
  schemesInGroup,
  searchColors,
  type ColorScheme,
  type TerminalColors,
} from "./terminalSchemes";
import { VIEWER_THEMES } from "./viewerThemes";

const THEMES: readonly Theme[] = ["dark", "light"];

/** Every scheme in both variants, with a label for failure messages. */
const VARIANTS = COLOR_SCHEMES.flatMap((scheme) =>
  THEMES.map((theme) => ({
    at: `${scheme}/${theme}`,
    scheme,
    theme,
    colors: schemeColors(scheme, theme),
  })),
);

const ACCENTS = ["red", "green", "yellow", "blue", "magenta", "cyan"] as const;
const BRIGHT_ACCENTS = [
  "brightRed",
  "brightGreen",
  "brightYellow",
  "brightBlue",
  "brightMagenta",
  "brightCyan",
] as const;

function ratio(colors: TerminalColors, color: string): number {
  return contrastRatio(color, colors.background);
}

describe("scheme keys", () => {
  it("starts with Hangar and lists every scheme once", () => {
    expect(COLOR_SCHEMES[0]).toBe("hangar");
    expect(new Set(COLOR_SCHEMES).size).toBe(COLOR_SCHEMES.length);
  });

  it("puts every scheme in exactly one group, in picker order", () => {
    expect(SCHEME_GROUPS.flatMap((group) => schemesInGroup(group))).toEqual([...COLOR_SCHEMES]);
    expect(schemesInGroup("own")[0]).toBe("hangar");
  });

  it("offers a terminal scheme for every file viewer family", () => {
    for (const family of VIEWER_THEMES) {
      expect(isColorScheme(family), family).toBe(true);
    }
  });

  it("reads every current key as itself", () => {
    for (const key of COLOR_SCHEMES) expect(parseColorScheme(key), key).toBe(key);
  });

  it("moves every key released in 0.6.0 to its renamed scheme", () => {
    const renamed: Record<string, ColorScheme> = {
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
    };
    for (const [old, now] of Object.entries(renamed)) {
      expect(parseColorScheme(old), old).toBe(now);
      expect(isColorScheme(old), old).toBe(false);
    }
  });

  it("keeps the keys from before 0.6.0 on the palettes they always had", () => {
    expect(parseColorScheme("harbor")).toBe("harbor");
    expect(parseColorScheme("meridian")).toBe("meridian");
  });

  it("rejects unknown values", () => {
    for (const value of [
      "Harbor",
      "GitHub",
      "midnight",
      "toString",
      "__proto__",
      "",
      3,
      null,
      undefined,
    ]) {
      expect(parseColorScheme(value), String(value)).toBeNull();
    }
  });
});

describe("color schemes", () => {
  it("Hangar is the previous terminal theme", () => {
    expect(schemeColors("hangar", "dark")).toBe(TERMINAL_THEMES.dark);
    expect(schemeColors("hangar", "light")).toBe(TERMINAL_THEMES.light);
  });

  it("keeps the colors of the renamed schemes", () => {
    expect(schemeColors("harbor", "dark").background).toBe("#0d1117");
    expect(schemeColors("harbor", "light").yellow).toBe("#4d2d00");
    expect(schemeColors("meridian", "dark").red).toBe("#e05561");
    expect(schemeColors("meridian", "light").background).toBe("#fafafa");
    expect(schemeColors("fjord", "dark").background).toBe("#2e3440");
    expect(schemeColors("nocturne", "dark").background).toBe("#282a36");
  });

  it("draws Ember from the classic 2006 palette", () => {
    const ember = schemeColors("ember", "dark");
    expect(ember.background).toBe("#272822");
    expect(ember.foreground).toBe("#f8f8f2");
    expect(schemePalette(ember)).toEqual([
      "#f92672",
      "#a6e22e",
      "#e6db74",
      "#66d9ef",
      "#ae81ff",
      "#a1efe4",
    ]);
  });

  it("returns the same object for the same choice (no needless repaint)", () => {
    expect(schemeColors("nocturne", "dark")).toBe(schemeColors("nocturne", "dark"));
  });

  it("uses #rrggbb everywhere", () => {
    for (const { at, colors } of VARIANTS) {
      for (const [key, value] of Object.entries(colors)) {
        expect(value, `${at} ${key}`).toMatch(/^#[0-9a-f]{6}$/);
      }
    }
  });

  it("gives every scheme a distinct light and dark variant", () => {
    for (const scheme of COLOR_SCHEMES) {
      const dark = schemeColors(scheme, "dark");
      const light = schemeColors(scheme, "light");
      expect(contrastRatio(dark.background, "#000000"), scheme).toBeLessThan(
        contrastRatio(light.background, "#000000"),
      );
    }
  });
});

describe("contrast", () => {
  it("sets the foreground at 4.5 : 1 or more", () => {
    for (const { at, colors } of VARIANTS) {
      expect(ratio(colors, colors.foreground), at).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("keeps every accent readable as text, normal and bright (3 : 1)", () => {
    for (const { at, colors } of VARIANTS) {
      for (const accent of [...ACCENTS, ...BRIGHT_ACCENTS]) {
        expect(ratio(colors, colors[accent]), `${at} ${accent}`).toBeGreaterThanOrEqual(3);
      }
    }
  });

  it("keeps blue clearly readable on dark backgrounds (4 : 1)", () => {
    // Links, paths and prompts are blue, and dark blue is where terminal palettes fail most.
    for (const { at, colors, theme } of VARIANTS) {
      if (theme === "dark") expect(ratio(colors, colors.blue), at).toBeGreaterThanOrEqual(4);
    }
  });

  it("keeps the gray of dimmed text readable (3 : 1)", () => {
    // Agent CLIs print hints, timings and secondary lines in bright black.
    for (const { at, colors } of VARIANTS) {
      expect(ratio(colors, colors.brightBlack), at).toBeGreaterThanOrEqual(3);
    }
  });

  it("keeps selected text readable", () => {
    for (const { at, colors } of VARIANTS) {
      expect(
        contrastRatio(colors.foreground, colors.selectionBackground),
        at,
      ).toBeGreaterThanOrEqual(3);
      expect(colors.selectionBackground, at).not.toBe(colors.background);
    }
  });

  it("keeps the cursor visible and the character under it readable", () => {
    for (const { at, colors } of VARIANTS) {
      expect(ratio(colors, colors.cursor), at).toBeGreaterThanOrEqual(1.5);
      expect(colors.cursorAccent, at).toBe(colors.background);
    }
  });

  it("has no light-mode bright white that vanishes on the page", () => {
    for (const { at, colors, theme } of VARIANTS) {
      if (theme === "light") expect(ratio(colors, colors.brightWhite), at).toBeGreaterThan(1.5);
    }
  });
});

describe("preview and search", () => {
  it("previews six accents per scheme", () => {
    const colors = schemeColors("fjord", "dark");
    expect(schemePalette(colors)).toEqual(ACCENTS.map((accent) => colors[accent]));
  });

  it("keeps text readable on search hits", () => {
    for (const { at, colors, scheme, theme } of VARIANTS) {
      const search = searchColors(scheme, theme);
      expect(search.match, at).toMatch(/^#[0-9a-f]{6}$/);
      expect(search.active, at).toMatch(/^#[0-9a-f]{6}$/);
      expect(contrastRatio(colors.foreground, search.active), at).toBeGreaterThanOrEqual(3);
      expect(contrastRatio(colors.foreground, search.match), at).toBeGreaterThanOrEqual(3);
      expect(search.match, at).not.toBe(colors.background);
    }
  });
});
