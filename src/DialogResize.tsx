// Resizing a large dialog by its edges and corners, like a window (dialogSize.ts holds the rules).
// `useDialogSize` keeps the chosen size and clamps it to the window; `DialogResizeHandles` draws
// the grips inside the dialog. While a grip is dragged the size goes straight to the dialog's
// style, at most once per frame; React and the stored value get it once, when the drag ends, so
// the dialog's content does not render again on every pointer move.

import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore } from "react";

import {
  clampDialogSize,
  dragDialogSize,
  loadDialogSize,
  RESIZE_EDGES,
  saveDialogSize,
  snapDialogSize,
  stepDialogSize,
  type DialogSize,
  type ResizeEdge,
} from "./dialogSize";
import { useT, type MessageKey } from "./i18n";

/** The sizes one resizable dialog works with. */
export interface DialogSizeBounds {
  /** localStorage key for the remembered size. */
  storageKey: string;
  /** The size it opens at without a remembered one, and after a double click on a grip. */
  standard: DialogSize;
  /** Below this the content would break; a smaller window still wins. */
  min: DialogSize;
}

export interface DialogSizeState {
  /** The shown size: the chosen one within the limits of the window. */
  size: DialogSize;
  viewport: DialogSize;
  min: DialogSize;
  /** For the dialog's `style`: the shown size, placed on whole device pixels. */
  style: React.CSSProperties;
  /** Keeps a size the user chose and remembers it. */
  resize: (size: DialogSize) => void;
  /** Back to the standard size; the remembered one is forgotten. */
  reset: () => void;
}

function subscribeViewport(onChange: () => void): () => void {
  window.addEventListener("resize", onChange);
  return () => {
    window.removeEventListener("resize", onChange);
  };
}

const viewportWidth = () => window.innerWidth;
const viewportHeight = () => window.innerHeight;

/** The size as CSS lengths, placed on whole device pixels (`snapDialogSize`). */
function pixels(size: DialogSize, viewport: DialogSize): { width: string; height: string } {
  const snapped = snapDialogSize(size, viewport, window.devicePixelRatio);
  return { width: `${String(snapped.width)}px`, height: `${String(snapped.height)}px` };
}

export function useDialogSize(bounds: DialogSizeBounds): DialogSizeState {
  const width = useSyncExternalStore(subscribeViewport, viewportWidth);
  const height = useSyncExternalStore(subscribeViewport, viewportHeight);
  const viewport = { width, height };
  const [chosen, setChosen] = useState<DialogSize>(
    () => loadDialogSize(bounds.storageKey) ?? bounds.standard,
  );
  const size = clampDialogSize(chosen, viewport, bounds.min);
  return {
    size,
    viewport,
    min: bounds.min,
    style: pixels(size, viewport),
    resize: (next) => {
      setChosen(next);
      saveDialogSize(bounds.storageKey, next);
    },
    reset: () => {
      setChosen(bounds.standard);
      saveDialogSize(bounds.storageKey, null);
    },
  };
}

const EDGE_LABELS: Record<ResizeEdge, MessageKey> = {
  n: "dialog.resizeHeight",
  s: "dialog.resizeHeight",
  e: "dialog.resizeWidth",
  w: "dialog.resizeWidth",
  ne: "dialog.resizeSize",
  nw: "dialog.resizeSize",
  se: "dialog.resizeSize",
  sw: "dialog.resizeSize",
};

/** The grip that takes the keyboard focus: the corner a window is usually resized by. */
const KEYBOARD_EDGE: ResizeEdge = "se";

interface Drag {
  edge: ResizeEdge;
  x: number;
  y: number;
  start: DialogSize;
  dialog: HTMLElement;
  frame: number;
  next: DialogSize | null;
}

/**
 * The grips on all edges and corners of a resizable dialog; render them as the dialog's last
 * children. Only the lower right corner is in the Tab order: there the arrow keys change the
 * size, so the dialog does not get eight extra stops.
 */
export function DialogResizeHandles({ state }: { state: DialogSizeState }) {
  const t = useT();
  const drag = useRef<Drag | null>(null);
  const latest = useRef(state);
  useLayoutEffect(() => {
    latest.current = state;
  });

  // Closed mid-drag: no frame may write to the detached dialog.
  useEffect(
    () => () => {
      const current = drag.current;
      if (current) window.cancelAnimationFrame(current.frame);
      drag.current = null;
    },
    [],
  );

  const endDrag = () => {
    const current = drag.current;
    if (!current) return;
    drag.current = null;
    window.cancelAnimationFrame(current.frame);
    if (current.next !== null) latest.current.resize(current.next);
  };

  return RESIZE_EDGES.map((edge) => {
    const label = t(EDGE_LABELS[edge]);
    const focusable = edge === KEYBOARD_EDGE;
    return (
      <div
        key={edge}
        role="separator"
        aria-orientation={edge === "n" || edge === "s" ? "horizontal" : "vertical"}
        aria-label={label}
        // A focusable separator is a widget and needs a value; the grip changes both sides.
        aria-valuenow={focusable ? state.size.width : undefined}
        aria-valuetext={
          focusable ? `${String(state.size.width)} × ${String(state.size.height)} px` : undefined
        }
        title={t("splitter.resetHint", { label })}
        tabIndex={focusable ? 0 : undefined}
        className={`dialog-resize ${edge}`}
        onPointerDown={(event) => {
          if (event.button !== 0) return;
          const dialog = event.currentTarget.parentElement;
          if (!dialog) return;
          event.currentTarget.setPointerCapture(event.pointerId);
          drag.current = {
            edge,
            x: event.clientX,
            y: event.clientY,
            start: latest.current.size,
            dialog,
            frame: 0,
            next: null,
          };
          // No text selection and no focus change while dragging.
          event.preventDefault();
        }}
        onPointerMove={(event) => {
          const current = drag.current;
          if (!current) return;
          const { viewport, min } = latest.current;
          current.next = dragDialogSize(
            current.start,
            current.edge,
            { x: event.clientX - current.x, y: event.clientY - current.y },
            viewport,
            min,
          );
          // At most one layout per frame, however fast the pointer reports.
          if (current.frame !== 0) return;
          current.frame = window.requestAnimationFrame(() => {
            const live = drag.current;
            if (!live) return;
            live.frame = 0;
            if (live.next === null) return;
            const style = pixels(live.next, latest.current.viewport);
            live.dialog.style.width = style.width;
            live.dialog.style.height = style.height;
          });
        }}
        onPointerUp={(event) => {
          event.currentTarget.releasePointerCapture(event.pointerId);
          endDrag();
        }}
        onPointerCancel={endDrag}
        // The window lost the pointer mid-drag (focus change): no pointerup follows.
        onLostPointerCapture={endDrag}
        onDoubleClick={state.reset}
        onKeyDown={
          focusable
            ? (event) => {
                const next = stepDialogSize(state.size, event.key, state.viewport, state.min);
                if (next === null) return;
                event.preventDefault();
                state.resize(next);
              }
            : undefined
        }
      />
    );
  });
}
