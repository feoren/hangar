// Whether the bundled figures play their calm idle loop (src/assets/sub-agents-animated) or stand
// still (src/assets/sub-agents). Pure: the setting, why a figure stands still although the user
// chose motion, and which file an image shows. useFigureMotion.ts wires it to the page.
// An animated WebP in an <img> cannot be paused, so standing still means showing the still file:
// both files hold the figure in the same place, so swapping them never moves it.

export const FIGURE_MOTIONS = ["animated", "still"] as const;
/** The setting "Figures: Animated / Still". */
export type FigureMotion = (typeof FIGURE_MOTIONS)[number];

export const DEFAULT_FIGURE_MOTION: FigureMotion = "animated";

export function parseFigureMotion(value: unknown): FigureMotion {
  return FIGURE_MOTIONS.find((motion) => motion === value) ?? DEFAULT_FIGURE_MOTION;
}

/** What keeps the figures still although the setting says animated. */
export type StillReason = "reducedMotion" | "powerSaving";

/** The conditions the whole window shares; `visible` per figure comes on top. */
export interface MotionConditions {
  choice: FigureMotion;
  /** The system asks for less motion (`prefers-reduced-motion: reduce`). */
  reducedMotion: boolean;
  /** Hangar saves energy on battery right now (power.ts `savingActive`). */
  powerSaving: boolean;
  /** The window is not hidden or minimized (`document.visibilityState`). */
  windowVisible: boolean;
}

/**
 * Why the figures stand still although the user chose motion, for the hint beside the setting;
 * `null` when they may move or the user chose still. Reduced motion is named first: it is the
 * user's own system wish and holds on AC too.
 */
export function stillReason(
  conditions: Omit<MotionConditions, "windowVisible">,
): StillReason | null {
  if (conditions.choice !== "animated") return null;
  if (conditions.reducedMotion) return "reducedMotion";
  if (conditions.powerSaving) return "powerSaving";
  return null;
}

/** Whether figures may move at all in this window right now. */
export function figuresMayMove(conditions: MotionConditions): boolean {
  return (
    conditions.choice === "animated" && conditions.windowVisible && stillReason(conditions) === null
  );
}

/**
 * The file one figure shows: the animated one only while figures may move, an animated file
 * exists and the figure is on screen; else the still one.
 */
export function figureSource(
  still: string,
  animated: string | undefined,
  mayMove: boolean,
  onScreen: boolean,
): string {
  return animated !== undefined && mayMove && onScreen ? animated : still;
}
