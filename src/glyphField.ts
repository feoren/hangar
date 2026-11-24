/**
 * The hidden glyph field behind the home view: a grid of random characters that is fully faded
 * out and drifts slowly in a direction picked at random. The pointer uncovers it, like wiping fog
 * off a window: a small area lights up where it passes and fades back to nothing after a moment.
 * Now and then a faint blot of random size and shape pulses up on its own somewhere. Pure model,
 * no DOM: `GlyphBackground` draws it on a canvas.
 */

/** Characters of the field: code punctuation, so the field reads as texture, not as words. */
export const GLYPHS = "01<>/\\{}[]()=+-*#:;.,~^$%&|!?";

/** Edge length of one grid cell in CSS pixels. */
export const CELL = 18;

/** Glyphs that change per second, as a share of the whole field. */
export const CHANGE_RATE = 0.02;

/** Seconds an uncovered spot takes to fade back to nothing. */
export const REVEAL_LIFE = 2.4;

/** Radius of the uncovered spot around the pointer, in pixels. */
export const REVEAL_RADIUS = 90;

/** Drift speed in pixels per second, and how far the path sways off its straight line. */
const DRIFT_SPEED = 9;
const DRIFT_SWAY = 40;

export interface Point {
  x: number;
  y: number;
}

/** One place the pointer passed, at `time` seconds. */
export interface Trace extends Point {
  time: number;
}

export interface Field {
  cols: number;
  rows: number;
  /** One index into `GLYPHS` per cell, row by row. */
  glyphs: Uint8Array;
}

/**
 * A field that covers `width` x `height` pixels with one spare row and column, so the drift can
 * wrap it around without a gap; every cell holds a random glyph.
 */
export function createField(width: number, height: number, random: () => number): Field {
  const cols = width > 0 ? Math.ceil(width / CELL) + 1 : 0;
  const rows = height > 0 ? Math.ceil(height / CELL) + 1 : 0;
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
 * How far the field has drifted at time `t`: steadily along `angle` (radians, picked at random
 * once), swaying gently to both sides so the path never looks mechanical. A closed form, so the
 * drift stays right even while nothing is drawn.
 */
export function driftOffset(t: number, angle: number): Point {
  const along = DRIFT_SPEED * t;
  const sway = DRIFT_SWAY * Math.sin(t * 0.07) + 0.5 * DRIFT_SWAY * Math.sin(t * 0.031 + 1.7);
  return {
    x: along * Math.cos(angle) - sway * Math.sin(angle),
    y: along * Math.sin(angle) + sway * Math.cos(angle),
  };
}

/**
 * The glyph that shows at screen cell (`col`, `row`) when the field has drifted by `offset`:
 * the field wraps around, so it never runs out. The cell itself is drawn moved by `cellShift`.
 */
export function glyphAt(field: Field, col: number, row: number, offset: Point): number {
  if (field.cols === 0 || field.rows === 0) return 0;
  const c = wrap(col - Math.floor(offset.x / CELL), field.cols);
  const r = wrap(row - Math.floor(offset.y / CELL), field.rows);
  return field.glyphs[r * field.cols + c] ?? 0;
}

/** The drift's part below one cell, where the grid is drawn from (0 up to `CELL`). */
export function cellShift(offset: Point): Point {
  return { x: wrap(offset.x, CELL), y: wrap(offset.y, CELL) };
}

/**
 * How far the spot at `at` is uncovered, from 0 (hidden) to 1: the strongest of the pointer's
 * traces, each one soft at its edge and fading out over its life. The strongest, not the sum,
 * so a slow pointer uncovers no brighter than a fast one.
 */
export function revealAt(at: Point, traces: readonly Trace[], t: number): number {
  let light = 0;
  for (const trace of traces) {
    const age = t - trace.time;
    if (age < 0 || age > REVEAL_LIFE) continue;
    const d = Math.hypot(at.x - trace.x, at.y - trace.y) / REVEAL_RADIUS;
    if (d >= 1.5) continue;
    const fade = 1 - age / REVEAL_LIFE;
    light = Math.max(light, Math.exp(-3 * d * d) * fade * fade);
  }
  return light;
}

/** The traces still uncovering something at time `t`. */
export function liveTraces(traces: readonly Trace[], t: number): Trace[] {
  return traces.filter((trace) => t - trace.time <= REVEAL_LIFE);
}

/**
 * A spot where the field pulses on its own now and then: an irregular blot, wider one way than
 * the other and turned, that swells up softly and sinks back to nothing over its life.
 */
export interface Pulse extends Point {
  /** Half widths along its own axes, in pixels. */
  rx: number;
  ry: number;
  /** Turn of its axes, in radians. */
  turn: number;
  /** Bumps along its edge (2 to 4) and where they sit, so no two blots look alike. */
  lobes: number;
  phase: number;
  start: number;
  /** Seconds from appearing to gone. */
  life: number;
}

/** How bright a pulse gets at its height: well below the pointer's light, so it stays a hint. */
export const PULSE_PEAK = 0.6;

/** Seconds between two pulses, at least and at most. */
export const PULSE_GAP_MIN = 1.5;
export const PULSE_GAP_MAX = 5;

/** A new pulse somewhere in the `width` x `height` area, of a random size and shape. */
export function spawnPulse(width: number, height: number, t: number, random: () => number): Pulse {
  const size = 35 + random() * 105;
  const stretch = 0.55 + random() * 0.45;
  return {
    x: random() * width,
    y: random() * height,
    rx: size,
    ry: size * stretch,
    turn: random() * Math.PI,
    lobes: 2 + Math.floor(random() * 3),
    phase: random() * Math.PI * 2,
    start: t,
    life: 3 + random() * 3,
  };
}

/**
 * The strongest light of the pulses at `at` and time `t`: soft inside its irregular outline,
 * rising and falling smoothly over its life, never above `PULSE_PEAK`.
 */
export function pulseAt(at: Point, pulses: readonly Pulse[], t: number): number {
  let light = 0;
  for (const pulse of pulses) {
    const age = t - pulse.start;
    if (age < 0 || age > pulse.life) continue;
    const dx = at.x - pulse.x;
    const dy = at.y - pulse.y;
    const cos = Math.cos(pulse.turn);
    const sin = Math.sin(pulse.turn);
    const u = (dx * cos + dy * sin) / pulse.rx;
    const v = (dy * cos - dx * sin) / pulse.ry;
    const edge = 1 + 0.22 * Math.sin(pulse.lobes * Math.atan2(v, u) + pulse.phase);
    const d = Math.hypot(u, v) / edge;
    if (d >= 1.5) continue;
    const swell = Math.sin((Math.PI * age) / pulse.life) ** 2;
    light = Math.max(light, PULSE_PEAK * swell * Math.exp(-3 * d * d));
  }
  return light;
}

/** The pulses still showing at time `t`. */
export function livePulses(pulses: readonly Pulse[], t: number): Pulse[] {
  return pulses.filter((pulse) => t - pulse.start <= pulse.life);
}

function wrap(value: number, size: number): number {
  return ((value % size) + size) % size;
}

function pick(random: () => number): number {
  return Math.min(GLYPHS.length - 1, Math.floor(random() * GLYPHS.length));
}
