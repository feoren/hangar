// How the three dots of a working session move (status chip in the sidebar and the rail). The
// choice is stored in the settings; styles.css draws each style from `data-dots` on the root, and a
// `dots-<style>` class overrides it locally (the previews in the settings). Pure.

/**
 * - `bounce`: each dot hops and lands softly, one after the other (default)
 * - `wave`: the dots rise and sink in a smooth, continuous wave
 * - `pulse`: the dots grow and shrink in turn
 * - `fade`: the dots light up in turn, without moving
 */
export const DOT_ANIMATIONS = ["bounce", "wave", "pulse", "fade"] as const;
export type DotAnimation = (typeof DOT_ANIMATIONS)[number];

export const DEFAULT_DOT_ANIMATION: DotAnimation = "bounce";

export function isDotAnimation(value: unknown): value is DotAnimation {
  return typeof value === "string" && DOT_ANIMATIONS.some((style) => style === value);
}

/** Stored value, or the default for anything unknown (also settings from before the choice). */
export function parseDotAnimation(value: unknown): DotAnimation {
  return isDotAnimation(value) ? value : DEFAULT_DOT_ANIMATION;
}

/** CSS class that shows one style regardless of the setting (styles.css, `.dots-*`). */
export function dotAnimationClass(style: DotAnimation): string {
  return `dots-${style}`;
}
