// Anonymous usage statistics, the window's side: the answer as the backend keeps it, the settings
// snapshot a daily report shows, and counting a feature. The backend decides everything else
// (telemetry.rs): nothing is counted or sent before the person said yes, and it maps every value
// to its allowlists before a report leaves the machine.

import { api } from "./api";
import type { Language } from "./i18n";
import type { Settings } from "./settings";

/** `null`: not asked yet; the window asks once (UsageStatsNotice.tsx). */
export type UsageConsent = boolean | null;

/** The functions a report counts; the backend takes these names only (`Feature`). */
export type UsageFeature =
  | "fileViewerOpened"
  | "commitHistoryOpened"
  | "notesCreated"
  | "sidePanelDetached"
  | "broadcastSent"
  | "pathMentioned"
  | "folderRevealed"
  | "updateInstalled";

/** The settings a report shows, as the window keeps them (`SettingsInput` in telemetry.rs). */
export interface UsageSettings {
  language: Language;
  theme: string;
  terminalScheme: string;
  terminalFont: string;
  terminalFontSize: number;
  cursorStyle: string;
  viewerTheme: string;
  dotAnimation: string;
  attention: boolean;
  restoreSessions: boolean;
  sidePanelDetached: boolean;
  sidebarCollapsed: boolean;
}

/** The answer of `telemetry_status`; `undefined` for anything else. */
export function parseUsageConsent(value: unknown): UsageConsent | undefined {
  if (value === null || typeof value === "boolean") return value;
  return undefined;
}

/** The snapshot of the current settings and layout. */
export function usageSettings(
  settings: Settings,
  layout: { fontSize: number; collapsed: boolean },
  panelDetached: boolean,
): UsageSettings {
  return {
    language: settings.language,
    theme: settings.theme,
    terminalScheme: settings.terminalScheme,
    terminalFont: settings.terminalFont,
    terminalFontSize: layout.fontSize,
    cursorStyle: settings.cursorStyle,
    viewerTheme: settings.viewerTheme,
    dotAnimation: settings.dotAnimation,
    attention: settings.attentionEnabled,
    restoreSessions: settings.restoreSessions,
    sidePanelDetached: panelDetached,
    sidebarCollapsed: layout.collapsed,
  };
}

/**
 * One use of a feature. Fire and forget: the backend ignores it while the statistics are off,
 * and a failure must never disturb what the person was doing.
 */
export function countFeature(feature: UsageFeature): void {
  try {
    api.telemetryCount(feature).catch(() => undefined);
  } catch {
    // Outside Tauri (a test, the browser) there is no backend to count with.
  }
}

/**
 * Like {@link countFeature}, but waits until the backend stored it: for an update whose installer
 * may end Hangar right after. Never rejects.
 */
export async function countFeatureNow(feature: UsageFeature): Promise<void> {
  try {
    await api.telemetryCount(feature);
  } catch {
    // Counting must never stop the update.
  }
}
