// App settings (language, appearance, attention, program details, updates).
// Layout and font size live separately in layout.ts. Stored per machine in localStorage; if storage
// is missing or its content is invalid, the defaults apply.

import { clamp } from "./layout";
import { isLanguage, type Language } from "./i18n";
import { DEFAULT_PROGRAM_PREFS, parseProgramPrefs, type ProgramPrefs } from "./programPrefs";
import { defaultSnippets, parseSnippets, type Snippet } from "./snippets";
import {
  DEFAULT_TERMINAL_LOOK,
  isColorScheme,
  isCursorStyle,
  isTerminalFont,
  type ColorScheme,
  type CursorStyle,
  type TerminalFont,
} from "./terminalLook";
import { isThemeSetting, type ThemeSetting } from "./theme";

const STORAGE_KEY = "hangar.settings";

export const QUIET_SECONDS = { min: 1, max: 30, default: 3 } as const;
/**
 * Scrollback per session: xterm holds this many lines and this many are saved.
 * Every line costs memory (10,000 lines are about 34 MB per terminal), hence 5000 as the default.
 */
export const SCROLLBACK_LINES = { min: 500, max: 10_000, step: 500, default: 5000 } as const;

export const ATTENTION_PRESETS = [
  { id: "cream", color: "#f2e3c6" },
  { id: "amber", color: "#f0b429" },
  // Cooler than the green of "running" (--ok), so waiting and running are not confused.
  { id: "mint", color: "#9ee8dc" },
  { id: "sky", color: "#8cc8ff" },
] as const;

export type AttentionPreset = (typeof ATTENTION_PRESETS)[number];

export const DEFAULT_ATTENTION_COLOR = ATTENTION_PRESETS[0].color;

/** Former mint default; anyone who picked it gets the new one (same choice, easier to tell apart). */
const OLD_MINT = "#7ee0b5";

/** Stored attention color, with old defaults lifted to their new version. */
export function migrateAttentionColor(color: string): string {
  return color === OLD_MINT
    ? (ATTENTION_PRESETS.find((p) => p.id === "mint")?.color ?? color)
    : color;
}

export interface Settings {
  language: Language;
  /** Appearance; "system" follows the operating system, also at runtime. */
  theme: ThemeSetting;
  attentionEnabled: boolean;
  /** Always lowercase `#rrggbb`, just as `<input type="color">` returns it. */
  attentionColor: string;
  quietSeconds: number;
  /** The taskbar icon flashes when a session starts waiting in the background (taskbarFlash.ts). */
  taskbarFlash: boolean;
  /** Off: Hangar launches no programs to read their version. */
  showVersions: boolean;
  /** Off: Hangar reads no program icons and shows letter badges. */
  showIcons: boolean;
  autoUpdate: boolean;
  /** Off: Hangar does not ask npm, PyPI and GitHub for new versions of the programs. */
  checkProgramUpdates: boolean;
  /** Off: nothing is remembered on exit and nothing restored on start. */
  restoreSessions: boolean;
  /** Lines of scrollback per session, in the terminal and when saving. */
  scrollbackLines: number;
  /** Order, visibility and icons of the terminals and agents (programPrefs.ts). */
  programs: ProgramPrefs;
  /** Terminal colors; every scheme follows the app theme (terminalLook.ts). */
  terminalScheme: ColorScheme;
  /** "default" keeps the font stack Hangar used before. */
  terminalFont: TerminalFont;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
  /** Stored texts for agents (snippets.ts); seeded in the language of the first start. */
  snippets: Snippet[];
}

type Flag =
  | "attentionEnabled"
  | "taskbarFlash"
  | "showVersions"
  | "showIcons"
  | "autoUpdate"
  | "checkProgramUpdates"
  | "restoreSessions"
  | "cursorBlink";

export function defaultSettings(language: Language): Settings {
  return {
    language,
    theme: "system",
    attentionEnabled: true,
    attentionColor: DEFAULT_ATTENTION_COLOR,
    quietSeconds: QUIET_SECONDS.default,
    taskbarFlash: true,
    showVersions: true,
    showIcons: true,
    autoUpdate: true,
    checkProgramUpdates: true,
    restoreSessions: true,
    scrollbackLines: SCROLLBACK_LINES.default,
    programs: DEFAULT_PROGRAM_PREFS,
    terminalScheme: DEFAULT_TERMINAL_LOOK.scheme,
    terminalFont: DEFAULT_TERMINAL_LOOK.font,
    cursorStyle: DEFAULT_TERMINAL_LOOK.cursorStyle,
    cursorBlink: DEFAULT_TERMINAL_LOOK.cursorBlink,
    snippets: defaultSnippets(language),
  };
}

/** `#rgb` or `#rrggbb`, normalized to `#rrggbb`; anything else yields `null`. */
export function normalizeColor(value: unknown): string | null {
  if (typeof value !== "string") return null;
  const short = /^#([0-9a-f])([0-9a-f])([0-9a-f])$/i.exec(value);
  if (short) {
    const doubled = short.slice(1).map((digit) => digit + digit);
    return `#${doubled.join("")}`.toLowerCase();
  }
  return /^#[0-9a-f]{6}$/i.test(value) ? value.toLowerCase() : null;
}

export function clampQuietSeconds(value: number): number {
  return clamp(value, QUIET_SECONDS.min, QUIET_SECONDS.max);
}

/** Rounded to the step size and clamped. */
export function clampScrollbackLines(value: number): number {
  const stepped = Math.round(value / SCROLLBACK_LINES.step) * SCROLLBACK_LINES.step;
  return clamp(stepped, SCROLLBACK_LINES.min, SCROLLBACK_LINES.max);
}

/** Reads stored settings; missing or invalid values are replaced with defaults. */
export function parseSettings(text: string | null, fallbackLanguage: Language): Settings {
  const defaults = defaultSettings(fallbackLanguage);
  if (!text) return defaults;
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch {
    return defaults;
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return defaults;
  const get = (key: keyof Settings): unknown => (key in raw ? Reflect.get(raw, key) : undefined);
  const flag = (key: Flag): boolean => {
    const value = get(key);
    return typeof value === "boolean" ? value : defaults[key];
  };
  const language = get("language");
  const theme = get("theme");
  const quiet = get("quietSeconds");
  const lines = get("scrollbackLines");
  const scheme = get("terminalScheme");
  const font = get("terminalFont");
  const cursorStyle = get("cursorStyle");
  const chosenLanguage = isLanguage(language) ? language : defaults.language;
  return {
    language: chosenLanguage,
    theme: isThemeSetting(theme) ? theme : defaults.theme,
    attentionEnabled: flag("attentionEnabled"),
    attentionColor: migrateAttentionColor(
      normalizeColor(get("attentionColor")) ?? defaults.attentionColor,
    ),
    quietSeconds:
      typeof quiet === "number" && Number.isFinite(quiet)
        ? clampQuietSeconds(quiet)
        : defaults.quietSeconds,
    taskbarFlash: flag("taskbarFlash"),
    showVersions: flag("showVersions"),
    showIcons: flag("showIcons"),
    autoUpdate: flag("autoUpdate"),
    checkProgramUpdates: flag("checkProgramUpdates"),
    restoreSessions: flag("restoreSessions"),
    scrollbackLines:
      typeof lines === "number" && Number.isFinite(lines)
        ? clampScrollbackLines(lines)
        : defaults.scrollbackLines,
    programs: parseProgramPrefs(get("programs")),
    terminalScheme: isColorScheme(scheme) ? scheme : defaults.terminalScheme,
    terminalFont: isTerminalFont(font) ? font : defaults.terminalFont,
    cursorStyle: isCursorStyle(cursorStyle) ? cursorStyle : defaults.cursorStyle,
    cursorBlink: flag("cursorBlink"),
    snippets: parseSnippets(get("snippets"), chosenLanguage),
  };
}

export function loadSettings(fallbackLanguage: Language): Settings {
  try {
    return parseSettings(localStorage.getItem(STORAGE_KEY), fallbackLanguage);
  } catch {
    return defaultSettings(fallbackLanguage);
  }
}

/** Are settings already stored? Tells existing users apart from the very first start. */
export function hasStoredSettings(): boolean {
  try {
    return localStorage.getItem(STORAGE_KEY) !== null;
  } catch {
    return false;
  }
}

export function saveSettings(settings: Settings): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(settings));
  } catch {
    // Without storage, the defaults apply again on the next start.
  }
}
