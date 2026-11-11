import { useEffect, useRef } from "react";

import { useT } from "./i18n";
import {
  splitters,
  ratioBounds,
  setRatio,
  stepRatio,
  type AreaSize,
  type ZoneSplitter,
} from "./paneRatios";
import { paneResizing } from "./sidebarMotion";
import type { LayoutId, Ratios } from "./snapLayouts";

interface Props {
  layout: Exclude<LayoutId, "single">;
  ratios: Ratios;
  onChange: (ratios: Ratios) => void;
  onReset: () => void;
}

/** Same gaps as the panes (TerminalPane.tsx): to the area's edge and between two panes. */
const OUTER = 6;
const INNER = 3;
/** Grab width of a splitter (px); it is centered on the gap between two panes. */
const GRAB = 8;
/** After the last arrow key press, the terminals fit after this pause (ms). */
const KEY_SETTLE_MS = 250;

/** Position of a splitter in the panes area: on the gap, as long as the panes beside it. */
function splitterStyle(splitter: ZoneSplitter): React.CSSProperties {
  const at = `calc(${String(splitter.at * 100)}% - ${String(GRAB / 2)}px)`;
  const from = `calc(${String(splitter.from * 100)}% + ${String(splitter.from === 0 ? OUTER : INNER)}px)`;
  const length = `calc(${String((splitter.to - splitter.from) * 100)}% - ${String(
    (splitter.from === 0 ? OUTER : INNER) + (splitter.to >= 1 - 1e-9 ? OUTER : INNER),
  )}px)`;
  return splitter.orientation === "vertical"
    ? { left: at, top: from, width: GRAB, height: length }
    : { top: at, left: from, height: GRAB, width: length };
}

function areaOf(element: HTMLElement): AreaSize {
  const area = element.parentElement?.getBoundingClientRect();
  return { width: area?.width ?? 0, height: area?.height ?? 0 };
}

/**
 * The splitters between the panes of a snap layout. Dragging follows the pointer once per frame;
 * the terminals keep their size until the drag ends (paneResizing gate), so the programs in them
 * redraw once instead of on every frame. Arrow keys step, Home and End go to the limits, a double
 * click gives the layout its preset sizes back.
 */
export function PaneSplitters({ layout, ratios, onChange, onReset }: Props) {
  const t = useT();
  const drag = useRef<{
    index: number;
    start: number;
    ratios: Ratios;
    area: AreaSize;
    frame: number;
    next: number;
  } | null>(null);
  const keyTimer = useRef<number | undefined>(undefined);

  // Unmounted mid-drag or mid-keyboard (layout changed): the waiting terminals fit right away.
  useEffect(
    () => () => {
      const current = drag.current;
      if (current) window.cancelAnimationFrame(current.frame);
      window.clearTimeout(keyTimer.current);
      document.body.classList.remove("resizing-vertical", "resizing-horizontal");
      paneResizing.setMoving(false);
    },
    [],
  );

  const endDrag = (element: HTMLElement, orientation: ZoneSplitter["orientation"]) => {
    const current = drag.current;
    if (!current) return;
    drag.current = null;
    window.cancelAnimationFrame(current.frame);
    onChange(setRatio(layout, current.ratios, current.index, current.next, current.area));
    element.classList.remove("dragging");
    document.body.classList.remove(`resizing-${orientation}`);
    // One frame later the final sizes are laid out, so each terminal fits once, to them.
    window.requestAnimationFrame(() => {
      paneResizing.setMoving(false);
    });
  };

  return (
    <>
      {splitters(layout, ratios).map((splitter) => {
        const { orientation, index } = splitter;
        const label = t(
          orientation === "vertical" ? "split.splitterColumns" : "split.splitterRows",
        );
        const bounds = ratioBounds(layout, ratios, index);
        const position = (event: React.PointerEvent) =>
          orientation === "vertical" ? event.clientX : event.clientY;
        return (
          <div
            key={`${layout}-${String(index)}`}
            role="separator"
            aria-orientation={orientation}
            aria-label={label}
            aria-valuenow={Math.round(splitter.at * 100)}
            aria-valuemin={Math.round(bounds.min * 100)}
            aria-valuemax={Math.round(bounds.max * 100)}
            tabIndex={0}
            title={t("splitter.resetHint", { label })}
            className={`pane-splitter ${orientation}`}
            style={splitterStyle(splitter)}
            onPointerDown={(event) => {
              if (event.button !== 0) return;
              event.currentTarget.setPointerCapture(event.pointerId);
              const at = ratios[index] ?? splitter.at;
              drag.current = {
                index,
                start: position(event),
                ratios,
                area: areaOf(event.currentTarget),
                frame: 0,
                next: at,
              };
              paneResizing.setMoving(true);
              event.currentTarget.classList.add("dragging");
              document.body.classList.add(`resizing-${orientation}`);
              event.preventDefault();
            }}
            onPointerMove={(event) => {
              const current = drag.current;
              if (!current) return;
              const size = orientation === "vertical" ? current.area.width : current.area.height;
              if (!(size > 0)) return;
              const start = current.ratios[current.index] ?? splitter.at;
              current.next = start + (position(event) - current.start) / size;
              // At most one layout per frame, however fast the pointer reports.
              if (current.frame !== 0) return;
              current.frame = window.requestAnimationFrame(() => {
                const live = drag.current;
                if (!live) return;
                live.frame = 0;
                onChange(setRatio(layout, live.ratios, live.index, live.next, live.area));
              });
            }}
            onPointerUp={(event) => {
              event.currentTarget.releasePointerCapture(event.pointerId);
              endDrag(event.currentTarget, orientation);
            }}
            onPointerCancel={(event) => {
              endDrag(event.currentTarget, orientation);
            }}
            // The window lost the pointer mid-drag (focus change): no pointerup follows, so the
            // drag ends here, or the terminals would wait for a fit that never comes.
            onLostPointerCapture={(event) => {
              endDrag(event.currentTarget, orientation);
            }}
            onDoubleClick={onReset}
            onKeyDown={(event) => {
              const next = stepRatio(layout, ratios, index, event.key, areaOf(event.currentTarget));
              if (next === null) return;
              event.preventDefault();
              // Held keys repeat: the terminals fit once the keys rest.
              paneResizing.setMoving(true);
              window.clearTimeout(keyTimer.current);
              keyTimer.current = window.setTimeout(() => {
                paneResizing.setMoving(false);
              }, KEY_SETTLE_MS);
              onChange(next);
            }}
          />
        );
      })}
    </>
  );
}
