import { describe, expect, it } from "vitest";

import { attentionEdges, attentionTint, mixColors, textOnColor } from "./attentionLook";
import { CREAM as DEFAULT_ATTENTION_COLOR, LOOK_PRESETS as ATTENTION_PRESETS } from "./stateLook";
import { contrastRatio } from "./theme";

describe("textOnColor", () => {
  it("puts dark ink on every preset and white on dark colors", () => {
    for (const preset of ATTENTION_PRESETS) expect(textOnColor(preset.color)).toBe("#1a1d21");
    expect(textOnColor("#123456")).toBe("#ffffff");
    expect(textOnColor("#000000")).toBe("#ffffff");
    expect(textOnColor("#ffffff")).toBe("#1a1d21");
  });

  it("falls back to the default color for invalid input", () => {
    expect(textOnColor("nonsense")).toBe(textOnColor(DEFAULT_ATTENTION_COLOR));
  });

  it("reaches 4.5 : 1 on every preset and picks the better side on any color", () => {
    for (const preset of ATTENTION_PRESETS) {
      expect(contrastRatio(preset.color, textOnColor(preset.color))).toBeGreaterThanOrEqual(4.5);
    }
    for (const color of ["#7f7f7f", "#123456", "#ff0000", "#00ff00"]) {
      const other = textOnColor(color) === "#ffffff" ? "#1a1d21" : "#ffffff";
      expect(contrastRatio(color, textOnColor(color))).toBeGreaterThanOrEqual(
        contrastRatio(color, other),
      );
    }
  });
});

describe("mixColors", () => {
  it("mixes like color-mix in srgb", () => {
    expect(mixColors("#ffffff", 50, "#000000")).toBe("#808080");
    expect(mixColors("#123456", 100, "#000000")).toBe("#123456");
    expect(mixColors("#123456", 0, "#abcdef")).toBe("#abcdef");
  });
});

describe("attentionTint", () => {
  it("gives the light presets a strong peak in the light theme", () => {
    expect(attentionTint("#f2e3c6", "light").high).toBe(72);
    expect(attentionTint("#ffffff", "light").high).toBe(72);
  });

  it("weakens dark colors in the light theme so the text stays readable", () => {
    const tint = attentionTint("#123456", "light");
    expect(tint.high).toBeLessThan(40);
    const background = mixColors("#123456", tint.high, "#f3f5f8");
    expect(contrastRatio(background, "#1f2328")).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(background, "#59636e")).toBeGreaterThanOrEqual(3);
  });

  it("keeps the dark theme's muted text readable at the peak", () => {
    for (const preset of ATTENTION_PRESETS) {
      const tint = attentionTint(preset.color, "dark");
      expect(tint.high).toBeGreaterThan(17);
      const background = mixColors(preset.color, tint.high, "#151a21");
      expect(contrastRatio(background, "#8592a3")).toBeGreaterThanOrEqual(3);
    }
  });

  it("breathes from about a third of the peak", () => {
    expect(attentionTint("#f2e3c6", "light")).toEqual({ low: 22, high: 72 });
  });

  it("never goes below the minimum", () => {
    expect(attentionTint("#000000", "light").high).toBeGreaterThanOrEqual(20);
    expect(attentionTint("not a color", "dark")).toEqual(
      attentionTint(DEFAULT_ATTENTION_COLOR, "dark"),
    );
  });
});

describe("attentionEdges", () => {
  const panels = { dark: "#151a21", light: "#f3f5f8" } as const;

  it("keeps 3 : 1 against the sidebar for any color in both themes", () => {
    const colors = [...ATTENTION_PRESETS.map((p) => p.color), "#123456", "#202020", "#ffffff"];
    for (const theme of ["dark", "light"] as const) {
      for (const color of colors) {
        const edges = attentionEdges(color, theme);
        for (const part of [edges.strong, edges.ring, edges.chipBg]) {
          expect(contrastRatio(part, panels[theme]), `${theme} ${color}`).toBeGreaterThanOrEqual(3);
        }
      }
    }
  });

  it("leaves bright colors unchanged in the dark theme and lightens dark ones", () => {
    expect(attentionEdges("#f2e3c6", "dark").strong).toBe("#f2e3c6");
    const lifted = attentionEdges("#123456", "dark").strong;
    expect(lifted).not.toBe("#123456");
    expect(contrastRatio(lifted, "#ffffff")).toBeLessThan(contrastRatio("#123456", "#ffffff"));
  });

  it("darkens in the light theme as before", () => {
    expect(attentionEdges("#f2e3c6", "light").strong).toBe(mixColors("#000000", 45, "#f2e3c6"));
  });

  it("gives text in the state color 4.5 : 1 against the sidebar", () => {
    const colors = [...ATTENTION_PRESETS.map((p) => p.color), "#3060c0", "#202020", "#ffffff"];
    for (const theme of ["dark", "light"] as const) {
      for (const color of colors) {
        const { text } = attentionEdges(color, theme);
        expect(contrastRatio(text, panels[theme]), `${theme} ${color}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it("puts readable text on the chip", () => {
    for (const theme of ["dark", "light"] as const) {
      const edges = attentionEdges("#123456", theme);
      expect(edges.chipFg).toBe(textOnColor(edges.chipBg));
    }
  });
});
