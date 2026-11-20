// How far a slider is filled. Chromium fills the track up to the thumb by itself; WebKit cannot,
// so webkit.css paints the fill from this ratio. Chromium ignores the property.

import type { CSSProperties } from "react";

declare module "react" {
  interface CSSProperties {
    /** 0 to 1: position of a slider's thumb between its minimum and maximum (webkit.css). */
    "--range-ratio"?: number;
  }
}

/** Inline style for an `<input type="range">` with the given value and bounds. */
export function rangeFillStyle(value: number, min: number, max: number): CSSProperties {
  const span = max - min;
  const ratio = span > 0 && Number.isFinite(value) ? (value - min) / span : 0;
  return { "--range-ratio": Math.min(1, Math.max(0, ratio)) };
}
