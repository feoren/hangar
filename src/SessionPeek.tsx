import { Fragment, memo, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import {
  PEEK_DELAY_MS,
  PEEK_REFRESH_MS,
  peekPosition,
  peekWidth,
  peekShows,
  type PeekAnchor,
} from "./peek";
import { runStyle, sameLine, samePeekContent, type PeekContent, type PeekLine } from "./peekStyle";

/** Open menus and modal dialogs own the screen; a peek would cover them or hide behind them. */
const PEEK_BLOCKERS = '.menu-panel, [aria-modal="true"]';

/**
 * When the mouse rests on a session row: which row, once PEEK_DELAY_MS have passed. Leaving the
 * row, pressing a button on it, any key and the window losing focus end the peek; the sidebar ends
 * it on scrolling (`onScrollCapture`). `enabled` off (say, while a row is dragged) ends it too
 * and starts none.
 */
export function useSessionPeek(enabled: boolean) {
  const [peek, setPeek] = useState<PeekAnchor | null>(null);
  const timer = useRef<number | null>(null);

  const leave = useCallback(() => {
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
    setPeek(null);
  }, []);

  const enter = useCallback(
    (id: number, row: HTMLElement) => {
      leave();
      if (!enabled) return;
      timer.current = window.setTimeout(() => {
        timer.current = null;
        if (!row.isConnected || document.querySelector(PEEK_BLOCKERS) !== null) return;
        const rect = row.getBoundingClientRect();
        const sidebar = row.closest(".sidebar")?.getBoundingClientRect().right ?? rect.right;
        const width = peekWidth(sidebar, window.innerWidth);
        if (width === null) return;
        setPeek({ id, top: rect.top, bottom: rect.bottom, sidebarRight: sidebar, width });
      }, PEEK_DELAY_MS);
    },
    [enabled, leave],
  );

  // Ends a pending peek as well as a shown one, so nothing appears after the user moved on.
  useEffect(() => {
    // Any key ends it, so a menu opened by keyboard is never covered. Only listens: the key
    // still reaches the terminal or the dialog it was meant for.
    const onKey = () => {
      leave();
    };
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("blur", leave);
    window.addEventListener("resize", leave);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("blur", leave);
      window.removeEventListener("resize", leave);
      if (timer.current !== null) window.clearTimeout(timer.current);
    };
  }, [leave]);

  // A drag or rename that starts while a peek shows hides it at once.
  return { peek: enabled ? peek : null, enter, leave };
}

interface Props {
  anchor: PeekAnchor;
  title: string;
  /** The state in words, so color is never the only signal. */
  status: string;
  /** What the row only hints at, spelled out (peekNotes), one muted line each. */
  notes: readonly string[];
  /** Badge of the session's program. */
  badge: React.ReactNode;
  /** CSS class of the state's edge color (`edge-*`, sessionChip.ts). */
  edge: string;
  /**
   * The session's last lines (`null` when it has none yet); no reader for a session on screen
   * anyway, whose peek then shows only its title, status and notes.
   */
  readLines: (() => PeekContent | null) | null;
}

/**
 * The last lines of a session's terminal beside the sidebar, read from its buffer (no extra
 * terminal), refreshed every PEEK_REFRESH_MS; for a session on screen only its title, status and
 * notes. Purely visual: hidden from screen readers, never
 * focused and transparent to the mouse, so the row below keeps its hover and its click.
 */
export function SessionPeek({ anchor, title, status, notes, badge, edge, readLines }: Props) {
  const reader = useRef(readLines);
  useEffect(() => {
    reader.current = readLines;
  });
  const [content, setContent] = useState<PeekContent | null>(() => readLines?.() ?? null);
  const lines = content?.lines ?? null;
  const box = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<{ left: number; top: number } | null>(null);

  useEffect(() => {
    const timer = window.setInterval(() => {
      const fresh = reader.current?.() ?? null;
      setContent((current) => (samePeekContent(current, fresh) ? current : fresh));
    }, PEEK_REFRESH_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, [anchor.id]);

  const shown = peekShows(readLines !== null, lines);
  const lineCount = lines?.length ?? 0;
  // Measured before paint, so the peek never shows at a wrong place first.
  useLayoutEffect(() => {
    const element = box.current;
    if (!element) return;
    setPlace(
      peekPosition(
        anchor,
        { width: element.offsetWidth, height: element.offsetHeight },
        { width: window.innerWidth, height: window.innerHeight },
      ),
    );
  }, [anchor, shown, lineCount, notes.length]);

  if (!shown) return null;
  return createPortal(
    <div
      ref={box}
      className={`session-peek ${edge}`}
      aria-hidden="true"
      style={
        place === null
          ? { visibility: "hidden", width: `${String(anchor.width)}px` }
          : {
              left: `${String(place.left)}px`,
              top: `${String(place.top)}px`,
              width: `${String(anchor.width)}px`,
            }
      }
    >
      <div className="session-peek-head">
        {badge}
        <span className="session-peek-name">{title}</span>
        <span className="session-peek-status">{status}</span>
      </div>
      {notes.map((note) => (
        <span key={note} className="session-peek-note">
          {note}
        </span>
      ))}
      {content !== null && content.lines.length > 0 && (
        // The scheme's own background and text color: the colors programs pick are meant for
        // it, and the peek then reads like a small copy of the terminal.
        <pre
          className="session-peek-lines"
          style={{ background: content.background, color: content.foreground }}
        >
          {content.lines.map((line, index) => (
            <PeekLineView key={index} line={line} last={index === content.lines.length - 1} />
          ))}
        </pre>
      )}
    </div>,
    document.body,
  );
}

/** One line of the peek; unchanged lines skip rendering (sameLine). */
const PeekLineView = memo(
  function PeekLineView({ line, last }: { line: PeekLine; last: boolean }) {
    return (
      <>
        {line.map((run, index) => {
          const style = runStyle(run);
          return style === undefined ? (
            <Fragment key={index}>{run.text}</Fragment>
          ) : (
            <span key={index} style={style}>
              {run.text}
            </span>
          );
        })}
        {!last && "\n"}
      </>
    );
  },
  (before, after) => before.last === after.last && sameLine(before.line, after.line),
);
