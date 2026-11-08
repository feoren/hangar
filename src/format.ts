// Pure display helpers for the sidebar.

import type { Profile } from "./api";
import type { AttentionState } from "./attention";
import type { Translate } from "./i18n";
import type { SessionView } from "./sessions";

/** First color of the palette; the fallback where no other one is chosen. */
export const DEFAULT_BADGE_COLOR = "#3b6fb6";
// White text reaches at least 4.5 : 1 on every color (WCAG AA), in light and dark alike.
export const BADGE_COLORS = [
  DEFAULT_BADGE_COLOR,
  "#8a5cc2",
  "#287a70",
  "#a85d2a",
  "#a8467a",
  "#5c7f2e",
];

/** The last two folders of a path; drive roots unchanged. */
export function shortPath(path: string): string {
  const parts = path.split(/[\\/]+/).filter((part) => part.length > 0);
  if (parts.length <= 1) return path;
  return parts.slice(-2).join("/");
}

/** Status for the sidebar. The attention state refines "running" and "exited". */
export function statusText(
  session: Pick<SessionView, "status" | "exitCode" | "stoppedByUser">,
  t: Translate,
  attention: AttentionState = "idle",
): string {
  if (attention === "attention") return t("status.attention");
  switch (session.status) {
    case "starting":
      return t("status.starting");
    case "running":
      return attention === "working" ? t("status.working") : t("status.running");
    case "exited":
      return session.stoppedByUser === true ? t("status.exitedByUser") : t("status.exited");
  }
}

/**
 * Exit code for the tooltip, only when it says something: non-zero and not ended by the user (then
 * the code is just that of the termination anyway). Otherwise empty.
 */
export function exitDetail(
  session: Pick<SessionView, "status" | "exitCode" | "stoppedByUser">,
  t: Translate,
): string {
  if (session.status !== "exited" || session.stoppedByUser === true) return "";
  if (session.exitCode === null || session.exitCode === 0) return "";
  return t("status.exitCode", { code: session.exitCode });
}

/** One or two characters: the profile icon, otherwise the first letter of the name. */
export function badgeText(profile: Profile | undefined, fallbackId: string): string {
  const icon = profile?.icon?.trim();
  if (icon) return Array.from(icon).slice(0, 2).join("");
  const source = profile?.name ?? fallbackId;
  return (Array.from(source)[0] ?? "?").toUpperCase();
}

/** Stable color per profile ID, so the same agents look the same. */
export function badgeColor(id: string): string {
  let hash = 0;
  for (const char of id) hash = (hash * 31 + (char.codePointAt(0) ?? 0)) >>> 0;
  return BADGE_COLORS[hash % BADGE_COLORS.length] ?? DEFAULT_BADGE_COLOR;
}

/** Short date with time in the UI language, e.g. "07.11., 16:40". */
export function formatShortTime(date: Date, language: string): string {
  return new Intl.DateTimeFormat(language, {
    day: "2-digit",
    month: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}
