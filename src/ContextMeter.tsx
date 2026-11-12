// How full a session's context window is, filling the rest of its row's detail line up to the
// right edge: a bar that takes the free width, the share and the tokens used of the window
// ("━━━━── 12 % · 137k / 1M"), the exact numbers for screen readers. No tooltip: session rows
// have none. Nothing shows until the agent answered once or for programs Hangar cannot read. Re-renders only when this session's numbers
// change.

import { useT } from "./i18n";
import { compactTokens, contextHint, contextShare, formatShare, HIGH_SHARE } from "./contextUsage";
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
    <span className={share >= HIGH_SHARE ? "session-context high" : "session-context"}>
      <span aria-hidden="true">·</span>
      <span className="session-context-bar" aria-hidden="true">
        <span
          className="session-context-fill"
          style={{ width: `${String(Math.round(share * 100))}%` }}
        />
      </span>
      <span aria-hidden="true">{formatShare(share, language)}</span>
      <span className="session-context-tokens" aria-hidden="true">
        · {compactTokens(context.used)} / {compactTokens(context.window)}
      </span>
      <span className="sr-only">{hint}</span>
    </span>
  );
}
