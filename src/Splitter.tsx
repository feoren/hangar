import { useEffect, useLayoutEffect, useRef } from "react";

import { useT } from "./i18n";
import { paneResizing, SPLITTER_KEY_SETTLE_MS } from "./sidebarMotion";

interface Props {
  /** `vertical`: the line stands upright and is dragged left and right. */
  orientation: "vertical" | "horizontal";
  /** Current value (width or height in px) that dragging starts from. */
  value: number;
  /** +1: dragging right/down increases the value, -1: decreases it. */
  direction: 1 | -1;
  label: string;
  onChange: (value: number) => void;
  onReset: () => void;
  className?: string;
}

const KEY_STEP = 16;

/**
 * Draggable splitter. Computes absolutely from the start value, so no offset builds up between
 * the pointer and the line after it hits a limit. The layout follows the pointer at most once per
 * frame; the terminals keep their size until the drag ends (paneResizing gate), so the programs in
 * them redraw once instead of on every frame.
 */
export function Splitter({
  orientation,
  value,
  direction,
  label,
  onChange,
  onReset,
  className,
}: Props) {
  const t = useT();
  const drag = useRef<{ start: number; value: number; frame: number; next: number | null } | null>(
    null,
  );
  const keyTimer = useRef<number | undefined>(undefined);
  /** This splitter holds the gate (drag or key steps); only then does it open it again. */
  const holding = useRef(false);
  const change = useRef(onChange);
  useLayoutEffect(() => {
    change.current = onChange;
  }, [onChange]);

  const hold = () => {
    holding.current = true;
    paneResizing.setMoving(true);
  };
  const release = () => {
    if (!holding.current) return;
    holding.current = false;
    paneResizing.setMoving(false);
  };

  // Unmounted mid-drag or mid-keyboard (the panel closed): the waiting terminals fit right away.
  useEffect(
    () => () => {
      const current = drag.current;
      if (current) window.cancelAnimationFrame(current.frame);
      drag.current = null;
      window.clearTimeout(keyTimer.current);
      document.body.classList.remove(`resizing-${orientation}`);
      if (holding.current) {
        holding.current = false;
        paneResizing.setMoving(false);
      }
    },
    [orientation],
  );

  const position = (event: React.PointerEvent) =>
    orientation === "vertical" ? event.clientX : event.clientY;

  const endDrag = () => {
    const current = drag.current;
    if (!current) return;
    drag.current = null;
    window.cancelAnimationFrame(current.frame);
    if (current.next !== null) change.current(current.next);
    document.body.classList.remove(`resizing-${orientation}`);
    // One frame later the final sizes are laid out, so each terminal fits once, to them.
    window.requestAnimationFrame(release);
  };

  return (
    <div
      role="separator"
      aria-orientation={orientation}
      aria-label={label}
      aria-valuenow={value}
      tabIndex={0}
      title={t("splitter.resetHint", { label })}
      className={`splitter ${orientation} ${className ?? ""}`.trim()}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = { start: position(event), value, frame: 0, next: null };
        hold();
        document.body.classList.add(`resizing-${orientation}`);
        event.preventDefault();
      }}
      onPointerMove={(event) => {
        const current = drag.current;
        if (!current) return;
        current.next = current.value + direction * (position(event) - current.start);
        // At most one layout per frame, however fast the pointer reports.
        if (current.frame !== 0) return;
        current.frame = window.requestAnimationFrame(() => {
          const live = drag.current;
          if (!live) return;
          live.frame = 0;
          if (live.next !== null) change.current(live.next);
        });
      }}
      onPointerUp={(event) => {
        event.currentTarget.releasePointerCapture(event.pointerId);
        endDrag();
      }}
      onPointerCancel={endDrag}
      // The window lost the pointer mid-drag (focus change): no pointerup follows, so the drag
      // ends here, or the terminals would wait for a fit that never comes.
      onLostPointerCapture={endDrag}
      onDoubleClick={onReset}
      onKeyDown={(event) => {
        const grow = orientation === "vertical" ? "ArrowRight" : "ArrowDown";
        const shrink = orientation === "vertical" ? "ArrowLeft" : "ArrowUp";
        let next: number;
        if (event.key === grow) next = value + direction * KEY_STEP;
        else if (event.key === shrink) next = value - direction * KEY_STEP;
        else return;
        event.preventDefault();
        // Held keys repeat: the terminals fit once the keys rest.
        hold();
        window.clearTimeout(keyTimer.current);
        keyTimer.current = window.setTimeout(release, SPLITTER_KEY_SETTLE_MS);
        onChange(next);
      }}
    />
  );
}
