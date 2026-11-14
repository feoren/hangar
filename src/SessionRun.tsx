// The session half of a row's third line: how long it runs, how full its context window is (a bar
// and the share) and the tokens used of the window ("1 h  ━━──  22 %  215.5k / 1M"). Each value
// sits in a cell of fixed width, so the columns line up from row to row (styles.css, "Third
// line"); the exact numbers are for screen readers. No tooltip: session rows have none. The
// context cells show once the agent answered and never for programs Hangar cannot read.
// Re-renders only when this session's numbers or its run time change.

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
  sessionId: number;
  /** "1 h"; `null` below a minute, when the cell stays as an empty column. */
  runtime: string | null;
  /** UI language, for the percent format. */
  language: string;
}

/** The group, or nothing at all when it has neither a run time nor context numbers. */
export function SessionRun({ sessionId, runtime, language }: Props) {
  const context = useSessionContext(sessionId);
  return <SessionRunCells runtime={runtime} context={context} language={language} />;
}

/** The same without the store, for a given context (or none yet). */
export function SessionRunCells({
  runtime,
  context,
  language,
}: Omit<Props, "sessionId"> & { context: SessionContext | undefined }) {
  const t = useT();
  if (runtime === null && context === undefined) return null;
  return (
    <span className="session-run">
      <span className="session-runtime">
        {runtime !== null && (
          <>
            <span className="sr-only">{t("session.runtime", { duration: runtime })}</span>
            <span aria-hidden="true">{runtime}</span>
          </>
        )}
      </span>
      {context !== undefined && <ContextCells context={context} language={language} />}
    </span>
  );
}

function ContextCells({ context, language }: { context: SessionContext; language: string }) {
  const t = useT();
  const share = contextShare(context);
  return (
    <span className={share >= HIGH_SHARE ? "session-context high" : "session-context"}>
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
    </span>
  );
}
