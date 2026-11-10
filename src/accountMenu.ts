// The menu behind the sidebar footer's avatar and name: the notes, the color mode and signing in
// or out. Pure, so the order and the marked mode are tested without React.

import { THEME_SETTINGS, type ThemeSetting } from "./theme";

export type AccountMenuEntry =
  /** Signing in through the Team window (signed out). */
  | { kind: "signIn" }
  /** Shows or hides the notes panel; `open` says which of the two the entry does. */
  | { kind: "notes"; open: boolean }
  | { kind: "signOut" }
  /** Caption above the color modes. */
  | { kind: "themeHeading" }
  | { kind: "theme"; setting: ThemeSetting; current: boolean }
  | { kind: "separator"; key: string };

/**
 * Signed in: notes, color modes, sign out. Signed out: sign in, notes, color modes. The color
 * modes are the same three as in Settings; the chosen one is marked, "System" included. The
 * account itself lives in Settings (the gear next to this menu).
 */
export function accountMenuEntries(
  signedIn: boolean,
  theme: ThemeSetting,
  notesOpen: boolean,
): AccountMenuEntry[] {
  const modes: AccountMenuEntry[] = THEME_SETTINGS.map((setting) => ({
    kind: "theme",
    setting,
    current: setting === theme,
  }));
  return [
    ...(signedIn
      ? []
      : [{ kind: "signIn" } as const, { kind: "separator", key: "sep-notes" } as const]),
    { kind: "notes", open: notesOpen },
    { kind: "separator", key: "sep-theme" },
    { kind: "themeHeading" },
    ...modes,
    ...(signedIn
      ? [{ kind: "separator", key: "sep-sign-out" } as const, { kind: "signOut" } as const]
      : []),
  ];
}
