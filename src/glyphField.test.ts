import { describe, expect, it } from "vitest";
import {
  CELL,
  CHANGE_RATE,
  createField,
  GLYPHS,
  glowCentres,
  intensityAt,
  liveRipples,
  mutate,
  RIPPLE_LIFE,
} from "./glyphField";

/** A repeatable sequence in [0, 1). */
function seeded(seed: number): () => number {
  let state = seed;
  return () => {
    state = (state * 1664525 + 1013904223) % 4294967296;
    return state / 4294967296;
  };
}

describe("glyph field", () => {
  it("covers the area with whole cells and valid glyphs", () => {
    const field = createField(100, 40, seeded(1));
    expect(field.cols).toBe(Math.ceil(100 / CELL));
    expect(field.rows).toBe(Math.ceil(40 / CELL));
    expect(field.glyphs.length).toBe(field.cols * field.rows);
    expect([...field.glyphs].every((g) => g < GLYPHS.length)).toBe(true);
  });

  it("stays empty without an area", () => {
    expect(createField(0, 0, seeded(1)).glyphs.length).toBe(0);
  });

  it("picks a valid glyph even when random returns its upper edge", () => {
    const field = createField(CELL, CELL, () => 0.9999999999);
    expect(field.glyphs[0]).toBe(GLYPHS.length - 1);
  });

  it("changes only a small share of glyphs per second", () => {
    const field = createField(1800, 1800, seeded(2));
    const before = [...field.glyphs];
    mutate(field, 1, seeded(3));
    const changed = field.glyphs.filter((g, i) => g !== before[i]).length;
    expect(changed).toBeLessThanOrEqual(Math.ceil(before.length * CHANGE_RATE));
    expect(changed).toBeGreaterThan(0);
  });

  it("keeps the glows inside the area", () => {
    for (let t = 0; t < 600; t += 7) {
      for (const glow of glowCentres(t, 800, 500)) {
        expect(glow.x).toBeGreaterThanOrEqual(0);
        expect(glow.x).toBeLessThanOrEqual(800);
        expect(glow.y).toBeGreaterThanOrEqual(0);
        expect(glow.y).toBeLessThanOrEqual(500);
      }
    }
  });

  it("lights a cell under the pointer and leaves far cells dark", () => {
    const pointer = { x: 50, y: 50 };
    expect(intensityAt({ x: 50, y: 50 }, [], pointer, [], 0)).toBeGreaterThan(0.5);
    expect(intensityAt({ x: 900, y: 900 }, [], pointer, [], 0)).toBeLessThan(0.001);
  });

  it("caps the light at 1", () => {
    const here = { x: 0, y: 0 };
    expect(intensityAt(here, [here, here, here], here, [{ ...here, start: 0 }], 0)).toBe(1);
  });

  it("runs a ring outwards and drops it after its life", () => {
    const ripple = { x: 0, y: 0, start: 0 };
    const near = intensityAt({ x: 0, y: 0 }, [], null, [ripple], 0.5);
    const onRing = intensityAt({ x: 130, y: 0 }, [], null, [ripple], 0.5);
    expect(onRing).toBeGreaterThan(near);
    expect(liveRipples([ripple], RIPPLE_LIFE + 0.1)).toEqual([]);
    expect(liveRipples([ripple], RIPPLE_LIFE - 0.1)).toEqual([ripple]);
  });
});
