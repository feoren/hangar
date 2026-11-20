// "Done" notice: when an agent session stops working, a short note says how many files in its
// folder are changed, and a click opens them in the Changes tab. Pure decisions, so they can be
// tested without the app.

import type { AttentionReason, AttentionState } from "./attention";

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
  return (
    previous === "working" && next !== "working" && reason !== "gate" && reason !== "permission"
  );
}

/** Changed files in the answer of `git_changed_count`; 0 outside a repository or when malformed. */
export function parseChangedCount(answer: unknown): number {
  return typeof answer === "number" && Number.isSafeInteger(answer) && answer > 0 ? answer : 0;
}

/**
 * Changed files in the folder of session `id` for the notice, asked through `count`
 * (`api.gitChangedCount`). Rejects when git fails, so the caller shows nothing, as before.
 */
export async function doneFileCount(
  id: number,
  count: (id: number) => Promise<unknown>,
): Promise<number> {
  return parseChangedCount(await count(id));
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
