// The menu behind the sidebar footer's avatar and name: the account, the color mode and signing
// in or out. Pure, so the order and the marked mode are tested without React.

import { THEME_SETTINGS, type ThemeSetting } from "./theme";

export type AccountMenuEntry =
  /** Settings on the Account section (signed in). */
  | { kind: "account" }
  /** Signing in through the Team window (signed out). */
  | { kind: "signIn" }
  | { kind: "signOut" }
  /** Caption above the color modes. */
  | { kind: "themeHeading" }
  | { kind: "theme"; setting: ThemeSetting; current: boolean }
  | { kind: "separator"; key: string };

/**
 * Signed in: account, color modes, sign out. Signed out: sign in, color modes. The color modes
 * are the same three as in Settings; the chosen one is marked, "System" included.
 */
export function accountMenuEntries(signedIn: boolean, theme: ThemeSetting): AccountMenuEntry[] {
  const modes: AccountMenuEntry[] = THEME_SETTINGS.map((setting) => ({
    kind: "theme",
    setting,
    current: setting === theme,
  }));
  return [
    signedIn ? { kind: "account" } : { kind: "signIn" },
    { kind: "separator", key: "sep-theme" },
    { kind: "themeHeading" },
    ...modes,
    ...(signedIn
      ? [{ kind: "separator", key: "sep-sign-out" } as const, { kind: "signOut" } as const]
      : []),
  ];
}
