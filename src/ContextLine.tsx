// The run time and the context window's parts of a session row: the run time in a cell of fixed
// width, the context as a striped bar with its share, a small bar, ten still segments, a ring or
// the share alone, and the tokens used of the window in a cell of fixed width ("1 h  ━━━━──  22 %
// 215.5k / 1M"), so every value lines up from row to row (styles.css, "Fourth line"). The row
// layout (rowLayout.ts) decides which of them show and where. All of them are pure and
// decorative: the exact numbers reach screen readers once per row (SessionRow.tsx). No tooltip:
// session rows have none.

import { useT } from "./i18n";
import {
  compactTokens,
  contextShare,
  formatShare,
  HIGH_SHARE,
  type SessionContext,
} from "./contextUsage";
import type { ContextForm } from "./rowLayout";

/** Segments of the "segments" form: one per tenth of the window. */
export const CONTEXT_SEGMENTS = 10;

/** The run time in its cell: the words for screen readers, the short form on screen. */
export function RuntimeCell({ runtime }: { runtime: string | null }) {
  const t = useT();
  return (
    <span className="session-runtime row-fact">
      {runtime !== null && (
        <>
          <span className="sr-only">{t("session.runtime", { duration: runtime })}</span>
          <span aria-hidden="true">{runtime}</span>
        </>
      )}
    </span>
  );
}

/**
 * The context in the chosen form. The ring shows also without numbers (a faint empty ring keeps
 * the rows' shape); the other forms show nothing then.
 */
export function ContextFact({
  context,
  language,
  form,
}: {
  context: SessionContext | undefined;
  language: string;
  form: ContextForm;
}) {
  if (form === "ring") return <ContextRing context={context} />;
  if (context === undefined) return null;
  const share = contextShare(context);
  if (form === "mini") return <ContextMini share={share} language={language} />;
  // A bar or segments take the line's free width; the share alone keeps its cell.
  const className = [
    "session-context",
    "row-fact",
    form === "number" ? "" : "grows",
    share >= HIGH_SHARE ? "high" : "",
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <span className={className}>
      {form === "segments" && <ContextSegments share={share} />}
      {form === "bar" && (
        <span className="session-context-bar" aria-hidden="true">
          <span
            className="session-context-fill"
            style={{ width: `${String(Math.round(share * 100))}%` }}
          />
        </span>
      )}
      <span className="session-context-share" aria-hidden="true">
        {formatShare(share, language)}
      </span>
    </span>
  );
}

/** "215.5k / 1M" in its cell; nothing without context numbers. */
export function ContextTokens({ context }: { context: SessionContext | undefined }) {
  if (context === undefined) return null;
  const high = contextShare(context) >= HIGH_SHARE;
  return (
    <span
      className={high ? "session-context-tokens row-fact high" : "session-context-tokens row-fact"}
      aria-hidden="true"
    >
      {compactTokens(context.used)} / {compactTokens(context.window)}
    </span>
  );
}

/** The share in ten still segments, like a battery: calmer than the stripes. */
export function ContextSegments({ share }: { share: number }) {
  const lit = Math.round(share * CONTEXT_SEGMENTS);
  return (
    <span className="session-context-segments" aria-hidden="true">
      {Array.from({ length: CONTEXT_SEGMENTS }, (_, index) => (
        <span key={index} className={index < lit ? "segment-on" : undefined} />
      ))}
    </span>
  );
}

/** A small bar with the share after it, for a line that holds more than the context. */
function ContextMini({ share, language }: { share: number; language: string }) {
  return (
    <span
      className={share >= HIGH_SHARE ? "session-context-mini high" : "session-context-mini"}
      aria-hidden="true"
    >
      <span className="session-context-bar">
        <span
          className="session-context-fill"
          style={{ width: `${String(Math.round(share * 100))}%` }}
        />
      </span>
      <span className="session-context-share">{formatShare(share, language)}</span>
    </span>
  );
}

/**
 * The share as a ring with the percent in its middle; a faint empty ring without numbers, so the
 * rows keep their shape.
 */
export function ContextRing({ context }: { context: SessionContext | undefined }) {
  const share = context === undefined ? null : contextShare(context);
  const className = [
    "session-ring",
    share === null ? "empty" : "",
    share !== null && share >= HIGH_SHARE ? "high" : "",
  ]
    .filter(Boolean)
    .join(" ");
  const percent = share === null ? 0 : Math.round(share * 100);
  return (
    <span className={className} aria-hidden="true">
      {/* The gradient is the ring; styles.css cuts its middle out with a mask. */}
      <span
        className="session-ring-track"
        style={{
          background: `conic-gradient(var(--ring-fill) ${String(percent)}%, var(--ring-rest) 0)`,
        }}
      />
      {share !== null && <span className="session-ring-value">{percent}</span>}
    </span>
  );
}
