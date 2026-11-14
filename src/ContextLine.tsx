// The fourth line of a session row: how long the process runs, in a cell of fixed width at the
// start, then how full its context window is, as a striped bar across the free width, and the
// share and the tokens used of the window in cells of fixed width at the right edge
// ("1 h  ━━━━──  22 %  215.5k / 1M"), so every value lines up from row to row (styles.css,
// "Fourth line"). The exact numbers are for screen readers. No tooltip: session rows have none.
// A row without context numbers (before the agent answered, or a program Hangar cannot read)
// keeps the run time in the same place; without either the line is gone. Some row styles
// (rowStyle.ts) show the context as ten segments, a small bar or a ring instead; their shapes
// live here too. All of them are pure: the row passes the numbers (useSessionContext).

import { useT } from "./i18n";
import {
  compactTokens,
  contextHint,
  contextShare,
  formatShare,
  HIGH_SHARE,
  type SessionContext,
} from "./contextUsage";

/** Segments of the "segments" row style: one per tenth of the window. */
export const CONTEXT_SEGMENTS = 10;

interface Props {
  /** "1 h"; `null` below a minute and for an ended session. */
  runtime: string | null;
  context: SessionContext | undefined;
  /** UI language, for the percent format. */
  language: string;
  /**
   * How the line shows the context: the striped bar, ten still segments, or nothing visible
   * (another part of the row shows it); the words for screen readers stay in every case.
   */
  meter?: "bar" | "segments" | "none";
  /** The run time's cell at the start; false where the row shows the run time elsewhere. */
  runtimeCell?: boolean;
}

/** The line, or nothing at all without a run time and context numbers. */
export function ContextLineCells({
  runtime,
  context,
  language,
  meter = "bar",
  runtimeCell = true,
}: Props) {
  const t = useT();
  const shownRuntime = runtimeCell ? runtime : null;
  if (shownRuntime === null && context === undefined) return null;
  const hint = context === undefined ? null : contextHint(context, language, t);
  // Nothing visible left: only the words, without an empty line.
  if (meter === "none" && shownRuntime === null) {
    return hint === null ? null : <span className="sr-only">{hint}</span>;
  }
  const share = context === undefined ? 0 : contextShare(context);
  return (
    <span className={share >= HIGH_SHARE ? "session-context high" : "session-context"}>
      {/* Kept as an empty cell below a minute, so the bar starts where it does in every row. */}
      {runtimeCell && <RuntimeCell runtime={shownRuntime} />}
      {context !== undefined && meter !== "none" && (
        <>
          {meter === "segments" ? (
            <ContextSegments share={share} />
          ) : (
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
          <span className="session-context-tokens" aria-hidden="true">
            {compactTokens(context.used)} / {compactTokens(context.window)}
          </span>
        </>
      )}
      {hint !== null && <span className="sr-only">{hint}</span>}
    </span>
  );
}

/** The run time in its cell: the words for screen readers, the short form on screen. */
export function RuntimeCell({ runtime }: { runtime: string | null }) {
  const t = useT();
  return (
    <span className="session-runtime">
      {runtime !== null && (
        <>
          <span className="sr-only">{t("session.runtime", { duration: runtime })}</span>
          <span aria-hidden="true">{runtime}</span>
        </>
      )}
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

/**
 * A small bar, with the share after it where `withShare` asks for it: on the title line of a
 * folded row, on the folder line of a compact one. Nothing without context numbers.
 */
export function ContextMini({
  context,
  language,
  withShare,
}: {
  context: SessionContext | undefined;
  language: string;
  withShare: boolean;
}) {
  if (context === undefined) return null;
  const share = contextShare(context);
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
      {withShare && <span className="session-context-share">{formatShare(share, language)}</span>}
    </span>
  );
}

/**
 * The share as a ring with the percent in its middle; a faint empty ring without numbers, so the
 * rows keep their shape. Decorative: the fourth line's words stay for screen readers.
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
