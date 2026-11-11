// Status line of a session in the sidebar: what it is doing now ("working for 3 min", "waiting for
// you", "active 5 min ago") and where it runs (folder, git branch, run time). Pure, with the time
// passed in, so it can be tested; the sidebar samples it on a shared clock tick.

import type { AttentionReason, AttentionState } from "./attention";
import { folderName } from "./folders";
import { statusText } from "./format";
import type { Translate } from "./i18n";
import type { SessionView } from "./sessions";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** How often the sidebar refreshes the times; minutes are the smallest unit shown. */
export const STATUS_TICK_MS = 30_000;

/** Times of a session (epoch milliseconds), collected outside React. */
export interface Activity {
  /** Start of the current working phase; `null` while not working. */
  workingSince: number | null;
  /** Last output of the program; `null` before the first one. */
  lastOutput: number | null;
}

export const NO_ACTIVITY: Activity = { workingSince: null, lastOutput: null };

/**
 * Compact duration in the largest whole unit: "3 min", "2 h", "4 days" (in the UI language via
 * Intl, so no unit needs translating). `null` below one minute: a few seconds say nothing.
 */
export function formatDuration(ms: number, language: string): string | null {
  if (!Number.isFinite(ms) || ms < MINUTE) return null;
  const [value, unit] =
    ms < HOUR
      ? [Math.floor(ms / MINUTE), "minute"]
      : ms < DAY
        ? [Math.floor(ms / HOUR), "hour"]
        : [Math.floor(ms / DAY), "day"];
  return new Intl.NumberFormat(language, { style: "unit", unit, unitDisplay: "short" }).format(
    value,
  );
}

/**
 * First line: the status, refined with how long it has been going on. Waiting and ended sessions
 * keep their plain status text (the exit detail lives in the tooltip).
 */
export function statusLine(
  session: Pick<SessionView, "status" | "exitCode" | "stoppedByUser">,
  attention: AttentionState,
  activity: Activity,
  now: number,
  t: Translate,
  language: string,
  reason: AttentionReason | null = null,
): string {
  const plain = statusText(session, t, attention, reason);
  if (attention === "attention" || session.status !== "running") return plain;
  if (attention === "working") {
    const since = activity.workingSince;
    const duration = since === null ? null : formatDuration(now - since, language);
    return duration === null ? plain : t("status.workingFor", { duration });
  }
  const last = activity.lastOutput;
  const ago = last === null ? null : formatDuration(now - last, language);
  return ago === null ? plain : t("status.activeAgo", { duration: ago });
}

export interface DetailLine {
  folder: string;
  branch: string | null;
  /** Run time of the process; `null` when it has ended or runs for less than a minute. */
  runtime: string | null;
}

/** Second line: folder name, git branch and run time. */
export function detailLine(
  session: Pick<SessionView, "cwd" | "status" | "createdAt">,
  branch: string | null,
  now: number,
  language: string,
): DetailLine {
  return {
    folder: folderName(session.cwd),
    branch,
    runtime: session.status === "exited" ? null : formatDuration(now - session.createdAt, language),
  };
}
