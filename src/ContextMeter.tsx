// How full a session's context window is: a hair-thin track along the bottom edge of its sidebar
// row, filled by the share, without text in the row. The share and the numbers are in the tooltip
// and in the row's accessible name. Nothing shows until the agent answered once or for programs
// Hangar cannot read. Re-renders only when this session's numbers change.

import { useT } from "./i18n";
import { contextHint, contextShare, HIGH_SHARE } from "./contextUsage";
import { useSessionContext } from "./useContextUsage";

interface Props {
  sessionId: number;
  /** UI language, for the percent format. */
  language: string;
}

export function ContextMeter({ sessionId, language }: Props) {
  const t = useT();
  const context = useSessionContext(sessionId);
  if (context === undefined) return null;
  const share = contextShare(context);
  const hint = contextHint(context, language, t);
  return (
    <span className={share >= HIGH_SHARE ? "session-context high" : "session-context"} title={hint}>
      <span className="session-context-track" aria-hidden="true">
        <span
          className="session-context-fill"
          style={{ width: `${String(Math.round(share * 100))}%` }}
        />
      </span>
      <span className="sr-only">{hint}</span>
    </span>
  );
}
