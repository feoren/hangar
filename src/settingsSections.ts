// Sections of the Settings dialog and keyboard control of the tab list (WAI-ARIA Tabs).

import type { MessageKey } from "./i18n";

export const SETTINGS_SECTIONS = [
  "general",
  "appearance",
  "attention",
  "programs",
  "snippets",
  "account",
  "updates",
] as const;

export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];

/** Name of each section, for the tab list and quick access. */
export const SECTION_NAMES: Record<SettingsSection, MessageKey> = {
  general: "settings.general",
  appearance: "settings.appearance",
  attention: "settings.attention",
  programs: "settings.programs",
  snippets: "settings.snippets",
  account: "settings.account",
  updates: "settings.updates",
};

/**
 * Target tab for a key, or `null` if the key does not concern the list. Up/Down for the
 * vertical list; Left/Right as well, because the list turns horizontal in a narrow window.
 * Past the end it wraps around to the start.
 */
export function nextTabIndex(key: string, index: number, count: number): number | null {
  if (count <= 0) return null;
  switch (key) {
    case "ArrowDown":
    case "ArrowRight":
      return (index + 1) % count;
    case "ArrowUp":
    case "ArrowLeft":
      return (index - 1 + count) % count;
    case "Home":
      return 0;
    case "End":
      return count - 1;
    default:
      return null;
  }
}
