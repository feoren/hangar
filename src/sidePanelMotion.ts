// The right panel slides in and out like the sidebar folds: same duration and easing (styles.css,
// `.side-panel.moving`). Closing keeps it mounted until the motion ends. While it moves, the
// terminals hold their fits (sidePanelMotion in sidebarMotion.ts) and fit once at the end.

import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from "react";

import { motionAllowed, PANEL_MOTION_LIMIT_MS, sidePanelMotion } from "./sidebarMotion";

/**
 * Where the panel is: not there, sliding in, resting open, sliding out (still mounted, inert).
 * Without motion it only knows `closed` and `open`.
 */
export type PanelPhase = "closed" | "opening" | "open" | "closing";

/** The panel should show or hide, or its slide came to an end. */
export type PanelEvent = "show" | "hide" | "settled";

/** The phase after `event`; `animate` says whether showing or hiding may slide. */
export function nextPanelPhase(phase: PanelPhase, event: PanelEvent, animate: boolean): PanelPhase {
  switch (event) {
    case "show":
      if (phase === "open" || phase === "opening") return phase;
      return animate ? "opening" : "open";
    case "hide":
      if (phase === "closed" || phase === "closing") return phase;
      return animate ? "closing" : "closed";
    case "settled":
      if (phase === "opening") return "open";
      if (phase === "closing") return "closed";
      return phase;
  }
}

/** Whether the panel is mid slide in this phase. */
export function panelMoving(phase: PanelPhase): boolean {
  return phase === "opening" || phase === "closing";
}

/** The transitions whose end settles the slide: inline it grows, as an overlay it slides. */
const SLIDE_PROPERTIES: ReadonlySet<string> = new Set(["width", "transform"]);

/**
 * Follows `shown` with a phase: the panel renders while the phase is not `closed`, and the ref
 * goes on its frame so the end of its transition settles the slide. A slide whose end event gets
 * lost (no transition ran) settles after PANEL_MOTION_LIMIT_MS. With `animate` false (or no
 * motion allowed) the panel comes and goes at once.
 */
export function useSidePanelMotion(
  shown: boolean,
  animate = true,
): {
  phase: PanelPhase;
  panel: RefObject<HTMLElement | null>;
} {
  const panel = useRef<HTMLElement>(null);
  const [state, setState] = useState<{ shown: boolean; phase: PanelPhase }>(() => ({
    shown,
    phase: shown ? "open" : "closed",
  }));
  let phase: PanelPhase = state.phase;
  // Adjusted while rendering, so the panel mounts (or starts to leave) in the same commit.
  if (state.shown !== shown) {
    phase = nextPanelPhase(state.phase, shown ? "show" : "hide", animate && motionAllowed());
    setState({ shown, phase });
  }

  // Before paint, so the terminals already wait in the first frame of the slide.
  useLayoutEffect(() => {
    if (!panelMoving(phase)) {
      sidePanelMotion.setMoving(false);
      return;
    }
    sidePanelMotion.setMoving(true);
    const element = panel.current;
    const settle = () => {
      setState((current) => ({
        ...current,
        phase: nextPanelPhase(current.phase, "settled", true),
      }));
    };
    const onEnd = (event: TransitionEvent) => {
      if (event.target === element && SLIDE_PROPERTIES.has(event.propertyName)) settle();
    };
    const timer = window.setTimeout(settle, PANEL_MOTION_LIMIT_MS);
    element?.addEventListener("transitionend", onEnd);
    return () => {
      window.clearTimeout(timer);
      element?.removeEventListener("transitionend", onEnd);
    };
  }, [phase]);

  useEffect(
    () => () => {
      sidePanelMotion.setMoving(false);
    },
    [],
  );

  return { phase, panel };
}
