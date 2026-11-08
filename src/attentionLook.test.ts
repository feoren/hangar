import { describe, expect, it } from "vitest";

import { attentionTint, mixColors, textOnColor } from "./attentionLook";
import { ATTENTION_PRESETS, DEFAULT_ATTENTION_COLOR } from "./settings";
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
