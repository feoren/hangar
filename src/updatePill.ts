// The "Update" button at the top right of the expanded sidebar: shown while a Hangar update is
// available or installing, nothing else. Its tooltip says what a click does. Pure, so the rules can
// be tested.

import type { Translate } from "./i18n";
import type { UpdateState } from "./useUpdater";
import { displayVersion } from "./version";

export type UpdatePill =
  | { installing: false; version: string }
  /** `percent`: download progress, `null` while unknown. */
  | { installing: true; version: string; percent: number | null };

/** What the button reads from an update state (`UpdateState` fits it). */
export type PillState =
  | { kind: "available"; update: { version: string } }
  | { kind: "installing"; update: { version: string }; percent: number | null }
  | { kind: Exclude<UpdateState["kind"], "available" | "installing"> };

/** The button for an update state; `null` when there is nothing to install. */
export function updatePill(state: PillState): UpdatePill | null {
  switch (state.kind) {
    case "available":
      return { installing: false, version: state.update.version };
    case "installing":
      return { installing: true, version: state.update.version, percent: state.percent };
    default:
      return null;
  }
}

/**
 * Tooltip and accessible name: the version and what happens to the running sessions. The app
 * saves the workspace before installing, so with "restore sessions" on they come back after the
 * restart; otherwise they just end.
 */
export function updatePillHint(
  pill: UpdatePill,
  running: number,
  restoresSessions: boolean,
  t: Translate,
): string {
  const version = displayVersion(pill.version, t("version.beta"));
  if (pill.installing) {
    return pill.percent === null
      ? t("update.installing", { version })
      : t("update.installingProgress", { version, percent: pill.percent });
  }
  if (running === 0) return t("update.pillHint", { version });
  return restoresSessions
    ? t("update.pillHintRestores", { version })
    : t("update.pillHintEnds", { version });
}

/** Width of the progress fill inside the button, in percent; `null` for the unknown shimmer. */
export function pillProgress(pill: UpdatePill): number | null {
  if (!pill.installing || pill.percent === null) return null;
  return Math.min(100, Math.max(0, pill.percent));
}
