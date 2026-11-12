import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";

import {
  PEEK_DELAY_MS,
  PEEK_REFRESH_MS,
  peekPosition,
  peekWidth,
  sameLines,
  type PeekAnchor,
} from "./peek";

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
  /** What the row's tooltip would warn about (peekNotes), one muted line each. */
  notes: readonly string[];
  /** Badge of the session's program. */
  badge: React.ReactNode;
  /** CSS class of the state's edge color (`edge-*`, sessionChip.ts). */
  edge: string;
  /** The session's last lines; `null` when it has none to show (say, it is on screen). */
  readLines: () => readonly string[] | null;
}

/**
 * The last lines of a session's terminal beside the sidebar, read from its buffer (no extra
 * terminal), refreshed every PEEK_REFRESH_MS. Purely visual: hidden from screen readers, never
 * focused and transparent to the mouse, so the row below keeps its hover and its click.
 */
export function SessionPeek({ anchor, title, status, notes, badge, edge, readLines }: Props) {
  const reader = useRef(readLines);
  useEffect(() => {
    reader.current = readLines;
  });
  const [lines, setLines] = useState<readonly string[] | null>(() => readLines());
  const box = useRef<HTMLDivElement>(null);
  const [place, setPlace] = useState<{ left: number; top: number } | null>(null);

  useEffect(() => {
    const timer = window.setInterval(() => {
      const fresh = reader.current();
      setLines((current) => (sameLines(current, fresh) ? current : fresh));
    }, PEEK_REFRESH_MS);
    return () => {
      window.clearInterval(timer);
    };
  }, [anchor.id]);

  const shown = lines !== null && lines.length > 0;
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
      <pre className="session-peek-lines">{lines.join("\n")}</pre>
    </div>,
    document.body,
  );
}
