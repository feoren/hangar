import { describe, expect, it } from "vitest";

import { contrastRatio, TERMINAL_THEMES } from "./theme";
import {
  COLOR_SCHEMES,
  fontStack,
  installedFonts,
  isColorScheme,
  isCursorStyle,
  isFontInstalled,
  isTerminalFont,
  schemeColors,
  searchColors,
  type MeasureText,
} from "./terminalLook";

/** Fake canvas: every installed font has its own width, everything else falls back. */
function fakeMeasure(installed: readonly string[]): MeasureText {
  return (cssFont) => {
    const match = /"([^"]+)"/.exec(cssFont);
    const fallback = cssFont.endsWith("serif") && !cssFont.endsWith("sans-serif") ? 500 : 400;
    if (match?.[1] !== undefined && installed.includes(match[1])) {
      return 300 + match[1].length;
    }
    return fallback;
  };
}

describe("font stack", () => {
  it("keeps the previous stack for the default", () => {
    expect(fontStack("default")).toBe(
      '"Cascadia Mono", "Cascadia Code", Consolas, "DejaVu Sans Mono", monospace',
    );
  });

  it("puts the chosen font first without repeating it", () => {
    expect(fontStack("Fira Code")).toBe(
      '"Fira Code", "Cascadia Mono", "Cascadia Code", Consolas, "DejaVu Sans Mono", monospace',
    );
    expect(fontStack("Cascadia Code")).toBe(
      '"Cascadia Code", "Cascadia Mono", Consolas, "DejaVu Sans Mono", monospace',
    );
  });
});

describe("installed fonts", () => {
  it("detects a font by a width different from the fallback", () => {
    const measure = fakeMeasure(["JetBrains Mono"]);
    expect(isFontInstalled("JetBrains Mono", measure)).toBe(true);
    expect(isFontInstalled("Fira Code", measure)).toBe(false);
  });

  it("detects a font that is itself the default monospace font", () => {
    // Consolas measures like monospace, but differs from serif.
    const measure: MeasureText = (cssFont) => {
      if (cssFont.includes('"Consolas"')) return 400;
      return cssFont.endsWith("serif") ? 500 : 400;
    };
    expect(isFontInstalled("Consolas", measure)).toBe(true);
  });

  it("lists only installed fonts in the offered order", () => {
    expect(installedFonts(fakeMeasure(["Consolas", "Cascadia Code"]))).toEqual([
      "Cascadia Code",
      "Consolas",
    ]);
    expect(installedFonts(fakeMeasure([]))).toEqual([]);
  });
});

describe("validation", () => {
  it("accepts only known values", () => {
    expect(COLOR_SCHEMES.every(isColorScheme)).toBe(true);
    expect(isColorScheme("dracula")).toBe(false);
    expect(isCursorStyle("bar")).toBe(true);
    expect(isCursorStyle("beam")).toBe(false);
    expect(isTerminalFont("default")).toBe(true);
    expect(isTerminalFont("Fira Code")).toBe(true);
    expect(isTerminalFont("Comic Sans MS")).toBe(false);
    expect(isTerminalFont(3)).toBe(false);
  });
});

describe("color schemes", () => {
  it("Hangar is the previous terminal theme", () => {
    expect(schemeColors("hangar", "dark")).toBe(TERMINAL_THEMES.dark);
    expect(schemeColors("hangar", "light")).toBe(TERMINAL_THEMES.light);
  });

  it("returns the same object for the same choice (no needless repaint)", () => {
    expect(schemeColors("catppuccin", "dark")).toBe(schemeColors("catppuccin", "dark"));
  });

  it("has readable text and a visible selection in every variant", () => {
    for (const scheme of COLOR_SCHEMES) {
      for (const theme of ["light", "dark"] as const) {
        const colors = schemeColors(scheme, theme);
        const at = `${scheme}/${theme}`;
        expect(contrastRatio(colors.foreground, colors.background), at).toBeGreaterThanOrEqual(4);
        expect(contrastRatio(colors.cursor, colors.background), at).toBeGreaterThanOrEqual(1.5);
        expect(colors.selectionBackground, at).not.toBe(colors.background);
      }
    }
  });

  it("keeps text readable on search hits", () => {
    for (const scheme of COLOR_SCHEMES) {
      for (const theme of ["light", "dark"] as const) {
        const colors = schemeColors(scheme, theme);
        const search = searchColors(scheme, theme);
        const at = `${scheme}/${theme}`;
        expect(search.match, at).toMatch(/^#[0-9a-f]{6}$/);
        expect(search.active, at).toMatch(/^#[0-9a-f]{6}$/);
        expect(contrastRatio(colors.foreground, search.active), at).toBeGreaterThanOrEqual(3);
        expect(contrastRatio(colors.foreground, search.match), at).toBeGreaterThanOrEqual(3);
      }
    }
  });
});
