import { describe, expect, it } from "vitest";

import { contrastRatio } from "./theme";
import {
  DEFAULT_VIEWER_THEME,
  isViewerTheme,
  PREVIEW_LINES,
  TOKEN_KINDS,
  VIEWER_THEMES,
  viewerCssVariables,
  viewerPalette,
} from "./viewerThemes";

const MODES = ["dark", "light"] as const;
const COLOR = /^#[0-9a-f]{6}([0-9a-f]{2})?$/;

describe("viewer themes", () => {
  it("knows its keys and starts with One Dark Pro", () => {
    expect(DEFAULT_VIEWER_THEME).toBe("oneDarkPro");
    expect(VIEWER_THEMES[0]).toBe(DEFAULT_VIEWER_THEME);
    for (const theme of VIEWER_THEMES) expect(isViewerTheme(theme)).toBe(true);
    for (const wrong of ["atom", "OneDarkPro", "", null, 1])
      expect(isViewerTheme(wrong)).toBe(false);
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
    const variables = viewerCssVariables(viewerPalette("github", "light"));
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
