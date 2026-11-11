// How full a session's context window is, at the end of its row's second line: a tiny bar and
// the share ("▮▯▯ 12 %"), the numbers in the tooltip. Nothing shows until the agent answered once
// or for programs Hangar cannot read. Re-renders only when this session's numbers change.

import { useT } from "./i18n";
import { contextHint, contextShare, formatShare, HIGH_SHARE } from "./contextUsage";
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
      <span aria-hidden="true">·</span>
      <span className="session-context-bar" aria-hidden="true">
        <span
          className="session-context-fill"
          style={{ width: `${String(Math.round(share * 100))}%` }}
        />
      </span>
      <span aria-hidden="true">{formatShare(share, language)}</span>
      <span className="sr-only">{hint}</span>
    </span>
  );
}
