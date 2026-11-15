// App settings (language, appearance, attention, program details, updates).
// Layout and font size live separately in layout.ts. Stored per machine in localStorage; if storage
// is missing or its content is invalid, the defaults apply. The security switches (claims, gate,
// command guard), the protected settings (paste warning, automatic updates, HANGAR.md,
// snippets), the notice settings (session restore, attention, taskbar flash, desktop
// notifications, chime, quiet hours) and whether the dictation key is on are the exception: the panel and file viewer windows share
// this localStorage, so the backend keeps them for the main window alone (mainStore.ts); they are
// neither read from nor written to localStorage here.

import { DEFAULT_DICTATION, parseDictationPrefs, type DictationPrefs } from "./dictation";
import { DEFAULT_DOT_ANIMATION, parseDotAnimation, type DotAnimation } from "./dotAnimation";
import { clamp } from "./layout";
import { isLanguage, type Language } from "./i18n";
import { DEFAULT_PROGRAM_PREFS, parseProgramPrefs, type ProgramPrefs } from "./programPrefs";
import { DEFAULT_ROW_LAYOUT, parseRowLayout, type RowLayout } from "./rowLayout";
import { defaultSnippets, parseSnippets, type Snippet } from "./snippets";
import {
  DEFAULT_TERMINAL_LOOK,
  parseColorScheme,
  isCursorStyle,
  isTerminalFont,
  type ColorScheme,
  type CursorStyle,
  type TerminalFont,
} from "./terminalLook";
import { DEFAULT_QUIET_HOURS, parseQuietHours, type QuietHours } from "./quietHours";
import { isSessionGrouping, type SessionGrouping } from "./sessionGroups";
import { clampStallMinutes, STALL_MINUTES } from "./stall";
import { DEFAULT_LOOKS, parseStateLooks, type StateLooks } from "./stateLook";
import { isThemeSetting, type ThemeSetting } from "./theme";
import { DEFAULT_VIEWER_THEME, parseViewerTheme, type ViewerTheme } from "./viewerThemes";

export const SETTINGS_STORAGE_KEY = "hangar.settings";

/** Settings the backend keeps (mainStore.ts), never localStorage. */
export const SECURITY_SWITCH_KEYS = [
  "claimsEnabled",
  "gateEnabled",
  "commandGuardEnabled",
] as const;

export type SecuritySwitchKey = (typeof SECURITY_SWITCH_KEYS)[number];

/** The security switches as the backend stores them (`main_store.rs`). */
export type SecuritySwitches = Record<SecuritySwitchKey, boolean>;

/**
 * Settings that decide what reaches an agent or the machine, kept by the backend (mainStore.ts):
 * a viewer that could write them could switch the paste warning or updates off, or plant a
 * snippet that is later pasted into an agent session.
 */
export const PROTECTED_KEYS = ["warnMultilinePaste", "autoUpdate", "hangarMd", "snippets"] as const;

export type ProtectedKey = (typeof PROTECTED_KEYS)[number];

/** The protected settings as the backend stores them (`main_store.rs`, `ProtectedSettings`). */
export type ProtectedSettings = Pick<Settings, ProtectedKey>;

/**
 * Settings that decide whether sessions come back and whether a session that waits for the
 * person shows itself, kept by the backend with the start preferences (mainStore.ts `MainPrefs`):
 * a viewer that could write them could silence the notices of an agent asking for approval, or
 * turn the restore off.
 */
export const NOTICE_KEYS = [
  "restoreSessions",
  "attentionEnabled",
  "taskbarFlash",
  "notifyDesktop",
  "chime",
  "quietHours",
] as const;

export type NoticeKey = (typeof NOTICE_KEYS)[number];

/** The notice settings as the backend stores them (`main_store.rs`, `MainPrefs`). */
export type NoticeSettings = Pick<Settings, NoticeKey>;

/**
 * Whether holding the dictation key opens the microphone, kept by the backend with the start
 * preferences (mainStore.ts `MainPrefs`): a viewer that could write it could turn the key on, so
 * that the next Ctrl+Shift+Space opens the microphone instead of reaching the program.
 */
export const DICTATION_KEY = "dictationEnabled";

/** Every setting the backend keeps and localStorage never holds. */
export const BACKEND_KEYS: readonly string[] = [
  ...SECURITY_SWITCH_KEYS,
  ...PROTECTED_KEYS,
  ...NOTICE_KEYS,
  DICTATION_KEY,
];

export const QUIET_SECONDS = { min: 1, max: 30, default: 3 } as const;
/**
 * Scrollback per session: xterm holds this many lines and this many are saved.
 * Every line costs memory (10,000 lines are about 34 MB per terminal), hence 5000 as the default.
 */
export const SCROLLBACK_LINES = { min: 500, max: 10_000, step: 500, default: 5000 } as const;

/** Chime volume in percent; quiet by default, a hint and not an alarm. */
export const CHIME_VOLUME = { min: 5, max: 100, step: 5, default: 40 } as const;

export interface Settings {
  language: Language;
  /** Appearance; "system" follows the operating system, also at runtime. */
  theme: ThemeSetting;
  attentionEnabled: boolean;
  /** Color and pulse of each session state in the sidebar (stateLook.ts). */
  looks: StateLooks;
  /** How the dots of a working session move (dotAnimation.ts). */
  dotAnimation: DotAnimation;
  /** How a session row in the sidebar lays out its facts: the row builder (rowLayout.ts). */
  rowLayout: RowLayout;
  quietSeconds: number;
  /**
   * Minutes without anything new on a working agent's screen before its row hints that it may be
   * stuck (stall.ts); 0 turns the hint off.
   */
  stallMinutes: number;
  /** The taskbar icon flashes when a session starts waiting in the background (taskbarFlash.ts). */
  taskbarFlash: boolean;
  /** Desktop notification when a session starts waiting in the background (desktopNotify.ts). */
  notifyDesktop: boolean;
  /** A soft chime when a session starts waiting out of sight (chime.ts). */
  chime: boolean;
  /** Chime volume in percent (CHIME_VOLUME). */
  chimeVolume: number;
  /** Daily window without taskbar flash, chime and desktop notifications (quietHours.ts). */
  quietHours: QuietHours;
  /**
   * CPU and memory of each session in its row's tooltip, and a mark on outliers (resources.ts).
   * Off: the window does not ask the backend at all.
   */
  showResources: boolean;
  /**
   * On battery, Hangar polls less often and stills the row pulses (power.ts). Only offered where
   * the backend can tell battery from AC; on AC it changes nothing.
   */
  powerSaving: boolean;
  /**
   * Closing the window only hides it; an icon in the notification area brings it back or quits
   * (tray.ts). Off: closing quits, as before.
   */
  keepInTray: boolean;
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
  /**
   * The sub-agent view shows each sub-agent's newest text. Off: the backend reads no assistant
   * text at all. The excerpts are never stored, whatever this says.
   */
  subAgentExcerpts: boolean;
  /**
   * The backend watches which files each session's agent changes and warns when two sessions
   * touch the same file or lines (claims.ts). Off: no watcher and no git call for it.
   */
  claimsEnabled: boolean;
  /**
   * Claude Code sessions started while this is on ask Hangar before editing a file another
   * session is changing (gate.ts). Only effective together with `claimsEnabled`.
   */
  gateEnabled: boolean;
  /**
   * While the gate is active, a team session's agent waits for the person before a shell command
   * that matches a rule of command-guard.toml (backend command_guard.rs). On by default; applies
   * to running sessions at once.
   */
  commandGuardEnabled: boolean;
  /**
   * Every agent start hands the agent HANGAR.md from Hangar's config folder as an extra
   * instruction, where its program takes one (backend `instructions.rs`). On by default: the
   * file starts as a comment only, which sends nothing until the user writes into it.
   */
  hangarMd: boolean;
  /**
   * An agent started in a fresh worktree first checks whether the base branch has new commits
   * upstream and asks before pulling them, where its program takes a first prompt.
   */
  worktreeUpstreamCheck: boolean;
  /**
   * Ctrl+V and Shift+Insert ask before text that would run line by line (no bracketed paste) or
   * hides characters (clipboardPaste.ts). On by default, against pastejacking.
   */
  warnMultilinePaste: boolean;
  /** Order, visibility and icons of the terminals and agents (programPrefs.ts). */
  programs: ProgramPrefs;
  /** Terminal colors; every scheme follows the app theme (terminalLook.ts). */
  terminalScheme: ColorScheme;
  /** "default" keeps the font stack Hangar used before. */
  terminalFont: TerminalFont;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
  /** Colors of the file viewer window; every theme follows the app theme (viewerThemes.ts). */
  viewerTheme: ViewerTheme;
  /** The sidebar groups the sessions by program or by project folder (sessionGroups.ts). */
  sessionGrouping: SessionGrouping;
  /** Stored texts for agents (snippets.ts); seeded in the language of the first start. */
  snippets: Snippet[];
  /**
   * Hold a key to dictate into the session (dictation.ts); off by default. Kept by the backend
   * (`DICTATION_KEY`), never in localStorage.
   */
  dictationEnabled: boolean;
  /**
   * Model and language of the dictation. Kept in localStorage: another window that rewrote them
   * could only pick the other pinned model or language. A model is downloaded only through the
   * main window's consent box (main-window commands).
   */
  dictation: DictationPrefs;
}

type Flag =
  | "attentionEnabled"
  | "taskbarFlash"
  | "notifyDesktop"
  | "chime"
  | "showResources"
  | "powerSaving"
  | "keepInTray"
  | "showVersions"
  | "showIcons"
  | "autoUpdate"
  | "checkProgramUpdates"
  | "restoreSessions"
  | "subAgentExcerpts"
  | "claimsEnabled"
  | "gateEnabled"
  | "commandGuardEnabled"
  | "hangarMd"
  | "worktreeUpstreamCheck"
  | "warnMultilinePaste"
  | "cursorBlink";

export function defaultSettings(language: Language): Settings {
  return {
    language,
    theme: "system",
    attentionEnabled: true,
    looks: DEFAULT_LOOKS,
    dotAnimation: DEFAULT_DOT_ANIMATION,
    rowLayout: DEFAULT_ROW_LAYOUT,
    quietSeconds: QUIET_SECONDS.default,
    stallMinutes: STALL_MINUTES.default,
    taskbarFlash: true,
    notifyDesktop: true,
    chime: true,
    chimeVolume: CHIME_VOLUME.default,
    quietHours: DEFAULT_QUIET_HOURS,
    showResources: true,
    powerSaving: true,
    keepInTray: false,
    showVersions: true,
    showIcons: true,
    autoUpdate: true,
    checkProgramUpdates: true,
    restoreSessions: true,
    scrollbackLines: SCROLLBACK_LINES.default,
    subAgentExcerpts: true,
    claimsEnabled: true,
    gateEnabled: true,
    commandGuardEnabled: true,
    hangarMd: true,
    worktreeUpstreamCheck: true,
    warnMultilinePaste: true,
    programs: DEFAULT_PROGRAM_PREFS,
    terminalScheme: DEFAULT_TERMINAL_LOOK.scheme,
    terminalFont: DEFAULT_TERMINAL_LOOK.font,
    cursorStyle: DEFAULT_TERMINAL_LOOK.cursorStyle,
    cursorBlink: DEFAULT_TERMINAL_LOOK.cursorBlink,
    viewerTheme: DEFAULT_VIEWER_THEME,
    sessionGrouping: "program",
    snippets: defaultSnippets(language),
    dictationEnabled: false,
    dictation: DEFAULT_DICTATION,
  };
}

/** Rounded to the step size and clamped. */
export function clampChimeVolume(value: number): number {
  const stepped = Math.round(value / CHIME_VOLUME.step) * CHIME_VOLUME.step;
  return clamp(stepped, CHIME_VOLUME.min, CHIME_VOLUME.max);
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
  const stall = get("stallMinutes");
  const lines = get("scrollbackLines");
  const volume = get("chimeVolume");
  const scheme = get("terminalScheme");
  const font = get("terminalFont");
  const cursorStyle = get("cursorStyle");
  const viewerTheme = get("viewerTheme");
  const grouping = get("sessionGrouping");
  const chosenLanguage = isLanguage(language) ? language : defaults.language;
  return {
    language: chosenLanguage,
    theme: isThemeSetting(theme) ? theme : defaults.theme,
    // Never from localStorage, like the notice settings below: the backend keeps them.
    attentionEnabled: defaults.attentionEnabled,
    // Older versions stored one attention color instead; it carries over to both waiting states.
    looks: parseStateLooks(get("looks"), Reflect.get(raw, "attentionColor")),
    dotAnimation: parseDotAnimation(get("dotAnimation")),
    // Versions before the row builder stored one of fourteen row styles; it becomes its preset
    // once, and the next save stores the layout instead.
    rowLayout: parseRowLayout(get("rowLayout"), Reflect.get(raw, "rowStyle")),
    quietSeconds:
      typeof quiet === "number" && Number.isFinite(quiet)
        ? clampQuietSeconds(quiet)
        : defaults.quietSeconds,
    stallMinutes:
      typeof stall === "number" && Number.isFinite(stall)
        ? clampStallMinutes(stall)
        : defaults.stallMinutes,
    taskbarFlash: defaults.taskbarFlash,
    notifyDesktop: defaults.notifyDesktop,
    chime: defaults.chime,
    chimeVolume:
      typeof volume === "number" && Number.isFinite(volume)
        ? clampChimeVolume(volume)
        : defaults.chimeVolume,
    quietHours: defaults.quietHours,
    showResources: flag("showResources"),
    powerSaving: flag("powerSaving"),
    keepInTray: flag("keepInTray"),
    showVersions: flag("showVersions"),
    showIcons: flag("showIcons"),
    // Never from localStorage, like the switches below: the backend keeps them (mainStore.ts).
    autoUpdate: defaults.autoUpdate,
    checkProgramUpdates: flag("checkProgramUpdates"),
    restoreSessions: defaults.restoreSessions,
    scrollbackLines:
      typeof lines === "number" && Number.isFinite(lines)
        ? clampScrollbackLines(lines)
        : defaults.scrollbackLines,
    subAgentExcerpts: flag("subAgentExcerpts"),
    // Never from localStorage: any window of this origin could have written them there.
    claimsEnabled: defaults.claimsEnabled,
    gateEnabled: defaults.gateEnabled,
    commandGuardEnabled: defaults.commandGuardEnabled,
    hangarMd: defaults.hangarMd,
    worktreeUpstreamCheck: flag("worktreeUpstreamCheck"),
    warnMultilinePaste: defaults.warnMultilinePaste,
    programs: parseProgramPrefs(get("programs")),
    terminalScheme: parseColorScheme(scheme) ?? defaults.terminalScheme,
    terminalFont: isTerminalFont(font) ? font : defaults.terminalFont,
    cursorStyle: isCursorStyle(cursorStyle) ? cursorStyle : defaults.cursorStyle,
    cursorBlink: flag("cursorBlink"),
    viewerTheme: parseViewerTheme(viewerTheme) ?? defaults.viewerTheme,
    sessionGrouping: isSessionGrouping(grouping) ? grouping : defaults.sessionGrouping,
    snippets: defaultSnippets(chosenLanguage),
    // Never from localStorage, like the switches: the backend keeps it (mainStore.ts).
    dictationEnabled: defaults.dictationEnabled,
    dictation: parseDictationPrefs(get("dictation")),
  };
}

/** The gate needs the claims: it asks only about files another session is known to change. */
export function gateActive(settings: Pick<Settings, "claimsEnabled" | "gateEnabled">): boolean {
  return settings.claimsEnabled && settings.gateEnabled;
}

export function loadSettings(fallbackLanguage: Language): Settings {
  try {
    return parseSettings(localStorage.getItem(SETTINGS_STORAGE_KEY), fallbackLanguage);
  } catch {
    return defaultSettings(fallbackLanguage);
  }
}

/** Are settings already stored? Tells existing users apart from the very first start. */
export function hasStoredSettings(): boolean {
  try {
    return localStorage.getItem(SETTINGS_STORAGE_KEY) !== null;
  } catch {
    return false;
  }
}

export function saveSettings(settings: Settings): void {
  try {
    localStorage.setItem(SETTINGS_STORAGE_KEY, JSON.stringify(withoutBackendKeys(settings)));
  } catch {
    // Without storage, the defaults apply again on the next start.
  }
}

/** The settings as stored in localStorage: without what the backend keeps (`BACKEND_KEYS`). */
export function withoutBackendKeys(settings: Settings): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(settings).filter(([key]) => !BACKEND_KEYS.includes(key)),
  );
}

/** The protected settings of `settings`. */
export function protectedOf(settings: ProtectedSettings): ProtectedSettings {
  return {
    warnMultilinePaste: settings.warnMultilinePaste,
    autoUpdate: settings.autoUpdate,
    hangarMd: settings.hangarMd,
    snippets: settings.snippets,
  };
}

/**
 * The protected settings an older Hangar kept in the stored settings text, for the one migration
 * to the backend (mainStore.ts); missing or invalid values are the defaults, never stored
 * snippets are the defaults in `language`.
 */
export function legacyProtected(text: string | null, language: Language): ProtectedSettings {
  const defaults = defaultSettings(language);
  let raw: unknown = null;
  try {
    raw = text === null ? null : JSON.parse(text);
  } catch {
    return protectedOf(defaults);
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return protectedOf(defaults);
  const flag = (key: "warnMultilinePaste" | "autoUpdate" | "hangarMd"): boolean => {
    const value: unknown = Reflect.get(raw, key);
    return typeof value === "boolean" ? value : defaults[key];
  };
  const storedLanguage: unknown = Reflect.get(raw, "language");
  return {
    warnMultilinePaste: flag("warnMultilinePaste"),
    autoUpdate: flag("autoUpdate"),
    hangarMd: flag("hangarMd"),
    snippets: parseSnippets(
      "snippets" in raw ? Reflect.get(raw, "snippets") : undefined,
      isLanguage(storedLanguage) ? storedLanguage : language,
    ),
  };
}

/** The notice settings of `settings`. */
export function noticesOf(settings: NoticeSettings): NoticeSettings {
  return {
    restoreSessions: settings.restoreSessions,
    attentionEnabled: settings.attentionEnabled,
    taskbarFlash: settings.taskbarFlash,
    notifyDesktop: settings.notifyDesktop,
    chime: settings.chime,
    quietHours: settings.quietHours,
  };
}

/**
 * The notice settings an older Hangar kept in the stored settings text, for the one migration to
 * the backend (mainStore.ts); missing or invalid values are the defaults.
 */
export function legacyNotices(text: string | null): NoticeSettings {
  const defaults = noticesOf(defaultSettings("en"));
  let raw: unknown = null;
  try {
    raw = text === null ? null : JSON.parse(text);
  } catch {
    return defaults;
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return defaults;
  const flag = (key: Exclude<NoticeKey, "quietHours">): boolean => {
    const value: unknown = Reflect.get(raw, key);
    return typeof value === "boolean" ? value : defaults[key];
  };
  return {
    restoreSessions: flag("restoreSessions"),
    attentionEnabled: flag("attentionEnabled"),
    taskbarFlash: flag("taskbarFlash"),
    notifyDesktop: flag("notifyDesktop"),
    chime: flag("chime"),
    quietHours: parseQuietHours(Reflect.get(raw, "quietHours")),
  };
}

/** The switches of `settings`. */
export function switchesOf(settings: SecuritySwitches): SecuritySwitches {
  return {
    claimsEnabled: settings.claimsEnabled,
    gateEnabled: settings.gateEnabled,
    commandGuardEnabled: settings.commandGuardEnabled,
  };
}

/**
 * The switches an older Hangar kept in the stored settings text, for the one migration to the
 * backend (mainStore.ts); missing or invalid values are on.
 */
export function legacySwitches(text: string | null): SecuritySwitches {
  const defaults = switchesOf(defaultSettings("en"));
  let raw: unknown = null;
  try {
    raw = text === null ? null : JSON.parse(text);
  } catch {
    return defaults;
  }
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) return defaults;
  const flag = (key: SecuritySwitchKey): boolean => {
    const value: unknown = Reflect.get(raw, key);
    return typeof value === "boolean" ? value : defaults[key];
  };
  return {
    claimsEnabled: flag("claimsEnabled"),
    gateEnabled: flag("gateEnabled"),
    commandGuardEnabled: flag("commandGuardEnabled"),
  };
}

/** The stored settings text, read once at start for [`legacySwitches`]. */
export function readStoredSettingsText(): string | null {
  try {
    return localStorage.getItem(SETTINGS_STORAGE_KEY);
  } catch {
    return null;
  }
}
