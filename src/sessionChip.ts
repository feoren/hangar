// Status chip at the end of a session row's title line (and its small twin in the collapsed
// rail). Each state has its own shape, so color is never the only signal: working shows three
// dots, a resting agent two pause bars, waiting a check (done) or a bell (the program rang, often a
// question), a failed exit an exclamation mark, an ordinary exit a quiet square. Pure, so the rules
// can be tested.

import type { AttentionReason, AttentionState } from "./attention";
import { shellKind } from "./pathQuote";
import type { SessionView } from "./sessions";
import type { LookState } from "./stateLook";

export type ChipKind = "working" | "paused" | "done" | "asks" | "failed" | "ended";

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

/**
 * The session runs an agent (or another program), not a plain shell: told apart by the command,
 * as `shellKind` does. Update sessions only run an installer; an unknown profile says nothing.
 */
export function runsAgent(command: string | undefined, kind?: SessionView["kind"]): boolean {
  if (command === undefined || kind === "update") return false;
  return shellKind(command) === "agent";
}

/**
 * Which chip a row shows; `null` for a starting session and for a resting shell. A resting agent
 * (`agent`: its program is no plain shell) shows the pause bars; a shell rests at its prompt most
 * of the time, so a pause chip there would only add noise.
 */
export function sessionChip(
  session: Pick<SessionView, "status" | "exitCode" | "stoppedByUser">,
  state: AttentionState,
  reason: AttentionReason | null,
  agent = false,
): ChipKind | null {
  if (exitedWithError(session)) return "failed";
  // A pending decision (gate) is a question too.
  if (state === "attention") return reason === "bell" || reason === "gate" ? "asks" : "done";
  if (session.status === "exited") return "ended";
  if (state === "working") return "working";
  if (session.status === "running" && agent) return "paused";
  return null;
}

/** Waiting chips stand out; the others are quiet companions of the status line. */
export function chipWaits(kind: ChipKind | null): boolean {
  return kind === "done" || kind === "asks";
}

/**
 * Which state look (stateLook.ts) a row takes: a failed exit that still waits to be seen looks
 * like "failed", a question or decision like "asks", any other waiting like "done", a running
 * session is working or resting ("idle", shown as pause). Starting and ended sessions keep their
 * plain look; a failed one once seen keeps only its edge and chip.
 */
export function rowLook(
  session: Pick<SessionView, "status">,
  state: AttentionState,
  chip: ChipKind | null,
): LookState | null {
  if (state === "attention") {
    if (chip === "failed") return "failed";
    return chip === "asks" ? "asks" : "done";
  }
  if (session.status !== "running") return null;
  return state === "working" ? "working" : "idle";
}

/**
 * Which colored left edge a row in the expanded sidebar carries; every row has one. A failed exit
 * keeps the failed color also once it was seen; otherwise the edge follows the row's look, and the
 * states without a look (starting, an ordinary end) get their own quiet edge.
 */
export type RowEdge = LookState | "starting" | "ended" | "failed";

export function rowEdge(
  session: Pick<SessionView, "status" | "exitCode" | "stoppedByUser">,
  state: AttentionState,
  chip: ChipKind | null,
): RowEdge {
  if (chip === "failed" || exitedWithError(session)) return "failed";
  const look = rowLook(session, state, chip);
  if (look !== null) return look;
  return session.status === "starting" ? "starting" : "ended";
}

/** The CSS class of an edge (styles.css, `.edge-*`). */
export function edgeClass(edge: RowEdge): string {
  return `edge-${edge}`;
}
