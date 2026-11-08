import { useRef } from "react";

import { useT } from "./i18n";

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
 * the pointer and the line after it hits a limit.
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
  const drag = useRef<{ start: number; value: number } | null>(null);
  const position = (event: React.PointerEvent) =>
    orientation === "vertical" ? event.clientX : event.clientY;

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
        drag.current = { start: position(event), value };
        document.body.classList.add(`resizing-${orientation}`);
        event.preventDefault();
      }}
      onPointerMove={(event) => {
        const start = drag.current;
        if (!start) return;
        onChange(start.value + direction * (position(event) - start.start));
      }}
      onPointerUp={(event) => {
        drag.current = null;
        event.currentTarget.releasePointerCapture(event.pointerId);
        document.body.classList.remove(`resizing-${orientation}`);
      }}
      onPointerCancel={() => {
        drag.current = null;
        document.body.classList.remove(`resizing-${orientation}`);
      }}
      onDoubleClick={onReset}
      onKeyDown={(event) => {
        const grow = orientation === "vertical" ? "ArrowRight" : "ArrowDown";
        const shrink = orientation === "vertical" ? "ArrowLeft" : "ArrowUp";
        if (event.key === grow) onChange(value + direction * KEY_STEP);
        else if (event.key === shrink) onChange(value - direction * KEY_STEP);
        else return;
        event.preventDefault();
      }}
    />
  );
}
