// What a session's state looks like in the sidebar. Expanded rows carry a thin bar at their left
// edge in the state's color; the collapsed rail keeps a small chip at the badge's corner. Each
// state has its own shape there (a check for done, a bell for a question, an exclamation mark for
// a failed exit), and the row's status line always says it in words, so color is never the only
// signal. Pure, so the rules can be tested.

import type { AttentionReason, AttentionState } from "./attention";
import type { SessionView } from "./sessions";
import type { LookState } from "./stateLook";

export type ChipKind = "working" | "done" | "asks" | "failed" | "ended";

/** The process ended on its own with a known, non-zero code. */
export function exitedWithError(
  session: Pick<SessionView, "status" | "exitCode" | "stoppedByUser">,
): boolean {
  return (
    session.status === "exited" &&
    session.stoppedByUser !== true &&
    session.exitCode !== null &&
    session.exitCode !== 0
  );
}

/** Which state a session shows; `null` for a running or starting session that needs nothing. */
export function sessionChip(
  session: Pick<SessionView, "status" | "exitCode" | "stoppedByUser">,
  state: AttentionState,
  reason: AttentionReason | null,
): ChipKind | null {
  if (exitedWithError(session)) return "failed";
  // A pending decision (gate) is a question too.
  if (state === "attention") return reason === "bell" || reason === "gate" ? "asks" : "done";
  if (session.status === "exited") return "ended";
  if (state === "working") return "working";
  return null;
}

/** Waiting chips stand out; the others are quiet companions of the status line. */
export function chipWaits(kind: ChipKind | null): boolean {
  return kind === "done" || kind === "asks";
}

/** The chips the narrow rail shows instead of its dot: waiting and failed sessions. */
export type RailChipKind = "done" | "asks" | "failed";

export function railShowsChip(chip: ChipKind | null): chip is RailChipKind {
  return chip === "done" || chip === "asks" || chip === "failed";
}

/**
 * Which state look (stateLook.ts) a row takes: a question or decision looks like "asks", any other
 * waiting like "done", a running session is working or resting ("idle", shown as pause). Starting
 * and ended sessions keep their plain look.
 */
export function rowLook(
  session: Pick<SessionView, "status">,
  state: AttentionState,
  chip: ChipKind | null,
): LookState | null {
  if (state === "attention") return chip === "asks" ? "asks" : "done";
  if (session.status !== "running") return null;
  return state === "working" ? "working" : "idle";
}

/** Color of a row's left bar: a state look, or one of the states without a look of their own. */
export type BarState = LookState | "starting" | "ended" | "failed";

/**
 * The left bar of a session row. A failed exit wins over waiting (the error is the news), then the
 * row's look; a session without one is either still starting or has ended.
 */
export function rowBar(
  session: Pick<SessionView, "status">,
  state: AttentionState,
  chip: ChipKind | null,
): BarState {
  if (chip === "failed") return "failed";
  const look = rowLook(session, state, chip);
  if (look !== null) return look;
  return session.status === "starting" ? "starting" : "ended";
}

/** Class of the bar element (styles.css, `.session-bar.bar-<state>`). */
export function barClass(bar: BarState): string {
  return `session-bar bar-${bar}`;
}
