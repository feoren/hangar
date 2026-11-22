import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";

import type { AttentionState } from "./attention";
import { CrewColumn } from "./CrewColumn";
import { crewForm, type CrewForm } from "./crew";
import { SessionLook } from "./SessionLook";
import { useT } from "./i18n";
import type { BadgeLook } from "./programPrefs";
import { SessionMarker } from "./SessionMarker";
import { paneResizing, sidebarMotion, sidePanelMotion, windowResizing } from "./sidebarMotion";
import type { SessionView } from "./sessions";
import { zoneRect, type LayoutId, type Ratios } from "./snapLayouts";
import type { SessionTerminal } from "./terminal";

/** Header and position of a pane in split view. */
export interface PaneSplit {
  layout: Exclude<LayoutId, "single">;
  slot: number;
  /** Splitter positions of the layout (paneRatios.ts). */
  ratios: Ratios;
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
  /**
   * The sub-agents beside the terminal (CrewColumn.tsx); `null`: no crew for this pane. In a pane
   * too narrow for the column the same crew is a strip above the terminal.
   */
  crew?: Omit<React.ComponentProps<typeof CrewColumn>, "form"> | null;
  /** Overlays inside the terminal area, e.g. the search bar. */
  children?: React.ReactNode;
}

/** Gap between panes and to the edge in split view (px). */
const OUTER = 6;
const INNER = 3;

/**
 * Absolute position of a pane in split view: its zone (snapLayouts.ts) plus the gaps. Absolute
 * positions instead of a CSS grid, because hidden panes stay in the DOM (invisible, at the size
 * of the whole panes area, so the scrollback survives) and must not take part in a grid's flow.
 */
export function paneStyle(layout: LayoutId, slot: number, ratios?: Ratios): React.CSSProperties {
  const rect = zoneRect(layout, slot, ratios);
  const start = (offset: number) => (offset === 0 ? OUTER : INNER);
  // Tolerance: a zone after a moved splitter ends at `a + (1 - a)`, which may miss 1 by a rounding.
  const end = (offset: number, size: number) => (offset + size >= 1 - 1e-9 ? OUTER : INNER);
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
  const shown = useRef(visible);
  const fitRef = useRef<(() => void) | null>(null);
  const paneRef = useRef<HTMLDivElement>(null);
  const [form, setForm] = useState<CrewForm>("column");
  const hasCrew = props.crew !== undefined && props.crew !== null;

  // The crew takes the form the pane's width allows (crew.ts).
  useEffect(() => {
    const element = paneRef.current;
    if (!hasCrew || !element) return;
    const observer = new ResizeObserver((entries) => {
      const width = entries[0]?.contentRect.width;
      if (width !== undefined && width > 0) setForm(crewForm(width));
    });
    observer.observe(element);
    return () => {
      observer.disconnect();
    };
  }, [hasCrew]);

  useEffect(() => {
    const element = host.current;
    if (!element) return;
    terminal.mount(element);
    // While the sidebar folds, the right panel slides or a splitter moves, the fit waits for the
    // end of the motion (sidebarMotion.ts). While the window is resized, only the panes on screen
    // follow; hidden ones fit once it rests. One stable function, so it waits at most once per gate.
    const fit = () => {
      if (sidebarMotion.isMoving) sidebarMotion.whenSettled(fit);
      else if (sidePanelMotion.isMoving) sidePanelMotion.whenSettled(fit);
      else if (paneResizing.isMoving) paneResizing.whenSettled(fit);
      else if (windowResizing.isMoving && !shown.current) windowResizing.whenSettled(fit);
      else terminal.fit();
    };
    fitRef.current = fit;
    const observer = new ResizeObserver(fit);
    observer.observe(element);
    return () => {
      observer.disconnect();
      fitRef.current = null;
      sidebarMotion.forget(fit);
      sidePanelMotion.forget(fit);
      paneResizing.forget(fit);
      windowResizing.forget(fit);
    };
  }, [terminal]);

  // Before the observer can fire for the new layout. A pane shown while the window still moves
  // does not wait for the rest of the drag.
  useLayoutEffect(() => {
    shown.current = visible;
    if (visible && windowResizing.isMoving) fitRef.current?.();
  }, [visible]);

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
      ref={paneRef}
      className={className}
      data-session={sessionId}
      aria-hidden={!visible}
      role={split !== null && visible ? "group" : undefined}
      aria-label={split !== null && visible ? split.session.title : undefined}
      style={
        split !== null && visible ? paneStyle(split.layout, split.slot, split.ratios) : undefined
      }
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
          <SessionLook look={split.look} session={split.session} />
          <SessionMarker marker={split.session.marker} />
          <span className="pane-title">{split.session.title}</span>
          <span
            className={`dot ${split.state === "attention" ? "attention" : split.session.status}`}
            aria-hidden="true"
          />
          <span className="sr-only">{split.status}</span>
        </div>
      )}
      {visible && <LastPromptBar terminal={terminal} />}
      {visible && props.crew && form === "strip" && <CrewColumn {...props.crew} form="strip" />}
      <div className="pane-row">
        <div className="pane-body">
          <div ref={host} className="pane-host" />
          {dropping && (
            <div className="drop-hint" aria-hidden="true">
              <span className="drop-hint-label">{t("drop.hint")}</span>
            </div>
          )}
          {props.children}
        </div>
        {visible && props.crew && form === "column" && <CrewColumn {...props.crew} form="column" />}
      </div>
    </div>
  );
}

/**
 * The user's last prompt in this session, one line above the terminal; a click scrolls back to
 * where it was sent (terminal.ts `revealLastPrompt`). Nothing before the first prompt, and in
 * plain shells, whose command lines are no prompts.
 */
function LastPromptBar({ terminal }: { terminal: SessionTerminal }) {
  const t = useT();
  const prompt = useSyncExternalStore(
    (listener) => terminal.subscribeLastPrompt(listener),
    () => terminal.lastPrompt(),
  );
  if (prompt === null) return null;
  const label = t("prompt.last");
  if (!prompt.reachable) {
    return (
      <div className="last-prompt" title={prompt.text}>
        <span className="sr-only">{label}: </span>
        <span className="last-prompt-text">{prompt.text}</span>
      </div>
    );
  }
  return (
    <button
      type="button"
      className="last-prompt"
      title={`${prompt.text}
${t("prompt.jump")}`}
      aria-label={`${label}: ${prompt.text}. ${t("prompt.jump")}`}
      // The keyboard stays in the terminal; the click itself focuses it again.
      onMouseDown={(event) => {
        event.preventDefault();
      }}
      onClick={() => {
        terminal.revealLastPrompt();
      }}
    >
      <span className="last-prompt-mark" aria-hidden="true">
        ›
      </span>
      <span className="last-prompt-text">{prompt.text}</span>
    </button>
  );
}

/** Placeholder for a pane without a session: says how to fill it. */
export function EmptyPane({
  layout,
  slot,
  ratios,
}: {
  layout: Exclude<LayoutId, "single">;
  slot: number;
  ratios: Ratios;
}) {
  const t = useT();
  return (
    <div className="pane-empty" style={paneStyle(layout, slot, ratios)}>
      <p>{t("split.empty")}</p>
    </div>
  );
}
