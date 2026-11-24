/**
 * The quiet glyph field behind the home view: a grid of random characters that stay nearly
 * invisible and only light up faintly where slow glows drift by, where the pointer moves and
 * where a click sends out a ring. Pure model, no DOM: `GlyphBackground` draws it on a canvas.
 */

/** Characters of the field: code punctuation, so the field reads as texture, not as words. */
export const GLYPHS = "01<>/\\{}[]()=+-*#:;.,~^$%&|!?";

/** Edge length of one grid cell in CSS pixels. */
export const CELL = 18;

/** Glyphs that change per second, as a share of the whole field. */
export const CHANGE_RATE = 0.02;

/** Seconds a click ring lives, and how fast and how wide it runs (pixels per second, pixels). */
export const RIPPLE_LIFE = 1.8;
const RIPPLE_SPEED = 260;
const RIPPLE_WIDTH = 34;

/** Radius of a drifting glow and of the pointer's light, in pixels. */
const GLOW_RADIUS = 260;
const POINTER_RADIUS = 110;

export interface Point {
  x: number;
  y: number;
}

export interface Ripple extends Point {
  /** Time of the click in seconds, on the same clock as `t`. */
  start: number;
}

export interface Field {
  cols: number;
  rows: number;
  /** One index into `GLYPHS` per cell, row by row. */
  glyphs: Uint8Array;
}

/** A field that covers `width` x `height` pixels, every cell with a random glyph. */
export function createField(width: number, height: number, random: () => number): Field {
  const cols = Math.max(0, Math.ceil(width / CELL));
  const rows = Math.max(0, Math.ceil(height / CELL));
  const glyphs = new Uint8Array(cols * rows);
  for (let i = 0; i < glyphs.length; i++) glyphs[i] = pick(random);
  return { cols, rows, glyphs };
}

/**
 * Swaps the glyphs of a few random cells for `seconds` of time. Fractions of a change carry
 * over by chance, so a slow frame rate changes as many glyphs per second as a fast one.
 */
export function mutate(field: Field, seconds: number, random: () => number): void {
  const exact = field.glyphs.length * CHANGE_RATE * Math.max(0, seconds);
  const count = Math.floor(exact) + (random() < exact % 1 ? 1 : 0);
  for (let n = 0; n < count; n++) {
    field.glyphs[Math.floor(random() * field.glyphs.length)] = pick(random);
  }
}

/**
 * The centres of the three glows at time `t`: each one wanders on its own slow Lissajous path
 * across the area, so they never line up and never stop.
 */
export function glowCentres(t: number, width: number, height: number): Point[] {
  const paths = [
    { fx: 0.031, fy: 0.023, px: 0, py: 1.3 },
    { fx: 0.019, fy: 0.037, px: 2.1, py: 0.4 },
    { fx: 0.027, fy: 0.015, px: 4.2, py: 3.1 },
  ];
  return paths.map((p) => ({
    x: width * (0.5 + 0.42 * Math.sin(t * p.fx * Math.PI * 2 + p.px)),
    y: height * (0.5 + 0.42 * Math.sin(t * p.fy * Math.PI * 2 + p.py)),
  }));
}

/** Soft falloff: 1 at the centre, about 0 at `radius`. */
function falloff(distance: number, radius: number): number {
  const d = distance / radius;
  return Math.exp(-4 * d * d);
}

/**
 * How brightly the cell at `cell` lights up, from 0 to 1: the glows, the pointer (when it is
 * over the area) and every living ring add up, capped at 1.
 */
export function intensityAt(
  cell: Point,
  glows: readonly Point[],
  pointer: Point | null,
  ripples: readonly Ripple[],
  t: number,
): number {
  let light = 0;
  for (const glow of glows) {
    light += 0.55 * falloff(Math.hypot(cell.x - glow.x, cell.y - glow.y), GLOW_RADIUS);
  }
  if (pointer) {
    light += 0.8 * falloff(Math.hypot(cell.x - pointer.x, cell.y - pointer.y), POINTER_RADIUS);
  }
  for (const ripple of ripples) {
    const age = t - ripple.start;
    if (age < 0 || age > RIPPLE_LIFE) continue;
    const ring = Math.abs(Math.hypot(cell.x - ripple.x, cell.y - ripple.y) - age * RIPPLE_SPEED);
    light += (1 - age / RIPPLE_LIFE) * falloff(ring, RIPPLE_WIDTH);
  }
  return Math.min(1, light);
}

/** The rings that still show at time `t`. */
export function liveRipples(ripples: readonly Ripple[], t: number): Ripple[] {
  return ripples.filter((ripple) => t - ripple.start <= RIPPLE_LIFE);
}

function pick(random: () => number): number {
  return Math.min(GLYPHS.length - 1, Math.floor(random() * GLYPHS.length));
}
