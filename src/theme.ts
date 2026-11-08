// Light and dark: resolving the setting, and the terminal colors. The colors of the rest of the
// UI live as tokens in styles.css (`:root` dark, `:root[data-theme="light"]` light).

import type { ITheme } from "@xterm/xterm";

export type Theme = "light" | "dark";
export type ThemeSetting = "system" | Theme;

export const THEME_SETTINGS: readonly ThemeSetting[] = ["system", "light", "dark"];

export function isThemeSetting(value: unknown): value is ThemeSetting {
  return value === "system" || value === "light" || value === "dark";
}

/** "System" follows `prefers-color-scheme`; without a value, dark applies. */
export function resolveTheme(setting: ThemeSetting, systemDark: boolean): Theme {
  if (setting !== "system") return setting;
  return systemDark ? "dark" : "light";
}

/** Toggle in the sidebar: always the opposite of what is currently shown. */
export function toggledTheme(current: Theme): Theme {
  return current === "dark" ? "light" : "dark";
}

export const TERMINAL_THEMES: Record<Theme, Required<Pick<ITheme, TerminalColor>>> = {
  dark: {
    background: "#0e1116",
    foreground: "#d6dde6",
    cursor: "#5aa9ff",
    cursorAccent: "#0e1116",
    selectionBackground: "#2b4a6f",
    black: "#1b212a",
    red: "#f47067",
    green: "#57ab5a",
    yellow: "#c69026",
    blue: "#539bf5",
    magenta: "#b083f0",
    cyan: "#39c5cf",
    white: "#adbac7",
    brightBlack: "#636e7b",
    brightRed: "#ff938a",
    brightGreen: "#6bc46d",
    brightYellow: "#daaa3f",
    brightBlue: "#6cb6ff",
    brightMagenta: "#dcbdfb",
    brightCyan: "#56d4dd",
    brightWhite: "#f0f6fc",
  },
  // Readable on white: yellow and white are darkened, all text colors reach 4.5 : 1.
  light: {
    background: "#ffffff",
    foreground: "#1f2328",
    cursor: "#0969da",
    cursorAccent: "#ffffff",
    selectionBackground: "#b6d7ff",
    black: "#24292f",
    red: "#cf222e",
    green: "#116329",
    yellow: "#7d4e00",
    blue: "#0969da",
    magenta: "#8250df",
    cyan: "#1b7c83",
    white: "#59636e",
    brightBlack: "#57606a",
    brightRed: "#a40e26",
    brightGreen: "#1a7f37",
    brightYellow: "#633c01",
    brightBlue: "#0550ae",
    brightMagenta: "#6639ba",
    brightCyan: "#136061",
    brightWhite: "#6e7781",
  },
};

type TerminalColor =
  "background" | "foreground" | "cursor" | "cursorAccent" | "selectionBackground" | AnsiColor;

export type AnsiColor =
  | "black"
  | "red"
  | "green"
  | "yellow"
  | "blue"
  | "magenta"
  | "cyan"
  | "white"
  | "brightBlack"
  | "brightRed"
  | "brightGreen"
  | "brightYellow"
  | "brightBlue"
  | "brightMagenta"
  | "brightCyan"
  | "brightWhite";

/**
 * Agent CLIs often set fixed truecolor colors meant for dark terminals. In light mode xterm raises
 * such text to at least 4.5 : 1; in dark mode everything stays as the program wants it.
 */
export const MINIMUM_CONTRAST: Record<Theme, number> = { dark: 1, light: 4.5 };

/** WCAG 2 contrast ratio for two `#rrggbb` colors. */
export function contrastRatio(a: string, b: string): number {
  const [lighter, darker] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return ((lighter ?? 0) + 0.05) / ((darker ?? 0) + 0.05);
}

function luminance(color: string): number {
  const match = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(color);
  if (!match) return 0;
  const [r, g, b] = match.slice(1).map((hex) => {
    const channel = parseInt(hex, 16) / 255;
    return channel <= 0.03928 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * (r ?? 0) + 0.7152 * (g ?? 0) + 0.0722 * (b ?? 0);
}

const DARK_QUERY = "(prefers-color-scheme: dark)";

/** Current system mode; without matchMedia (tests, old WebViews) dark applies. */
export function systemPrefersDark(): boolean {
  return typeof window.matchMedia === "function" ? window.matchMedia(DARK_QUERY).matches : true;
}

/** For useSyncExternalStore: reports changes of the system mode at runtime. */
export function subscribeSystemTheme(onChange: () => void): () => void {
  if (typeof window.matchMedia !== "function") return () => undefined;
  const query = window.matchMedia(DARK_QUERY);
  query.addEventListener("change", onChange);
  return () => {
    query.removeEventListener("change", onChange);
  };
}

/** Sets the tokens from styles.css and `color-scheme` (scrollbars, form controls). */
export function applyDocumentTheme(theme: Theme): void {
  const root = document.documentElement;
  root.dataset.theme = theme;
  root.style.colorScheme = theme;
}
