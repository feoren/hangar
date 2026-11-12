// "Done" notice: when an agent session stops working, a short note says how many files in its
// folder are changed, and a click opens them in the Changes tab. Pure decisions, so they can be
// tested without the app.

import type { AttentionReason, AttentionState } from "./attention";
import { parseGitDiff } from "./gitDiff";

/** How long the notice stays when nobody touches it. */
export const DONE_NOTICE_MS = 8000;

export interface DoneNotice {
  sessionId: number;
  title: string;
  count: number;
}

/**
 * Whether the session just finished a stretch of work: it was working and now rests or waits.
 * Waiting for a decision at the gate is not done.
 */
export function finishedWork(
  previous: AttentionState,
  next: AttentionState,
  reason: AttentionReason | null,
): boolean {
  return previous === "working" && next !== "working" && reason !== "gate";
}

/** Changed files in the answer of `git_diff`; 0 outside a repository or for a malformed answer. */
export function changedFileCount(answer: unknown): number {
  const diff = parseGitDiff(answer);
  return diff?.isRepo === true ? diff.files.length : 0;
}

/**
 * Whether to show the notice once the count is known: only with changes, only while the session
 * still rests, and not when its changes are on screen already (it is the active session and the
 * Changes tab is open, since that tab follows the active session).
 */
export function offersDoneNotice(input: {
  count: number;
  sessionId: number;
  state: AttentionState;
  activeId: number | null;
  changesShown: boolean;
}): boolean {
  if (input.count <= 0 || input.state === "working") return false;
  return !(input.changesShown && input.activeId === input.sessionId);
}
