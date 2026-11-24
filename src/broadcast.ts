// "Send to several": the same text to several running sessions at once, each as a bracketed paste
// followed by Enter. Pure choice of the candidates and the default selection.

import { pasteWarning } from "./appPaste";
import { shellKind } from "./pathQuote";
import type { SessionView } from "./sessions";
import { sanitizePaste } from "./termSafety";

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
  /** Whether the program in the session has turned on bracketed paste right now. */
  bracketedPaste: () => boolean;
  /** Bracketed paste; resolves once the backend has the text. */
  pasteAndWait: (text: string) => Promise<void>;
  /** Enter as a separate write. */
  submit: () => void;
}

/**
 * Text sent without a question (a broadcast, a queued message, an agent command) may carry
 * content the user did not read, such as a diff or a selection filled in for a variable. Without
 * bracketed paste a line break runs what follows it, so such text does not go to that target.
 * Invisible characters make the text read differently than it runs, and an agent reads tag
 * characters as an instruction nobody saw; bracketed paste does not change that, so they block
 * every target. Line breaks, tabs and emoji joiners do not count (`pasteWarning`).
 */
export function blockedFor(target: Pick<BroadcastTarget, "bracketedPaste">, text: string): boolean {
  const warning = pasteWarning(text, target.bracketedPaste());
  return warning !== null && (warning.runsLines || warning.hidden);
}

/** Whether anything is left to send once control characters are stripped (`sanitizePaste`). */
function hasContent(text: string): boolean {
  return sanitizePaste(text).trim() !== "";
}

/**
 * Pastes into all targets at once; each gets Enter only after its own paste has reached the
 * backend and `pause` has passed, so a TUI does not take the Enter as part of the paste.
 * Resolves to the targets left out (`blockedFor`), so the caller can say so. Text that is blank
 * once control characters are stripped sends nothing at all: the paste would be empty and the
 * Enter alone would submit whatever the program had in its input.
 */
export async function sendToTargets<T extends BroadcastTarget>(
  targets: readonly T[],
  text: string,
  pause: () => Promise<void>,
): Promise<T[]> {
  if (!hasContent(text)) return [];
  const blocked = targets.filter((target) => blockedFor(target, text));
  await Promise.all(
    targets
      .filter((target) => !blocked.includes(target))
      .map(async (target) => {
        await target.pasteAndWait(text);
        await pause();
        target.submit();
      }),
  );
  return blocked;
}

/**
 * Text can be sent when it is not blank, also not once control characters are stripped, and at
 * least one session is chosen.
 */
export function canBroadcast(text: string, targets: readonly number[]): boolean {
  return hasContent(text) && targets.length > 0;
}
