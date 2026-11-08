import { useEffect, useRef } from "react";

import type { AttentionState } from "./attention";
import { Badge } from "./Badge";
import { useT } from "./i18n";
import type { BadgeLook } from "./programPrefs";
import { SessionMarker } from "./SessionMarker";
import type { SessionView } from "./sessions";
import { slotRect, type SplitMode } from "./split";
import type { SessionTerminal } from "./terminal";

/** Header and position of a pane in split view. */
export interface PaneSplit {
  mode: Exclude<SplitMode, "single">;
  slot: number;
  session: SessionView;
  look: BadgeLook;
  state: AttentionState;
  /** Status text for the header tooltip and screen readers ("waiting for you"). */
  status: string;
}

interface Props {
  terminal: SessionTerminal;
  /** Session ID; drag and drop finds the pane under the cursor by it (`data-session`). */
  sessionId: number;
  /** Shown in the focused pane: gets the keyboard. */
  active: boolean;
  /** On screen (single view: the active one; split view: every session in a pane). */
  visible: boolean;
  /** Split view: header and position; `null` in single view. */
  split: PaneSplit | null;
  /** Files are being dragged over this pane: show where they will land. */
  dropping: boolean;
  /** Clicked in split view: this pane gets the focus. */
  onFocusPane: (id: number) => void;
  /** Double click on the header: show only this session. */
  onMaximize: (id: number) => void;
  /** Right click on the header: the session menu. */
  onHeaderMenu: (id: number, x: number, y: number) => void;
  /** Overlays inside the terminal area, e.g. the search bar. */
  children?: React.ReactNode;
}

/** Gap between panes and to the edge in split view (px). */
const OUTER = 6;
const INNER = 3;

/** Absolute position of a pane in split view; fractions from split.ts plus the gaps. */
export function paneStyle(mode: SplitMode, slot: number): React.CSSProperties {
  const rect = slotRect(mode, slot);
  const start = (offset: number) => (offset === 0 ? OUTER : INNER);
  const end = (offset: number, size: number) => (offset + size >= 1 ? OUTER : INNER);
  return {
    left: `calc(${String(rect.x * 100)}% + ${String(start(rect.x))}px)`,
    top: `calc(${String(rect.y * 100)}% + ${String(start(rect.y))}px)`,
    width: `calc(${String(rect.width * 100)}% - ${String(start(rect.x) + end(rect.x, rect.width))}px)`,
    height: `calc(${String(rect.height * 100)}% - ${String(start(rect.y) + end(rect.y, rect.height))}px)`,
  };
}

/**
 * All panes stay in the DOM; hidden ones are merely invisible. That way they keep their size, the
 * scrollback survives, and switching needs no rebuild. xterm lives in its own host element, so
 * React never touches the nodes xterm creates. In split view a pane only changes its position;
 * the ResizeObserver fits the terminal and the PTY follows through `onResize`.
 */
export function TerminalPane(props: Props) {
  const { terminal, sessionId, active, visible, split, dropping } = props;
  const t = useT();
  const host = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const element = host.current;
    if (!element) return;
    terminal.mount(element);
    const observer = new ResizeObserver(() => {
      terminal.fit();
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, [terminal]);

  useEffect(() => {
    if (!active) return;
    terminal.fit();
    terminal.focus();
  }, [active, terminal]);

  const className = [
    "pane",
    visible ? "visible" : "",
    active ? "active" : "",
    split !== null ? "split" : "",
    split?.state === "attention" ? "attention" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      className={className}
      data-session={sessionId}
      aria-hidden={!visible}
      role={split !== null && visible ? "group" : undefined}
      aria-label={split !== null && visible ? split.session.title : undefined}
      style={split !== null && visible ? paneStyle(split.mode, split.slot) : undefined}
      onPointerDownCapture={() => {
        if (split !== null && !active) props.onFocusPane(sessionId);
      }}
    >
      {split !== null && visible && (
        <div
          className="pane-header"
          title={`${split.session.title} · ${split.status}\n${t("split.maximize")}`}
          // A click on the header must not take the focus from the terminal: every key, Ctrl+C
          // included, keeps going to the program. The session menu is reached by keyboard from
          // the sidebar row (context menu key, Shift+F10).
          onMouseDown={(event) => {
            event.preventDefault();
          }}
          onDoubleClick={() => {
            props.onMaximize(sessionId);
          }}
          onContextMenu={(event) => {
            event.preventDefault();
            props.onHeaderMenu(sessionId, event.clientX, event.clientY);
          }}
        >
          <Badge look={split.look} />
          <SessionMarker marker={split.session.marker} />
          <span className="pane-title">{split.session.title}</span>
          <span
            className={`dot ${split.state === "attention" ? "attention" : split.session.status}`}
            aria-hidden="true"
          />
          <span className="sr-only">{split.status}</span>
        </div>
      )}
      <div className="pane-body">
        <div ref={host} className="pane-host" />
        {dropping && (
          <div className="drop-hint" aria-hidden="true">
            <span className="drop-hint-label">{t("drop.hint")}</span>
          </div>
        )}
        {props.children}
      </div>
    </div>
  );
}

/** Placeholder for a pane without a session: says how to fill it. */
export function EmptyPane({ mode, slot }: { mode: Exclude<SplitMode, "single">; slot: number }) {
  const t = useT();
  return (
    <div className="pane-empty" style={paneStyle(mode, slot)}>
      <p>{t("split.empty")}</p>
    </div>
  );
}
