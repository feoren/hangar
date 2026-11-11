// While the sidebar folds in or out, the terminals do not follow every frame: each fit would
// resize the PTY and make the program redraw a dozen times. Their fits wait and run once when the
// motion settles. The gate is plain state outside React; the hook drives it from the app grid.

import { useEffect, useLayoutEffect, useRef, type RefObject } from "react";

/** Longest the motion may take before the gate opens anyway (the CSS uses 220 ms). */
export const SIDEBAR_MOTION_LIMIT_MS = 400;

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

function reducedMotion(): boolean {
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
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
    if (!element || reducedMotion()) return;
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
    const timer = window.setTimeout(settle, SIDEBAR_MOTION_LIMIT_MS);
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
