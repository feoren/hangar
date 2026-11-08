// "Send to several": the same text to several running sessions at once, each as a bracketed paste
// followed by Enter. Pure choice of the candidates and the default selection.

import { shellKind } from "./pathQuote";
import type { SessionView } from "./sessions";

type Candidate = Pick<SessionView, "id" | "profileId" | "status" | "kind">;

/** Running sessions that can receive text; update sessions are left out. */
export function broadcastCandidates<T extends Candidate>(sessions: readonly T[]): T[] {
  return sessions.filter((session) => session.status !== "exited" && session.kind !== "update");
}

/**
 * Checked at the start: the running agent sessions. Shells are left out, because a prompt meant
 * for an agent would run as a command there. Programs are told apart by their command
 * (`shellKind`): anything that is not a known shell counts as an agent.
 */
export function defaultTargets(
  sessions: readonly Candidate[],
  commandOf: (profileId: string) => string | undefined,
): number[] {
  return broadcastCandidates(sessions)
    .filter((session) => shellKind(commandOf(session.profileId) ?? "") === "agent")
    .map((session) => session.id);
}

/** What sending needs from a terminal (`SessionTerminal`). */
export interface BroadcastTarget {
  /** Bracketed paste; resolves once the backend has the text. */
  pasteAndWait: (text: string) => Promise<void>;
  /** Enter as a separate write. */
  submit: () => void;
}

/**
 * Pastes into all targets at once; each gets Enter only after its own paste has reached the
 * backend and `pause` has passed, so a TUI does not take the Enter as part of the paste.
 */
export async function sendToTargets(
  targets: readonly BroadcastTarget[],
  text: string,
  pause: () => Promise<void>,
): Promise<void> {
  await Promise.all(
    targets.map(async (target) => {
      await target.pasteAndWait(text);
      await pause();
      target.submit();
    }),
  );
}

/** Text can be sent when it is not blank and at least one session is chosen. */
export function canBroadcast(text: string, targets: readonly number[]): boolean {
  return text.trim() !== "" && targets.length > 0;
}
