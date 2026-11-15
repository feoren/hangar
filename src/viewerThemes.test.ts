import { describe, expect, it } from "vitest";

import { contrastRatio } from "./theme";
import {
  DEFAULT_VIEWER_THEME,
  isViewerTheme,
  parseViewerTheme,
  PREVIEW_LINES,
  TOKEN_KINDS,
  VIEWER_THEMES,
  viewerCssVariables,
  viewerPalette,
  type ViewerTheme,
} from "./viewerThemes";

const MODES = ["dark", "light"] as const;
const COLOR = /^#[0-9a-f]{6}([0-9a-f]{2})?$/;

describe("viewer themes", () => {
  it("knows its keys and starts with Meridian", () => {
    expect(DEFAULT_VIEWER_THEME).toBe("meridian");
    expect(VIEWER_THEMES[0]).toBe(DEFAULT_VIEWER_THEME);
    for (const theme of VIEWER_THEMES) {
      expect(isViewerTheme(theme)).toBe(true);
      expect(parseViewerTheme(theme)).toBe(theme);
    }
    for (const wrong of ["atom", "OneDarkPro", "toString", "__proto__", "", null, 1]) {
      expect(isViewerTheme(wrong)).toBe(false);
      expect(parseViewerTheme(wrong)).toBeNull();
    }
  });

  it("moves every key released in 0.6.0 to its renamed family", () => {
    const renamed: Record<string, ViewerTheme> = {
      oneDarkPro: "meridian",
      github: "harbor",
      vscode: "studio",
      darcula: "anvil",
      solarized: "sundial",
      catppuccin: "velvet",
      monokai: "ember",
    };
    for (const [old, now] of Object.entries(renamed)) {
      expect(parseViewerTheme(old), old).toBe(now);
      expect(isViewerTheme(old), old).toBe(false);
    }
  });

  it("draws Ember from the classic 2006 palette", () => {
    const ember = viewerPalette("ember", "dark");
    expect(ember.background).toBe("#272822");
    expect(ember.tokens.keyword).toBe("#f92672");
    expect(ember.tokens.string).toBe("#e6db74");
    expect(ember.tokens.comment).toBe("#75715e");
  });

  it("has a dark and a light variant with a color for every part and every token", () => {
    for (const theme of VIEWER_THEMES) {
      const dark = viewerPalette(theme, "dark");
      const light = viewerPalette(theme, "light");
      expect(dark.background).not.toBe(light.background);
      for (const palette of [dark, light]) {
        const { tokens, ...parts } = palette;
        for (const color of Object.values(parts)) expect(color).toMatch(COLOR);
        expect(Object.keys(tokens).sort()).toEqual([...TOKEN_KINDS].sort());
        for (const color of Object.values(tokens)) expect(color).toMatch(COLOR);
      }
    }
  });

  it("keeps text and line numbers readable on the background", () => {
    for (const theme of VIEWER_THEMES) {
      for (const mode of MODES) {
        const palette = viewerPalette(theme, mode);
        expect(contrastRatio(palette.foreground, palette.background)).toBeGreaterThanOrEqual(4.5);
        expect(
          contrastRatio(palette.gutterActive, palette.gutterBackground),
        ).toBeGreaterThanOrEqual(3);
        // Line numbers stay as quiet as in the editors these themes come from, but visible.
        expect(contrastRatio(palette.gutterForeground, palette.gutterBackground)).toBeGreaterThan(
          1.8,
        );
      }
    }
  });

  it("dark variants are dark and light ones light", () => {
    for (const theme of VIEWER_THEMES) {
      const dark = viewerPalette(theme, "dark").background;
      const light = viewerPalette(theme, "light").background;
      expect(contrastRatio(dark, "#000000")).toBeLessThan(contrastRatio(light, "#000000"));
      expect(contrastRatio(light, "#000000")).toBeGreaterThan(12);
      expect(contrastRatio(dark, "#000000")).toBeLessThan(2);
    }
  });

  it("hands every color to CSS as a variable", () => {
    const variables = viewerCssVariables(viewerPalette("harbor", "light"));
    expect(variables["--viewer-bg"]).toBe("#ffffff");
    expect(variables["--viewer-tok-keyword"]).toBe("#cf222e");
    for (const kind of TOKEN_KINDS) expect(variables[`--viewer-tok-${kind}`]).toMatch(COLOR);
  });

  it("previews only known token kinds", () => {
    for (const line of PREVIEW_LINES) {
      for (const [kind, text] of line) {
        expect(text.length).toBeGreaterThan(0);
        if (kind !== null) expect(TOKEN_KINDS).toContain(kind);
      }
    }
  });
});
