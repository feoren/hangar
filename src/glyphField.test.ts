import { describe, expect, it } from "vitest";
import {
  CELL,
  CHANGE_RATE,
  cellShift,
  createField,
  driftOffset,
  GLYPHS,
  glyphAt,
  livePulses,
  liveTraces,
  mutate,
  PULSE_PEAK,
  pulseAt,
  spawnPulse,
  REVEAL_LIFE,
  REVEAL_RADIUS,
  revealAt,
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
  it("covers the area with a spare row and column and valid glyphs", () => {
    const field = createField(100, 40, seeded(1));
    expect(field.cols).toBe(Math.ceil(100 / CELL) + 1);
    expect(field.rows).toBe(Math.ceil(40 / CELL) + 1);
    expect(field.glyphs.length).toBe(field.cols * field.rows);
    expect([...field.glyphs].every((g) => g < GLYPHS.length)).toBe(true);
  });

  it("stays empty without an area", () => {
    const field = createField(0, 0, seeded(1));
    expect(field.glyphs.length).toBe(0);
    expect(glyphAt(field, 3, 3, { x: 0, y: 0 })).toBe(0);
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
});

describe("drift", () => {
  it("moves steadily along its direction", () => {
    const right = driftOffset(100, 0);
    expect(right.x).toBeGreaterThan(500);
    const down = driftOffset(100, Math.PI / 2);
    expect(down.y).toBeGreaterThan(500);
  });

  it("carries a glyph one cell on when the field drifts one cell", () => {
    const field = createField(200, 200, seeded(4));
    const before = glyphAt(field, 2, 3, { x: 0, y: 0 });
    expect(glyphAt(field, 3, 3, { x: CELL, y: 0 })).toBe(before);
    expect(glyphAt(field, 2, 4, { x: 0, y: CELL })).toBe(before);
  });

  it("wraps around in every direction without running out", () => {
    const field = createField(90, 90, seeded(5));
    const far = { x: -12345.6, y: 98765.4 };
    for (let col = -2; col < field.cols + 2; col++) {
      expect(glyphAt(field, col, 1, far)).toBeLessThan(GLYPHS.length);
    }
    const shift = cellShift(far);
    expect(shift.x).toBeGreaterThanOrEqual(0);
    expect(shift.x).toBeLessThan(CELL);
    expect(shift.y).toBeGreaterThanOrEqual(0);
    expect(shift.y).toBeLessThan(CELL);
  });
});

describe("reveal", () => {
  const trace = { x: 100, y: 100, time: 0 };

  it("keeps everything hidden without the pointer", () => {
    expect(revealAt({ x: 100, y: 100 }, [], 0)).toBe(0);
  });

  it("uncovers a small spot under the pointer, soft at its edge", () => {
    const centre = revealAt({ x: 100, y: 100 }, [trace], 0);
    const edge = revealAt({ x: 100 + REVEAL_RADIUS * 0.8, y: 100 }, [trace], 0);
    const outside = revealAt({ x: 100 + REVEAL_RADIUS * 2, y: 100 }, [trace], 0);
    expect(centre).toBe(1);
    expect(edge).toBeGreaterThan(0);
    expect(edge).toBeLessThan(centre);
    expect(outside).toBe(0);
  });

  it("fades back to nothing over its life", () => {
    const at = { x: 100, y: 100 };
    const early = revealAt(at, [trace], REVEAL_LIFE * 0.2);
    const late = revealAt(at, [trace], REVEAL_LIFE * 0.8);
    expect(late).toBeLessThan(early);
    expect(revealAt(at, [trace], REVEAL_LIFE + 0.1)).toBe(0);
    expect(liveTraces([trace], REVEAL_LIFE + 0.1)).toEqual([]);
    expect(liveTraces([trace], REVEAL_LIFE - 0.1)).toEqual([trace]);
  });

  it("is no brighter where traces overlap", () => {
    const many = [trace, { ...trace }, { ...trace, x: 105 }];
    expect(revealAt({ x: 100, y: 100 }, many, 0)).toBe(1);
  });
});

describe("pulses", () => {
  const pulse = {
    x: 200,
    y: 200,
    rx: 80,
    ry: 40,
    turn: 0,
    lobes: 3,
    phase: 0,
    start: 0,
    life: 4,
  };

  it("swells up and sinks back to nothing, never above its peak", () => {
    const centre = { x: 200, y: 200 };
    expect(pulseAt(centre, [pulse], 0)).toBe(0);
    expect(pulseAt(centre, [pulse], 2)).toBeCloseTo(PULSE_PEAK);
    expect(pulseAt(centre, [pulse], 1)).toBeLessThan(pulseAt(centre, [pulse], 2));
    expect(pulseAt(centre, [pulse], 4.1)).toBe(0);
    expect(livePulses([pulse], 4.1)).toEqual([]);
  });

  it("is wider one way than the other", () => {
    const along = pulseAt({ x: 260, y: 200 }, [pulse], 2);
    const across = pulseAt({ x: 200, y: 260 }, [pulse], 2);
    expect(along).toBeGreaterThan(across);
    expect(pulseAt({ x: 600, y: 600 }, [pulse], 2)).toBe(0);
  });

  it("appears inside the area with its own size and shape", () => {
    const random = seeded(7);
    const spawned = Array.from({ length: 20 }, () => spawnPulse(800, 500, 10, random));
    for (const p of spawned) {
      expect(p.x).toBeGreaterThanOrEqual(0);
      expect(p.x).toBeLessThanOrEqual(800);
      expect(p.y).toBeGreaterThanOrEqual(0);
      expect(p.y).toBeLessThanOrEqual(500);
      expect(p.ry).toBeLessThanOrEqual(p.rx);
      expect(p.start).toBe(10);
    }
    expect(new Set(spawned.map((p) => Math.round(p.rx))).size).toBeGreaterThan(10);
  });
});
