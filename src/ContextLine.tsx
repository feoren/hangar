// The fourth line of a session row: how long the process runs, in a cell of fixed width at the
// start, then how full its context window is, as a striped bar across the free width, and the
// share and the tokens used of the window in cells of fixed width at the right edge
// ("1 h  ━━━━──  22 %  215.5k / 1M"), so every value lines up from row to row (styles.css,
// "Fourth line"). The exact numbers are for screen readers. No tooltip: session rows have none.
// A row without context numbers (before the agent answered, or a program Hangar cannot read)
// keeps the run time in the same place; without either the line is gone. Re-renders only when
// this session's numbers or its run time change.

import { useT } from "./i18n";
import {
  compactTokens,
  contextHint,
  contextShare,
  formatShare,
  HIGH_SHARE,
  type SessionContext,
} from "./contextUsage";
import { useSessionContext } from "./useContextUsage";

interface Props {
  /** "1 h"; `null` below a minute and for an ended session. */
  runtime: string | null;
  /** UI language, for the percent format. */
  language: string;
}

/** The line, or nothing at all without a run time and context numbers. */
export function ContextLine({ sessionId, runtime, language }: Props & { sessionId: number }) {
  const context = useSessionContext(sessionId);
  return <ContextLineCells runtime={runtime} context={context} language={language} />;
}

/** The same without the store, for given numbers (or none yet). */
export function ContextLineCells({
  runtime,
  context,
  language,
}: Props & { context: SessionContext | undefined }) {
  const t = useT();
  if (runtime === null && context === undefined) return null;
  const share = context === undefined ? 0 : contextShare(context);
  return (
    <span className={share >= HIGH_SHARE ? "session-context high" : "session-context"}>
      {/* Kept as an empty cell below a minute, so the bar starts where it does in every row. */}
      <span className="session-runtime">
        {runtime !== null && (
          <>
            <span className="sr-only">{t("session.runtime", { duration: runtime })}</span>
            <span aria-hidden="true">{runtime}</span>
          </>
        )}
      </span>
      {context !== undefined && (
        <>
          <span className="session-context-bar" aria-hidden="true">
            <span
              className="session-context-fill"
              style={{ width: `${String(Math.round(share * 100))}%` }}
            />
          </span>
          <span className="session-context-share" aria-hidden="true">
            {formatShare(share, language)}
          </span>
          <span className="session-context-tokens" aria-hidden="true">
            {compactTokens(context.used)} / {compactTokens(context.window)}
          </span>
          <span className="sr-only">{contextHint(context, language, t)}</span>
        </>
      )}
    </span>
  );
}
