// Terminal look: color scheme, font and cursor. Every scheme has a light and a dark variant and
// follows the app theme. The defaults keep the look Hangar had before these settings existed.

import type { ColorScheme } from "./terminalSchemes";

export {
  COLOR_SCHEMES,
  isColorScheme,
  parseColorScheme,
  schemeColors,
  schemePalette,
  searchColors,
  type ColorScheme,
  type SearchColors,
  type TerminalColors,
} from "./terminalSchemes";

export const CURSOR_STYLES = ["block", "bar", "underline"] as const;
export type CursorStyle = (typeof CURSOR_STYLES)[number];

/**
 * Fonts Hangar ships as woff2 (src/fonts.css), so they are always available. All of them cover box
 * drawing and block elements, which agent CLIs draw their frames with.
 */
export const BUNDLED_FONTS = [
  "JetBrains Mono",
  "Fira Code",
  "Geist Mono",
  "Hack",
  "Source Code Pro",
  "IBM Plex Mono",
  "Victor Mono",
] as const;
export type BundledFont = (typeof BUNDLED_FONTS)[number];

/**
 * Offered only when installed, in this order: Windows (Cascadia ships with 11), macOS, Linux. The
 * installation check hides every font this machine does not have.
 */
export const SYSTEM_FONTS = [
  "Cascadia Mono",
  "Cascadia Code",
  "Consolas",
  "Lucida Console",
  "SF Mono",
  "Menlo",
  "Monaco",
  "DejaVu Sans Mono",
  "Ubuntu Mono",
  "Noto Sans Mono",
  "Liberation Mono",
] as const;
export type SystemFont = (typeof SYSTEM_FONTS)[number];

/** Every font a setting may name. Saved names from older versions are all still in here. */
export const TERMINAL_FONTS = [...BUNDLED_FONTS, ...SYSTEM_FONTS] as const;
/** "default" is the stack of the Windows fonts, with the bundled Cascadia behind them. */
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

export function isCursorStyle(value: unknown): value is CursorStyle {
  return CURSOR_STYLES.some((style) => style === value);
}

export function isTerminalFont(value: unknown): value is TerminalFont {
  return value === "default" || TERMINAL_FONTS.some((font) => font === value);
}

export function isBundledFont(font: TerminalFont): font is BundledFont {
  return BUNDLED_FONTS.some((bundled) => bundled === font);
}

export function sameLook(a: TerminalLook, b: TerminalLook): boolean {
  return (
    a.scheme === b.scheme &&
    a.font === b.font &&
    a.cursorStyle === b.cursorStyle &&
    a.cursorBlink === b.cursorBlink
  );
}

/**
 * Cascadia Mono as Hangar ships it (fonts.css), under its own family name. Only WebKit (macOS,
 * Linux) gets it in the stack, after the Windows fonts. Chromium (Windows) keeps the stack without
 * it: there it would draw every glyph the installed Cascadia or Consolas lacks (Arabic, Braille,
 * U+276F) in place of the system fallback, with other widths.
 */
export const BUNDLED_DEFAULT_FONT = "Hangar Cascadia Mono";

/** The Windows fonts of the default stack; when one is installed, the bundled one is never used. */
const SYSTEM_DEFAULT_FONTS = ["Cascadia Mono", "Cascadia Code", "Consolas"] as const;

const FALLBACK_FONTS = ['"Cascadia Mono"', '"Cascadia Code"', "Consolas", '"DejaVu Sans Mono"'];

/** Whether the stacks name the bundled Cascadia; set once at startup (main.tsx) for WebKit. */
let bundledMono = false;

export function setBundledMonoFont(enabled: boolean): void {
  bundledMono = enabled;
}

export function bundledMonoFont(): boolean {
  return bundledMono;
}

function fallbackFonts(): string[] {
  if (!bundledMono) return FALLBACK_FONTS;
  const afterWindowsFonts = FALLBACK_FONTS.indexOf("Consolas") + 1;
  return [
    ...FALLBACK_FONTS.slice(0, afterWindowsFonts),
    `"${BUNDLED_DEFAULT_FONT}"`,
    ...FALLBACK_FONTS.slice(afterWindowsFonts),
  ];
}

/**
 * CSS font stack: the chosen font first, then the fallbacks that applied before. Deliberately no
 * color emoji font: the system fallback already draws emoji and VS16 sequences in color, while a
 * named emoji font would also take text-style symbols such as U+23FA (Claude Code's bullet), U+2714
 * or U+26A0 and draw them as wide color emoji in a single cell (measured in headless Edge).
 */
export function fontStack(font: TerminalFont): string {
  const first = font === "default" ? [] : [`"${font}"`];
  const rest = fallbackFonts().filter((name) => !first.includes(name));
  return [...first, ...rest, "monospace"].join(", ");
}

/**
 * Stack for the moment the chosen font is not loaded yet: the default stack without the bundled
 * Cascadia. It differs from the final stack of every font that has to load, so switching to that
 * makes xterm measure the cell size anew.
 */
export function waitingStack(): string {
  return [...FALLBACK_FONTS, "monospace"].join(", ");
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

/** Installed system fonts in the offered order; bundled fonts need no check. */
export function installedFonts(measure: MeasureText): SystemFont[] {
  return SYSTEM_FONTS.filter((font) => isFontInstalled(font, measure));
}

/**
 * Fonts in the picker: the default, every bundled font, then the installed system fonts. A stored
 * font that is missing on this machine stays listed, so the picker never hides the current value.
 */
export function fontChoices(
  installed: readonly SystemFont[],
  current: TerminalFont,
): TerminalFont[] {
  const choices: TerminalFont[] = ["default", ...BUNDLED_FONTS, ...installed];
  return choices.includes(current) ? choices : [...choices, current];
}

/**
 * Whether the default stack ends up at the bundled Cascadia: it is in the stack (WebKit) and none
 * of the Windows fonts before it is installed. Without a canvas to measure with, the answer is no,
 * which keeps the stack as it always worked.
 */
export function defaultUsesBundledFont(measure: MeasureText | null): boolean {
  if (!bundledMono || measure === null) return false;
  return !SYSTEM_DEFAULT_FONTS.some((font) => isFontInstalled(font, measure));
}

/**
 * `document.fonts.load` queries for the faces xterm draws with (regular and bold). Empty for fonts
 * that are not bundled: system fonts need no loading. The default needs the bundled Cascadia only
 * where it is the font actually drawn (`bundledDefault`).
 */
export function fontLoadQueries(font: TerminalFont, bundledDefault = false): string[] {
  if (font === "default") {
    return bundledDefault
      ? [`16px "${BUNDLED_DEFAULT_FONT}"`, `bold 16px "${BUNDLED_DEFAULT_FONT}"`]
      : [];
  }
  if (!isBundledFont(font)) return [];
  return [`16px "${font}"`, `bold 16px "${font}"`];
}

/** Loads web fonts; `document.fonts.load` in the app, a fake in tests. */
export type LoadFontFace = (query: string) => Promise<unknown>;

export interface FontLoader {
  /** True when xterm can measure the font right away (loaded, or nothing to load). */
  isReady(font: TerminalFont): boolean;
  /** Resolves once the font is ready; never rejects, a failed font falls back in the stack. */
  load(font: TerminalFont): Promise<void>;
}

/**
 * xterm measures the cell size once when the font is set. A web font that is still loading would
 * be measured in its fallback and keep wrong cell widths, so terminals wait for this loader.
 */
export function createFontLoader(
  loadFace: LoadFontFace,
  bundledDefault: () => boolean = () => false,
): FontLoader {
  const ready = new Set<TerminalFont>();
  const pending = new Map<TerminalFont, Promise<void>>();
  // Asked once, on first use: measuring needs the document and its installed fonts.
  let usesBundled: boolean | null = null;
  const queries = (font: TerminalFont): string[] => {
    if (font === "default") usesBundled ??= bundledDefault();
    return fontLoadQueries(font, usesBundled ?? false);
  };
  const isReady = (font: TerminalFont): boolean => queries(font).length === 0 || ready.has(font);
  const load = (font: TerminalFont): Promise<void> => {
    if (isReady(font)) return Promise.resolve();
    const running = pending.get(font);
    if (running !== undefined) return running;
    const promise = Promise.all(queries(font).map((query) => loadFace(query)))
      .catch(() => undefined)
      .then(() => {
        ready.add(font);
        pending.delete(font);
      });
    pending.set(font, promise);
    return promise;
  };
  return { isReady, load };
}

/** Shown in each picker option, in that font: shapes that tell coding fonts apart. */
export const FONT_SAMPLE = "=> != 0Oo 1lI {}";

const MEASURE_SAMPLE = "mmmmmmmmmmlli1WW@#{}";

/** Canvas based measurement for `installedFonts`; `null` without a 2D canvas (tests). */
export function canvasMeasure(): MeasureText | null {
  const context = document.createElement("canvas").getContext("2d");
  if (!context) return null;
  return (cssFont) => {
    context.font = cssFont;
    return context.measureText(MEASURE_SAMPLE).width;
  };
}
