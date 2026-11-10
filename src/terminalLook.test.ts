import { describe, expect, it } from "vitest";

import { contrastRatio, TERMINAL_THEMES } from "./theme";
import {
  BUNDLED_FONTS,
  COLOR_SCHEMES,
  createFontLoader,
  fontChoices,
  fontLoadQueries,
  fontStack,
  installedFonts,
  isColorScheme,
  isCursorStyle,
  isFontInstalled,
  isBundledFont,
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

describe("font choices", () => {
  it("offers the default, every bundled font and the installed system fonts", () => {
    expect(fontChoices(["Consolas"], "default")).toEqual([
      "default",
      "JetBrains Mono",
      "Fira Code",
      "Geist Mono",
      "Hack",
      "Consolas",
    ]);
  });

  it("keeps a stored font listed that is not installed here", () => {
    expect(fontChoices([], "Cascadia Code").at(-1)).toBe("Cascadia Code");
    expect(
      fontChoices(["Cascadia Code"], "Cascadia Code").filter((f) => f === "Cascadia Code"),
    ).toHaveLength(1);
  });

  it("never checks bundled fonts for installation", () => {
    const asked: string[] = [];
    installedFonts((cssFont) => {
      asked.push(cssFont);
      return 400;
    });
    expect(asked.some((cssFont) => BUNDLED_FONTS.some((font) => cssFont.includes(font)))).toBe(
      false,
    );
  });

  it("keeps saved fonts from earlier versions valid", () => {
    for (const font of [
      "Cascadia Mono",
      "Cascadia Code",
      "JetBrains Mono",
      "Fira Code",
      "Consolas",
    ]) {
      expect(isTerminalFont(font), font).toBe(true);
    }
  });
});

describe("font loading", () => {
  it("loads regular and bold of bundled fonts only", () => {
    expect(fontLoadQueries("Hack")).toEqual(['16px "Hack"', 'bold 16px "Hack"']);
    expect(fontLoadQueries("Consolas")).toEqual([]);
    expect(fontLoadQueries("default")).toEqual([]);
    expect(isBundledFont("Geist Mono")).toBe(true);
    expect(isBundledFont("Cascadia Mono")).toBe(false);
  });

  it("is ready at once for fonts that need no loading", () => {
    const loader = createFontLoader(() => Promise.reject(new Error("not expected")));
    expect(loader.isReady("default")).toBe(true);
    expect(loader.isReady("Consolas")).toBe(true);
  });

  it("becomes ready after loading and loads each font once", async () => {
    const queries: string[] = [];
    const loader = createFontLoader((query) => {
      queries.push(query);
      return Promise.resolve([]);
    });
    expect(loader.isReady("JetBrains Mono")).toBe(false);
    await Promise.all([loader.load("JetBrains Mono"), loader.load("JetBrains Mono")]);
    expect(loader.isReady("JetBrains Mono")).toBe(true);
    await loader.load("JetBrains Mono");
    expect(queries).toEqual(['16px "JetBrains Mono"', 'bold 16px "JetBrains Mono"']);
  });

  it("counts a failed font as ready, so terminals fall back instead of waiting", async () => {
    const loader = createFontLoader(() => Promise.reject(new Error("missing")));
    await expect(loader.load("Fira Code")).resolves.toBeUndefined();
    expect(loader.isReady("Fira Code")).toBe(true);
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
