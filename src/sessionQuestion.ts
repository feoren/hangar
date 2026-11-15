// A question the panel asks about one session's folder ("Discard changes to app.ts?"). The
// panel follows the active session, so the session can change while the question is open: its
// answer must act on the session and folder it was asked for, or not at all. Pure logic, so it can
// be tested without React.

/** What a question is about, with the session and folder it was opened for. */
export interface SessionQuestion<T> {
  item: T;
  sessionId: number;
  cwd: string;
}

/** A question about `item` for the panel's session and folder; `null` without either. */
export function askFor<T>(
  item: T,
  sessionId: number | null,
  cwd: string | null,
): SessionQuestion<T> | null {
  if (sessionId === null || cwd === null) return null;
  return { item, sessionId, cwd };
}

/**
 * The open question while the panel still shows the session and folder it was asked for;
 * `null` once either changed, so neither the question nor its answer reaches another session.
 */
export function stillAsked<T>(
  question: SessionQuestion<T> | null,
  sessionId: number | null,
  cwd: string | null,
): SessionQuestion<T> | null {
  if (question === null) return null;
  return question.sessionId === sessionId && question.cwd === cwd ? question : null;
}
