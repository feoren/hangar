// Status chip at the right end of a session row (and its small twin in the collapsed rail). Each
// state has its own shape, so color is never the only signal: working shows three dots, waiting a
// check (done) or a bell (the program rang, often a question), a failed exit a red exclamation
// mark, an ordinary exit a quiet square. Pure, so the rules can be tested.

import type { AttentionReason, AttentionState } from "./attention";
import type { MessageKey } from "./i18n";
import type { SessionView } from "./sessions";

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

/** Which chip a row shows; `null` for a running or starting session that needs nothing. */
export function sessionChip(
  session: Pick<SessionView, "status" | "exitCode" | "stoppedByUser">,
  state: AttentionState,
  reason: AttentionReason | null,
): ChipKind | null {
  if (exitedWithError(session)) return "failed";
  if (state === "attention") return reason === "bell" ? "asks" : "done";
  if (session.status === "exited") return "ended";
  if (state === "working") return "working";
  return null;
}

/** Waiting chips stand out; the others are quiet companions of the status line. */
export function chipWaits(kind: ChipKind | null): boolean {
  return kind === "done" || kind === "asks";
}

/** Short visible label; working and an ordinary exit have none, the status line says it. */
export const CHIP_LABELS: Record<ChipKind, MessageKey | null> = {
  working: null,
  done: "chip.done",
  asks: "chip.asks",
  failed: "chip.failed",
  ended: null,
};
