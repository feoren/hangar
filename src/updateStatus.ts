// The status lines of the Updates section in Settings: one for Hangar, one for the programs. Pure,
// so the order of the cases can be tested.

import { formatShortTime } from "./format";
import type { Translate } from "./i18n";
import { displayVersion } from "./version";

/** What the Hangar line reads from an update state (`UpdateState` fits it). */
export type HangarUpdateStatus =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "current"; version: string }
  | { kind: "available"; update: { version: string } }
  | { kind: "installing"; update: { version: string } }
  | { kind: "error"; message: string };

/**
 * Result of Hangar's last check. "Idle" also follows an automatic check that found nothing, so it
 * names the time of the last successful check, or stays empty before the first one.
 */
export function hangarUpdateStatus(
  state: HangarUpdateStatus,
  checkedAt: number | null,
  language: string,
  t: Translate,
): string {
  switch (state.kind) {
    case "idle":
      return checkedAt === null
        ? ""
        : t("settings.lastChecked", { time: formatShortTime(new Date(checkedAt), language) });
    case "checking":
      return t("update.checking");
    case "current":
      return t("update.current", { version: displayVersion(state.version, t("version.beta")) });
    case "available":
      return t("update.inSidebar", {
        version: displayVersion(state.update.version, t("version.beta")),
      });
    case "installing":
      return t("update.installing", {
        version: displayVersion(state.update.version, t("version.beta")),
      });
    case "error":
      return state.message;
  }
}

/** What the programs line reads from `ProgramUpdateState`. */
export interface ProgramUpdateStatus {
  updates: ReadonlyMap<string, { updateAvailable: boolean }>;
  checking: boolean;
  error: string | null;
  checkedAt: number | null;
}

/** Checking, the error of "Check now", nothing yet, or how many updates and when. */
export function programUpdateStatus(
  state: ProgramUpdateStatus,
  language: string,
  t: Translate,
): string {
  if (state.checking) return t("settings.programChecking");
  if (state.error !== null) return t("settings.programCheckFailed", { error: state.error });
  if (state.checkedAt === null) return t("settings.notChecked");
  const count = [...state.updates.values()].filter((entry) => entry.updateAvailable).length;
  const result =
    count === 0 ? t("settings.programUpdatesNone") : t("settings.programUpdatesSome", { count });
  const time = formatShortTime(new Date(state.checkedAt), language);
  return `${result} ${t("settings.programCheckedAt", { time })}`;
}
