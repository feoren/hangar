// Terminal look: color scheme, font and cursor. Every scheme has a light and a dark variant and
// follows the app theme. The defaults keep the look Hangar had before these settings existed.

import type { ITheme } from "@xterm/xterm";

import { TERMINAL_THEMES, type AnsiColor, type Theme } from "./theme";

export const COLOR_SCHEMES = ["hangar", "catppuccin", "solarized"] as const;
export type ColorScheme = (typeof COLOR_SCHEMES)[number];

export const CURSOR_STYLES = ["block", "bar", "underline"] as const;
export type CursorStyle = (typeof CURSOR_STYLES)[number];

/** Offered fonts; the list only shows the installed ones. "default" is the previous font stack. */
export const TERMINAL_FONTS = [
  "Cascadia Mono",
  "Cascadia Code",
  "JetBrains Mono",
  "Fira Code",
  "Consolas",
] as const;
export type TerminalFont = "default" | (typeof TERMINAL_FONTS)[number];

export interface TerminalLook {
  scheme: ColorScheme;
  font: TerminalFont;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
}

export const DEFAULT_TERMINAL_LOOK: TerminalLook = {
  scheme: "hangar",
  font: "default",
  cursorStyle: "block",
  cursorBlink: true,
};

export function isColorScheme(value: unknown): value is ColorScheme {
  return COLOR_SCHEMES.some((scheme) => scheme === value);
}

export function isCursorStyle(value: unknown): value is CursorStyle {
  return CURSOR_STYLES.some((style) => style === value);
}

export function isTerminalFont(value: unknown): value is TerminalFont {
  return value === "default" || TERMINAL_FONTS.some((font) => font === value);
}

export function sameLook(a: TerminalLook, b: TerminalLook): boolean {
  return (
    a.scheme === b.scheme &&
    a.font === b.font &&
    a.cursorStyle === b.cursorStyle &&
    a.cursorBlink === b.cursorBlink
  );
}

const FALLBACK_FONTS = ['"Cascadia Mono"', '"Cascadia Code"', "Consolas", '"DejaVu Sans Mono"'];

/**
 * CSS font stack: the chosen font first, then the fallbacks that applied before. Deliberately no
 * color emoji font: the system fallback already draws emoji and VS16 sequences in color, while a
 * named emoji font would also take text-style symbols such as U+23FA (Claude Code's bullet), U+2714
 * or U+26A0 and draw them as wide color emoji in a single cell (measured in headless Edge).
 */
export function fontStack(font: TerminalFont): string {
  const first = font === "default" ? [] : [`"${font}"`];
  const rest = FALLBACK_FONTS.filter((name) => !first.includes(name));
  return [...first, ...rest, "monospace"].join(", ");
}

/** Measures the width of a sample text in the given CSS font (`font` shorthand). */
export type MeasureText = (cssFont: string) => number;

/**
 * A font is installed when text set in it differs in width from the generic fallback. Two
 * fallbacks, because a font can be the default monospace font itself (Consolas on Windows).
 * `document.fonts.check` cannot tell: it reports true for fonts that are not installed.
 */
export function isFontInstalled(font: string, measure: MeasureText): boolean {
  return ["monospace", "serif"].some(
    (fallback) => measure(`72px "${font}", ${fallback}`) !== measure(`72px ${fallback}`),
  );
}

export function installedFonts(measure: MeasureText): TerminalFont[] {
  return TERMINAL_FONTS.filter((font) => isFontInstalled(font, measure));
}

const SAMPLE = "mmmmmmmmmmlli1WW@#{}";

/** Canvas based measurement for `installedFonts`; `null` without a 2D canvas (tests). */
export function canvasMeasure(): MeasureText | null {
  const context = document.createElement("canvas").getContext("2d");
  if (!context) return null;
  return (cssFont) => {
    context.font = cssFont;
    return context.measureText(SAMPLE).width;
  };
}

export type TerminalColors = Required<
  Pick<ITheme, "background" | "foreground" | "cursor" | "cursorAccent" | "selectionBackground">
> &
  Record<AnsiColor, string>;

// Catppuccin (https://catppuccin.com/palette), terminal colors as in its Windows Terminal port.
const CATPPUCCIN: Record<Theme, TerminalColors> = {
  dark: {
    background: "#1e1e2e",
    foreground: "#cdd6f4",
    cursor: "#f5e0dc",
    cursorAccent: "#1e1e2e",
    selectionBackground: "#45475a",
    black: "#45475a",
    red: "#f38ba8",
    green: "#a6e3a1",
    yellow: "#f9e2af",
    blue: "#89b4fa",
    magenta: "#f5c2e7",
    cyan: "#94e2d5",
    white: "#bac2de",
    brightBlack: "#7f849c",
    brightRed: "#f38ba8",
    brightGreen: "#a6e3a1",
    brightYellow: "#f9e2af",
    brightBlue: "#89b4fa",
    brightMagenta: "#f5c2e7",
    brightCyan: "#94e2d5",
    brightWhite: "#a6adc8",
  },
  light: {
    background: "#eff1f5",
    foreground: "#4c4f69",
    cursor: "#dc8a78",
    cursorAccent: "#eff1f5",
    selectionBackground: "#ccd0da",
    black: "#5c5f77",
    red: "#d20f39",
    green: "#40a02b",
    yellow: "#df8e1d",
    blue: "#1e66f5",
    magenta: "#ea76cb",
    cyan: "#179299",
    white: "#acb0be",
    brightBlack: "#6c6f85",
    brightRed: "#d20f39",
    brightGreen: "#40a02b",
    brightYellow: "#df8e1d",
    brightBlue: "#1e66f5",
    brightMagenta: "#ea76cb",
    brightCyan: "#179299",
    brightWhite: "#bcc0cc",
  },
};

// Solarized (https://ethanschoonover.com/solarized). Unlike the original, the bright colors keep
// their hue: agent CLIs use them as accents, and the original grays made gray text invisible.
const SOLARIZED: Record<Theme, TerminalColors> = {
  dark: {
    background: "#002b36",
    foreground: "#93a1a1",
    cursor: "#93a1a1",
    cursorAccent: "#002b36",
    selectionBackground: "#174652",
    black: "#073642",
    red: "#dc322f",
    green: "#859900",
    yellow: "#b58900",
    blue: "#268bd2",
    magenta: "#d33682",
    cyan: "#2aa198",
    white: "#eee8d5",
    brightBlack: "#657b83",
    brightRed: "#cb4b16",
    brightGreen: "#859900",
    brightYellow: "#b58900",
    brightBlue: "#268bd2",
    brightMagenta: "#6c71c4",
    brightCyan: "#2aa198",
    brightWhite: "#fdf6e3",
  },
  light: {
    background: "#fdf6e3",
    foreground: "#586e75",
    cursor: "#586e75",
    cursorAccent: "#fdf6e3",
    selectionBackground: "#e6dfc9",
    black: "#073642",
    red: "#dc322f",
    green: "#859900",
    yellow: "#b58900",
    blue: "#268bd2",
    magenta: "#d33682",
    cyan: "#2aa198",
    white: "#93a1a1",
    brightBlack: "#657b83",
    brightRed: "#cb4b16",
    brightGreen: "#859900",
    brightYellow: "#b58900",
    brightBlue: "#268bd2",
    brightMagenta: "#6c71c4",
    brightCyan: "#2aa198",
    brightWhite: "#586e75",
  },
};

const SCHEMES: Record<ColorScheme, Record<Theme, TerminalColors>> = {
  hangar: TERMINAL_THEMES,
  catppuccin: CATPPUCCIN,
  solarized: SOLARIZED,
};

/** Always the same object per scheme and theme, so xterm only repaints on a real change. */
export function schemeColors(scheme: ColorScheme, theme: Theme): TerminalColors {
  return SCHEMES[scheme][theme];
}

/** Search hits: every match, and the current one. `#rrggbb` only (xterm's search addon). */
export interface SearchColors {
  match: string;
  active: string;
}

const SEARCH_COLORS: Record<ColorScheme, Record<Theme, SearchColors>> = {
  hangar: {
    dark: { match: "#4a3d14", active: "#8a6416" },
    light: { match: "#fff1b8", active: "#f5c84c" },
  },
  catppuccin: {
    dark: { match: "#4d4636", active: "#8a7346" },
    light: { match: "#f6e3bd", active: "#efc072" },
  },
  solarized: {
    dark: { match: "#2f4a2a", active: "#5c4d07" },
    light: { match: "#f0e2a8", active: "#e2c45c" },
  },
};

export function searchColors(scheme: ColorScheme, theme: Theme): SearchColors {
  return SEARCH_COLORS[scheme][theme];
}
