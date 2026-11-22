import { describe, expect, it } from "vitest";

import {
  DEFAULT_FIGURE_MOTION,
  figureSource,
  figuresMayMove,
  parseFigureMotion,
  stillReason,
  type MotionConditions,
} from "./figureMotion";

const free: MotionConditions = {
  choice: "animated",
  reducedMotion: false,
  powerSaving: false,
  windowVisible: true,
};

describe("figure motion", () => {
  it("is animated by default and reads only known values", () => {
    expect(DEFAULT_FIGURE_MOTION).toBe("animated");
    expect(parseFigureMotion("still")).toBe("still");
    expect(parseFigureMotion("animated")).toBe("animated");
    expect(parseFigureMotion("dance")).toBe("animated");
    expect(parseFigureMotion(undefined)).toBe("animated");
    expect(parseFigureMotion(false)).toBe("animated");
  });

  it("moves only when chosen, without reduced motion or power saving and in a visible window", () => {
    expect(figuresMayMove(free)).toBe(true);
    expect(figuresMayMove({ ...free, choice: "still" })).toBe(false);
    expect(figuresMayMove({ ...free, reducedMotion: true })).toBe(false);
    expect(figuresMayMove({ ...free, powerSaving: true })).toBe(false);
    expect(figuresMayMove({ ...free, windowVisible: false })).toBe(false);
  });

  it("names why figures stand still only while animated is chosen", () => {
    expect(stillReason(free)).toBeNull();
    expect(stillReason({ ...free, reducedMotion: true })).toBe("reducedMotion");
    expect(stillReason({ ...free, powerSaving: true })).toBe("powerSaving");
    // The system's own wish comes first.
    expect(stillReason({ ...free, reducedMotion: true, powerSaving: true })).toBe("reducedMotion");
    // Chosen still: nothing to explain.
    expect(stillReason({ ...free, choice: "still", reducedMotion: true })).toBeNull();
  });

  it("shows the animated file only when it exists, figures may move and the figure is on screen", () => {
    expect(figureSource("still.webp", "moving.webp", true, true)).toBe("moving.webp");
    expect(figureSource("still.webp", "moving.webp", true, false)).toBe("still.webp");
    expect(figureSource("still.webp", "moving.webp", false, true)).toBe("still.webp");
    // No animated file for this figure: the still one, whatever else holds.
    expect(figureSource("still.webp", undefined, true, true)).toBe("still.webp");
  });
});
