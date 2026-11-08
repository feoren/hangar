// Sections of the Settings dialog and keyboard control of the tab list (WAI-ARIA Tabs).

export const SETTINGS_SECTIONS = [
  "general",
  "appearance",
  "attention",
  "programs",
  "snippets",
  "updates",
] as const;

export type SettingsSection = (typeof SETTINGS_SECTIONS)[number];

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
