// While the sidebar folds in or out, the terminals do not follow every frame: each fit would
// resize the PTY and make the program redraw a dozen times. Their fits wait and run once when the
// motion settles. The gate is plain state outside React; the hook drives it from the app grid.

import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";

import { SAVING_CLASS } from "./power";

/**
 * Longest the sidebar's or the right panel's motion may take before the gate opens anyway (the
 * CSS token `--panel-motion` is 280 ms).
 */
export const PANEL_MOTION_LIMIT_MS = 500;

/** Holds fits back while the sidebar moves; each waiting fit runs once afterwards. */
export class MotionGate {
  private moving = false;
  private readonly waiting = new Set<() => void>();

  get isMoving(): boolean {
    return this.moving;
  }

  /** Runs `fit` now, or once the motion ends; the same function waits only once. */
  whenSettled(fit: () => void): void {
    if (this.moving) this.waiting.add(fit);
    else fit();
  }

  /** Motion starts or ends; at the end every waiting fit runs. */
  setMoving(moving: boolean): void {
    if (this.moving === moving) return;
    this.moving = moving;
    if (moving) return;
    const due = [...this.waiting];
    this.waiting.clear();
    for (const fit of due) fit();
  }

  /** A fit that no longer needs to run (its terminal pane went away). */
  forget(fit: () => void): void {
    this.waiting.delete(fit);
  }
}

export const sidebarMotion = new MotionGate();

/** Held while the right panel slides in or out (sidePanelMotion.ts), like the sidebar's gate. */
export const sidePanelMotion = new MotionGate();

/**
 * Held while a splitter is dragged or stepped by keyboard, between panes (PaneSplitters.tsx) as well
 * as the sidebar and side panel widths (Splitter.tsx): the layout follows the pointer, the
 * terminals fit once at the end, so the programs redraw once.
 */
export const paneResizing = new MotionGate();

/** After the last arrow key step on a splitter, the terminals fit after this pause (ms). */
export const SPLITTER_KEY_SETTLE_MS = 250;

/**
 * Held while the window is being resized: the terminals on screen follow at once, hidden ones
 * fit once the window rests (or when they are shown), instead of on every frame of the drag.
 */
export const windowResizing = new MotionGate();

/** The window counts as resting once this long passes without a resize event (ms). */
export const WINDOW_RESIZE_SETTLE_MS = 150;

/**
 * Holds `gate` from the first `resize` event of `target` until `settleMs` pass without another.
 * Returns the function that stops listening and opens the gate.
 */
export function followResizes(target: EventTarget, gate: MotionGate, settleMs: number): () => void {
  let timer: ReturnType<typeof setTimeout> | undefined = undefined;
  const settle = () => {
    timer = undefined;
    gate.setMoving(false);
  };
  const onResize = () => {
    gate.setMoving(true);
    if (timer !== undefined) clearTimeout(timer);
    timer = setTimeout(settle, settleMs);
  };
  target.addEventListener("resize", onResize);
  return () => {
    target.removeEventListener("resize", onResize);
    if (timer !== undefined) clearTimeout(timer);
    settle();
  };
}

/** Drives `windowResizing` from the window's resize events while the app is mounted. */
export function useWindowResizing(): void {
  useEffect(() => followResizes(window, windowResizing, WINDOW_RESIZE_SETTLE_MS), []);
}

/**
 * Whether panels may glide: not with reduced motion and not while saving power on battery (the
 * root's class, power.ts). Without motion they open and close at once.
 */
export function motionAllowed(): boolean {
  if (document.documentElement.classList.contains(SAVING_CLASS)) return false;
  return !window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

/**
 * Marks the app grid with `sidebar-moving` from the moment the sidebar folds in or out until its
 * column transition ends, and holds terminal fits back meanwhile. `--sidebar-open` carries the
 * expanded width, so the expanded content keeps its final layout while the column grows.
 */
export function useSidebarMotion(
  app: RefObject<HTMLElement | null>,
  collapsed: boolean,
  openWidth: number,
): void {
  const shown = useRef(collapsed);

  useLayoutEffect(() => {
    app.current?.style.setProperty("--sidebar-open", `${String(openWidth)}px`);
  }, [app, openWidth]);

  // Before paint, so the first frame of the motion already has the class.
  useLayoutEffect(() => {
    if (shown.current === collapsed) return;
    shown.current = collapsed;
    const element = app.current;
    if (!element || !motionAllowed()) return;
    element.classList.add("sidebar-moving");
    sidebarMotion.setMoving(true);
    const settle = () => {
      window.clearTimeout(timer);
      element.classList.remove("sidebar-moving");
      sidebarMotion.setMoving(false);
    };
    const onEnd = (event: TransitionEvent) => {
      if (event.target === element && event.propertyName === "grid-template-columns") settle();
    };
    // Fallback: no transition ran (the column kept its width) or its end event got lost.
    const timer = window.setTimeout(settle, PANEL_MOTION_LIMIT_MS);
    element.addEventListener("transitionend", onEnd);
    element.addEventListener("transitioncancel", onEnd);
    return () => {
      element.removeEventListener("transitionend", onEnd);
      element.removeEventListener("transitioncancel", onEnd);
      settle();
    };
  }, [app, collapsed]);

  useEffect(
    () => () => {
      sidebarMotion.setMoving(false);
    },
    [],
  );
}
