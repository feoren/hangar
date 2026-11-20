// Status line of a session in the sidebar: who is doing what right now ("Claude Code is working",
// "Claude Code is waiting for you") and where it runs (folder, git branch, run time).
// Pure, with the time passed in, so it can be tested; the sidebar samples it on a shared clock tick.

import type { AttentionReason, AttentionState } from "./attention";
import { folderName } from "./folders";
import { SHORT_PHRASES, statusPhrase, type StatusPhrase } from "./format";
import type { MessageKey, Translate } from "./i18n";
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

/** Each phrase with the program as its subject, so every language can build its own sentence. */
const AGENT_PHRASES: Record<StatusPhrase, MessageKey> = {
  starting: "status.agent.starting",
  working: "status.agent.working",
  idle: "status.agent.idle",
  attention: "status.agent.attention",
  asks: "status.agent.asks",
  gate: "status.agent.gate",
  permission: "status.agent.permission",
  failed: "status.agent.failed",
  exited: "status.agent.exited",
  exitedByUser: "status.agent.exitedByUser",
};

/** A session's state twice: one word for the row, the whole sentence for the peek. */
export interface StatusLine {
  /** One word for the row's title line: "working", "question". */
  word: string;
  /** The whole sentence for the peek and screen readers: "Claude Code has been working for 3 min". */
  full: string;
  /** Word and sentence are the stall hint ("quiet", "nothing new for 7 min"), not the state. */
  stalled?: boolean;
  /** Word and sentence say how long the session has waited for the person (waitHint.ts). */
  waitedLong?: boolean;
}

/** Waiting phrases that name the person as the one to act. */
const WAIT_PHRASES: readonly StatusPhrase[] = ["attention", "asks", "gate", "permission"];

/**
 * The row's state as one word ("working"), and for the peek the whole sentence with the program as
 * its subject ("Claude Code has been working for 3 min"). Working and resting say for how long
 * once that is a minute or more; waiting and ended sessions keep the plain phrase (the exit detail
 * lives in the peek). A working session that has shown nothing new since `stalledSince`
 * (stall.ts) says so instead, quietly, in the same place. A running session that has waited for
 * the person since `waitingSince` (waitHint.ts, set only past the threshold) says for how long.
 */
export function statusLine(
  agent: string,
  session: Pick<SessionView, "status" | "exitCode" | "stoppedByUser">,
  attention: AttentionState,
  activity: Activity,
  now: number,
  t: Translate,
  language: string,
  reason: AttentionReason | null = null,
  stalledSince: number | null = null,
  waitingSince: number | null = null,
): StatusLine {
  const phrase = statusPhrase(session, attention, reason);
  const waitedFor =
    waitingSince !== null && session.status === "running" && WAIT_PHRASES.includes(phrase)
      ? formatDuration(now - waitingSince, language)
      : null;
  if (waitedFor !== null) {
    return {
      word: t("waitHint.short", { duration: waitedFor }),
      full: t("waitHint.agent", { agent, duration: waitedFor }),
      waitedLong: true,
    };
  }
  const stalledFor =
    phrase === "working" && stalledSince !== null
      ? formatDuration(now - stalledSince, language)
      : null;
  if (stalledFor !== null) {
    return {
      word: t("stall.short"),
      full: t("stall.agent", { agent, duration: stalledFor }),
      stalled: true,
    };
  }
  const since =
    phrase === "working" ? activity.workingSince : phrase === "idle" ? activity.lastOutput : null;
  const duration = since === null ? null : formatDuration(now - since, language);
  // The title and the group header already name the program; the row says only the state.
  const word = t(SHORT_PHRASES[phrase]);
  if (duration === null) return { word, full: t(AGENT_PHRASES[phrase], { agent }) };
  const full =
    phrase === "working"
      ? t("status.agent.workingFor", { agent, duration })
      : t("status.agent.idleFor", { agent, duration });
  return { word, full };
}

export interface DetailLine {
  folder: string;
  branch: string | null;
  /** Run time of the process; `null` when it has ended or runs for less than a minute. */
  runtime: string | null;
}

/** Third line: folder name, git branch and run time. */
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
