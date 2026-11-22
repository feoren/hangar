// The page side of figureMotion.ts: one store for the whole window that knows the setting, the
// system's reduced motion wish, power saving and whether the window is hidden, and one shared
// IntersectionObserver that tells each animated figure whether it is on screen. Listeners on the
// page are attached only while a component subscribes.

import { useEffect, useState, useSyncExternalStore, type RefObject } from "react";

import {
  DEFAULT_FIGURE_MOTION,
  figuresMayMove,
  stillReason,
  type FigureMotion,
  type MotionConditions,
  type StillReason,
} from "./figureMotion";
import { isPowerSaving, onPowerSavingChange } from "./usePower";

const REDUCED_QUERY = "(prefers-reduced-motion: reduce)";

let choice: FigureMotion = DEFAULT_FIGURE_MOTION;
const listeners = new Set<() => void>();
let detach: (() => void) | null = null;

function reducedMotion(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia(REDUCED_QUERY).matches
  );
}

function windowVisible(): boolean {
  return typeof document === "undefined" || document.visibilityState !== "hidden";
}

function conditions(): MotionConditions {
  return {
    choice,
    reducedMotion: reducedMotion(),
    powerSaving: isPowerSaving(),
    windowVisible: windowVisible(),
  };
}

function notify(): void {
  for (const listener of listeners) listener();
}

function attach(): () => void {
  const query =
    typeof window !== "undefined" && typeof window.matchMedia === "function"
      ? window.matchMedia(REDUCED_QUERY)
      : null;
  query?.addEventListener("change", notify);
  document.addEventListener("visibilitychange", notify);
  const stopPower = onPowerSavingChange(notify);
  return () => {
    query?.removeEventListener("change", notify);
    document.removeEventListener("visibilitychange", notify);
    stopPower();
  };
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  if (detach === null && typeof document !== "undefined") detach = attach();
  return () => {
    listeners.delete(listener);
    if (listeners.size === 0 && detach !== null) {
      detach();
      detach = null;
    }
  };
}

/** The setting "Figures: Animated / Still" (App.tsx keeps it equal to the settings). */
export function setFigureMotion(next: FigureMotion): void {
  if (next === choice) return;
  choice = next;
  notify();
}

/** Whether figures may move in this window right now; rendered on a server: never. */
export function useFiguresMayMove(): boolean {
  return useSyncExternalStore(
    subscribe,
    () => figuresMayMove(conditions()),
    () => false,
  );
}

/** Why the figures stand still although the setting says animated (the hint in Settings). */
export function useFigureStillReason(): StillReason | null {
  return useSyncExternalStore(
    subscribe,
    () => stillReason(conditions()),
    () => null,
  );
}

// One observer for every animated figure: a figure in a scrolled-away list, a collapsed group or
// a hidden pane does not intersect and so shows its still file.
const watched = new Map<Element, (onScreen: boolean) => void>();
let observer: IntersectionObserver | null = null;

function sharedObserver(): IntersectionObserver {
  observer ??= new IntersectionObserver((entries) => {
    for (const entry of entries) watched.get(entry.target)?.(entry.isIntersecting);
  });
  return observer;
}

/**
 * Whether the element is on screen, watched only while `enabled`. Without an
 * IntersectionObserver (old WebViews, tests) it counts as on screen.
 */
export function useOnScreen(ref: RefObject<Element | null>, enabled: boolean): boolean {
  const [onScreen, setOnScreen] = useState(false);
  const observable = typeof IntersectionObserver !== "undefined";
  useEffect(() => {
    const element = ref.current;
    if (!enabled || element === null || !observable) return;
    const io = sharedObserver();
    watched.set(element, setOnScreen);
    io.observe(element);
    return () => {
      watched.delete(element);
      io.unobserve(element);
      // Unwatched, the last answer goes stale: a figure scrolled away while figures stood still
      // must not count as on screen when they may move again, before the observer answers anew.
      setOnScreen(false);
    };
  }, [ref, enabled, observable]);
  return enabled && (!observable || onScreen);
}

/**
 * `wanted` once the browser has decoded it, the source shown so far until then: an image whose
 * `src` changes to a file that is not decoded yet may paint an empty frame in between. A file
 * that fails to decode is shown anyway, as the image would without this.
 */
export function useDecodedSource(wanted: string): string {
  const [shown, setShown] = useState(wanted);
  useEffect(() => {
    if (wanted === shown) return;
    let current = true;
    const swap = () => {
      if (current) setShown(wanted);
    };
    const probe = new Image();
    probe.src = wanted;
    probe.decode().then(swap, swap);
    return () => {
      current = false;
    };
  }, [wanted, shown]);
  return shown;
}
