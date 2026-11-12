// Dragging a session row out of the sidebar: within the list it reorders, over the panes area it
// places the session in a pane or, through the snap layout bar, in a zone of another layout.
//
// Pointer events with pointer capture instead of HTML5 drag and drop: Tauri's native file drop
// (`dragDropEnabled`, on by default and needed for dropping files from Explorer onto a terminal)
// registers the window as an OLE drop target on Windows, and HTML5 drag and drop inside the page
// then does not work (Tauri's configuration documentation names this). Pointer capture also keeps
// the gesture away from xterm: while captured, mouse events go to the row, so no program in a pane
// sees mouse motion with a pressed button. The two drags never meet: files arrive through
// `onDragDropEvent`, sessions through these pointer events.
//
// Rules (threshold, drop index, autoscroll) are pure functions in sessionDrag.ts. The gesture
// lives outside React in `SessionDrag`; React state changes only when the drop target changes,
// and the ghost under the pointer moves without a render.

import { useEffect, useState } from "react";

import {
  autoscrollStep,
  beyondThreshold,
  dropPlacement,
  sameDragTarget,
  type DragTarget,
} from "./sessionDrag";
import { ratiosFor, type PaneRatios } from "./paneRatios";
import { groupDropIndex } from "./sessionGroups";
import { isLayoutId, zoneAt, type LayoutId } from "./snapLayouts";

/**
 * Attributes the drag reads from the DOM. The sidebar, the panes area and the snap bar set them
 * literally (`data-session-list` and so on); these selectors must match.
 */
const SELECTOR = {
  /** The scrolling session list (expanded sidebar or rail). */
  list: "[data-session-list]",
  /**
   * One row in it; `data-session-row` holds the session ID, `data-session-group` its sidebar
   * group (sessionGroups.ts). The expanded sidebar has one list per group, the rail one for all.
   */
  row: "[data-session-list] [data-session-row]",
  /** The panes area. */
  panes: "[data-panes]",
  /** A layout thumbnail's zone area in the snap bar; `data-snap-layout` holds the layout ID. */
  snapLayout: "[data-snap-layout]",
  snapBar: ".snap-bar",
} as const;

/**
 * Listener options for the capture phase. An object, not the boolean shorthand: some Node
 * versions (the tests run there) do not match `removeEventListener(type, fn, true)` to a
 * listener added with `true`, so the click guard stayed up after a drag.
 */
const CAPTURE: AddEventListenerOptions = { capture: true };

/** Ghost offset from the pointer (px), so the pointer still sees what lies under it. */
const GHOST_OFFSET = 14;

export interface SessionDragView {
  id: number;
  target: DragTarget | null;
  /** The pointer is over the panes area: the snap layout bar shows. */
  overBody: boolean;
}

export interface SessionDragOptions {
  layout: LayoutId;
  /** Splitter positions per layout: drop targets follow the zones as they are drawn. */
  ratios?: PaneRatios;
  /** Session IDs in sidebar order. */
  order: readonly number[];
  onMove: (id: number, index: number) => void;
  onPlace: (id: number, layout: LayoutId, zone: number) => void;
}

/** What the drag needs of the pressed row; an `HTMLElement` in the app, a stand-in in tests. */
export type DragHandle = Pick<
  HTMLElement,
  | "setPointerCapture"
  | "hasPointerCapture"
  | "releasePointerCapture"
  | "addEventListener"
  | "removeEventListener"
>;

/** The pointer down that may start a drag (a React pointer event in the app). */
export interface DragPress {
  button: number;
  isPrimary: boolean;
  pointerId: number;
  clientX: number;
  clientY: number;
  currentTarget: DragHandle;
}

interface Gesture {
  id: number;
  element: DragHandle;
  pointerId: number;
  startX: number;
  startY: number;
  x: number;
  y: number;
  started: boolean;
  target: DragTarget | null;
  overBody: boolean;
  frame: number;
  stop: (commit: boolean) => void;
}

/** The list that holds the dragged session's row; the first list if the row is not found. */
function draggedList(id: number): HTMLElement | null {
  const row = document.querySelector(`[data-session-list] [data-session-row="${String(id)}"]`);
  return (
    row?.closest<HTMLElement>(SELECTOR.list) ?? document.querySelector<HTMLElement>(SELECTOR.list)
  );
}

function inside(rect: DOMRect, x: number, y: number): boolean {
  return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
}

/** One drag at a time, from pointer down to drop or cancel. */
export class SessionDrag {
  private gesture: Gesture | null = null;
  private ghost: HTMLElement | null = null;
  /** The newest order, layout and callbacks; read when the pointer moves and on the drop. */
  private options: SessionDragOptions;
  private readonly onView: (view: SessionDragView | null) => void;

  constructor(options: SessionDragOptions, onView: (view: SessionDragView | null) => void) {
    this.options = options;
    this.onView = onView;
  }

  setOptions(options: SessionDragOptions) {
    this.options = options;
  }

  /** Callback ref for the ghost element: placed at once, before its first paint. */
  readonly attachGhost = (element: HTMLElement | null) => {
    this.ghost = element;
    this.placeGhost();
  };

  /** Pointer down on a session row; becomes a drag once it moves past the threshold. */
  readonly begin = (id: number, event: DragPress) => {
    if (event.button !== 0 || !event.isPrimary || this.gesture) return;
    const element = event.currentTarget;
    const pointerId = event.pointerId;
    try {
      element.setPointerCapture(pointerId);
    } catch {
      return;
    }
    const onMove = (move: PointerEvent) => {
      if (move.pointerId === pointerId) this.move(move.clientX, move.clientY);
    };
    const onUp = (up: PointerEvent) => {
      if (up.pointerId === pointerId) stop(true);
    };
    const onCancel = () => {
      stop(false);
    };
    const onKey = (key: KeyboardEvent) => {
      if (key.key !== "Escape") return;
      key.preventDefault();
      key.stopPropagation();
      stop(false);
    };
    // After a real drag the button still gets its click (press and release on the captured
    // row); it must not activate the session on top of the drop.
    const swallowClick = (click: MouseEvent) => {
      // Enter or Space on a button also fires a click (detail 0); that one is the user's.
      if (click.detail === 0) return;
      click.stopPropagation();
      click.preventDefault();
    };
    const stop = (commit: boolean) => {
      const current = this.gesture;
      if (current?.element !== element) return;
      this.gesture = null;
      cancelAnimationFrame(current.frame);
      element.removeEventListener("pointermove", onMove);
      element.removeEventListener("pointerup", onUp);
      element.removeEventListener("pointercancel", onCancel);
      element.removeEventListener("lostpointercapture", onCancel);
      window.removeEventListener("keydown", onKey, CAPTURE);
      window.removeEventListener("blur", onCancel);
      try {
        if (element.hasPointerCapture(pointerId)) element.releasePointerCapture(pointerId);
      } catch {
        // The row may already be gone (session closed during the drag).
      }
      if (!current.started) return;
      document.body.classList.remove("session-dragging");
      window.addEventListener("click", swallowClick, CAPTURE);
      const unswallow = () => {
        window.removeEventListener("click", swallowClick, CAPTURE);
      };
      if (commit) {
        window.setTimeout(unswallow, 0);
      } else {
        // Escape or blur cancel while the button is still down: the release that follows must
        // not click the row either. The next release (or a new press, if the release happened
        // outside the window) ends the guard.
        const onRelease = () => {
          window.removeEventListener("pointerup", onRelease, CAPTURE);
          window.removeEventListener("pointerdown", onPress, CAPTURE);
          window.setTimeout(unswallow, 0);
        };
        const onPress = () => {
          window.removeEventListener("pointerup", onRelease, CAPTURE);
          window.removeEventListener("pointerdown", onPress, CAPTURE);
          unswallow();
        };
        window.addEventListener("pointerup", onRelease, CAPTURE);
        window.addEventListener("pointerdown", onPress, CAPTURE);
      }
      this.onView(null);
      if (commit && current.target !== null) this.drop(current.id, current.target);
    };
    this.gesture = {
      id,
      element,
      pointerId,
      startX: event.clientX,
      startY: event.clientY,
      x: event.clientX,
      y: event.clientY,
      started: false,
      target: null,
      overBody: false,
      frame: 0,
      stop,
    };
    element.addEventListener("pointermove", onMove);
    element.addEventListener("pointerup", onUp);
    element.addEventListener("pointercancel", onCancel);
    element.addEventListener("lostpointercapture", onCancel);
    window.addEventListener("keydown", onKey, CAPTURE);
    window.addEventListener("blur", onCancel);
  };

  /** Ends a running drag without dropping (unmount). */
  cancel() {
    this.gesture?.stop(false);
  }

  private move(x: number, y: number) {
    const current = this.gesture;
    if (!current) return;
    current.x = x;
    current.y = y;
    if (!current.started) {
      if (!beyondThreshold(x - current.startX, y - current.startY)) return;
      current.started = true;
      document.body.classList.add("session-dragging");
      this.onView({ id: current.id, target: null, overBody: false });
      current.frame = requestAnimationFrame(this.autoscroll);
    }
    this.placeGhost();
    this.update();
  }

  private placeGhost() {
    const current = this.gesture;
    if (!current || !this.ghost) return;
    const x = String(current.x + GHOST_OFFSET);
    const y = String(current.y + GHOST_OFFSET);
    this.ghost.style.transform = `translate(${x}px, ${y}px)`;
  }

  private update() {
    const current = this.gesture;
    if (!current?.started) return;
    const next = this.locate(current);
    if (sameDragTarget(next.target, current.target) && next.overBody === current.overBody) return;
    current.target = next.target;
    current.overBody = next.overBody;
    this.onView({ id: current.id, target: next.target, overBody: next.overBody });
  }

  /** Near the top or bottom edge of the list, the list scrolls; the target follows. */
  private readonly autoscroll = () => {
    const current = this.gesture;
    if (!current?.started) return;
    const list = draggedList(current.id);
    if (list && !current.overBody) {
      const rect = list.getBoundingClientRect();
      if (current.x >= rect.left && current.x <= rect.right) {
        const step = autoscrollStep(current.y, rect.top, rect.bottom);
        const before = list.scrollTop;
        if (step !== 0) list.scrollTop = before + step;
        if (list.scrollTop !== before) this.update();
      }
    }
    current.frame = requestAnimationFrame(this.autoscroll);
  };

  /** Drop target under the pointer, read from the DOM. */
  private locate(current: Gesture): { target: DragTarget | null; overBody: boolean } {
    const { x, y } = current;
    const list = draggedList(current.id);
    if (list) {
      const rect = list.getBoundingClientRect();
      // Above or below the list, but in its column, counts as its start or end of its group.
      if (x >= rect.left && x <= rect.right) {
        const rows = Array.from(document.querySelectorAll<HTMLElement>(SELECTOR.row)).map((row) => {
          const box = row.getBoundingClientRect();
          return {
            id: Number(row.dataset.sessionRow),
            mid: box.top + box.height / 2,
            group: row.dataset.sessionGroup ?? "",
          };
        });
        return {
          target: { kind: "list", index: groupDropIndex(rows, current.id, y, this.options.order) },
          overBody: false,
        };
      }
    }
    const panes = document.querySelector<HTMLElement>(SELECTOR.panes);
    if (!panes) return { target: null, overBody: false };
    const area = panes.getBoundingClientRect();
    if (!inside(area, x, y) || area.width <= 0 || area.height <= 0) {
      return { target: null, overBody: false };
    }
    const hit = document.elementFromPoint(x, y);
    const thumb = hit?.closest<HTMLElement>(SELECTOR.snapLayout);
    const layout = thumb?.dataset.snapLayout;
    if (thumb && isLayoutId(layout)) {
      const box = thumb.getBoundingClientRect();
      const zone = zoneAt(
        layout,
        (x - box.left) / box.width,
        (y - box.top) / box.height,
        ratiosFor(this.options.ratios ?? {}, layout),
      );
      return { target: zone === null ? null : { kind: "snap", layout, zone }, overBody: true };
    }
    // Between the thumbnails: on the bar, but no target.
    if (hit?.closest(SELECTOR.snapBar)) return { target: null, overBody: true };
    const zone = zoneAt(
      this.options.layout,
      (x - area.left) / area.width,
      (y - area.top) / area.height,
      ratiosFor(this.options.ratios ?? {}, this.options.layout),
    );
    return { target: zone === null ? null : { kind: "pane", zone }, overBody: true };
  }

  private drop(id: number, target: DragTarget) {
    const { order, layout, onMove, onPlace } = this.options;
    if (!order.includes(id)) return;
    if (target.kind === "list") {
      if (order.indexOf(id) !== target.index) onMove(id, target.index);
      return;
    }
    const placement = dropPlacement(target, layout);
    if (placement) onPlace(id, placement.layout, placement.zone);
  }
}

/** The drag for App: `drag` is what to show (drop line, snap bar, preview, ghost). */
export function useSessionDrag(options: SessionDragOptions) {
  const [view, setView] = useState<SessionDragView | null>(null);
  const [controller] = useState(() => new SessionDrag(options, setView));
  useEffect(() => {
    controller.setOptions(options);
  });
  useEffect(
    () => () => {
      controller.cancel();
    },
    [controller],
  );
  return { drag: view, begin: controller.begin, attachGhost: controller.attachGhost };
}
