import { describe, expect, it } from "vitest";

import {
  contrastRatio,
  isThemeSetting,
  MINIMUM_CONTRAST,
  resolveTheme,
  TERMINAL_THEMES,
  toggledTheme,
  type AnsiColor,
} from "./theme";

const ANSI: readonly AnsiColor[] = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
  "brightBlack",
  "brightRed",
  "brightGreen",
  "brightYellow",
  "brightBlue",
  "brightMagenta",
  "brightCyan",
  "brightWhite",
];

describe("resolveTheme", () => {
  it("system follows the operating system", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
  });

  it("a fixed choice applies regardless of the system", () => {
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });
});

describe("toggledTheme", () => {
  it("switches to the opposite of the visible mode", () => {
    expect(toggledTheme("dark")).toBe("light");
    expect(toggledTheme("light")).toBe("dark");
  });
});

describe("isThemeSetting", () => {
  it("knows only system, light and dark", () => {
    expect(["system", "light", "dark"].every(isThemeSetting)).toBe(true);
    for (const value of ["auto", "", null, 1, "Light"]) expect(isThemeSetting(value)).toBe(false);
  });
});

describe("contrastRatio", () => {
  it("computes according to WCAG", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 5);
    expect(contrastRatio("#ffffff", "#ffffff")).toBeCloseTo(1, 5);
    expect(contrastRatio("#777777", "#ffffff")).toBeCloseTo(4.48, 1);
  });
});

describe("terminal colors", () => {
  it("light theme text colors are readable on white (AA)", () => {
    const theme = TERMINAL_THEMES.light;
    expect(contrastRatio(theme.foreground, theme.background)).toBeGreaterThanOrEqual(7);
    for (const color of ANSI) {
      expect(contrastRatio(theme[color], theme.background), color).toBeGreaterThanOrEqual(4.5);
    }
  });

  it("dark theme text colors are readable, except black", () => {
    const theme = TERMINAL_THEMES.dark;
    expect(contrastRatio(theme.foreground, theme.background)).toBeGreaterThanOrEqual(7);
    for (const color of ANSI.filter((c) => c !== "black")) {
      expect(contrastRatio(theme[color], theme.background), color).toBeGreaterThanOrEqual(3);
    }
  });

  it("raises the minimum contrast only in light mode", () => {
    expect(MINIMUM_CONTRAST.light).toBeGreaterThanOrEqual(4.5);
    expect(MINIMUM_CONTRAST.dark).toBe(1);
  });
});
