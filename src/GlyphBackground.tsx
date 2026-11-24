import { useEffect, useRef } from "react";
import {
  CELL,
  cellShift,
  createField,
  driftOffset,
  type Field,
  GLYPHS,
  glyphAt,
  liveTraces,
  mutate,
  REVEAL_RADIUS,
  revealAt,
  type Trace,
} from "./glyphField";

/** Frames per second while something is uncovered; the fade is slow, more would only cost. */
const FPS = 30;
/** Opacity of a fully uncovered glyph, per colour: a faint text tone under a faint accent. */
const TEXT_PEAK = 0.09;
const ACCENT_PEAK = 0.13;
/** A new trace only after the pointer moved this far, which keeps the list short. */
const TRACE_SPACING = 10;

/**
 * The hidden glyph field behind the home view (model in `glyphField.ts`). Nothing shows until
 * the pointer passes: it uncovers a small spot like wiping fog, which fades back to nothing. The
 * field drifts slowly underneath in a random direction. While nothing is uncovered no frame is
 * drawn at all. The canvas ignores the pointer itself; moves are read on its parent, so every
 * control in front keeps working. With reduced motion it stays empty.
 */
export function GlyphBackground() {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const host = canvas?.parentElement;
    const context = canvas?.getContext("2d");
    if (!canvas || !host || !context) return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;

    const angle = Math.random() * Math.PI * 2;
    let field: Field = createField(0, 0, Math.random);
    let width = 0;
    let height = 0;
    let traces: Trace[] = [];
    let style = { text: "", accent: "", font: "" };
    let styleRead = -Infinity;
    let last = 0;
    let frame = 0;
    const started = performance.now();
    const now = () => (performance.now() - started) / 1000;

    const resize = () => {
      const box = host.getBoundingClientRect();
      const ratio = window.devicePixelRatio || 1;
      width = box.width;
      height = box.height;
      canvas.width = Math.round(width * ratio);
      canvas.height = Math.round(height * ratio);
      context.setTransform(ratio, 0, 0, ratio, 0, 0);
      field = createField(width, height, Math.random);
    };

    // The theme can switch while the home view shows; reading the tokens once a second follows it.
    const readStyle = (t: number) => {
      if (t - styleRead < 1) return;
      styleRead = t;
      const css = getComputedStyle(canvas);
      style = {
        text: css.getPropertyValue("--text").trim() || "#d6dde6",
        accent: css.getPropertyValue("--accent").trim() || "#5aa9ff",
        // The canvas font does not resolve CSS variables, so the token's value goes in itself.
        font: `${String(CELL * 0.62)}px ${css.getPropertyValue("--mono").trim() || "monospace"}`,
      };
    };

    const draw = (t: number) => {
      readStyle(t);
      context.clearRect(0, 0, width, height);
      if (traces.length === 0) return;
      // Only the cells around the traces can light up; the rest of the field stays untouched.
      const reach = REVEAL_RADIUS * 1.5;
      const left = Math.min(...traces.map((trace) => trace.x)) - reach;
      const right = Math.max(...traces.map((trace) => trace.x)) + reach;
      const top = Math.min(...traces.map((trace) => trace.y)) - reach;
      const bottom = Math.max(...traces.map((trace) => trace.y)) + reach;
      const offset = driftOffset(t, angle);
      const shift = cellShift(offset);
      context.font = style.font;
      context.textAlign = "center";
      context.textBaseline = "middle";
      const firstCol = Math.max(-1, Math.floor((left - shift.x) / CELL));
      const lastCol = Math.min(field.cols, Math.ceil((right - shift.x) / CELL));
      const firstRow = Math.max(-1, Math.floor((top - shift.y) / CELL));
      const lastRow = Math.min(field.rows, Math.ceil((bottom - shift.y) / CELL));
      const lit: { x: number; y: number; glyph: string; light: number }[] = [];
      for (let row = firstRow; row <= lastRow; row++) {
        for (let col = firstCol; col <= lastCol; col++) {
          const x = col * CELL + shift.x + CELL / 2;
          const y = row * CELL + shift.y + CELL / 2;
          const light = revealAt({ x, y }, traces, t);
          if (light < 0.01) continue;
          lit.push({ x, y, glyph: GLYPHS[glyphAt(field, col, row, offset)] ?? "", light });
        }
      }
      for (const [colour, peak] of [
        [style.text, TEXT_PEAK],
        [style.accent, ACCENT_PEAK],
      ] as const) {
        context.fillStyle = colour;
        for (const cell of lit) {
          context.globalAlpha = cell.light * peak;
          context.fillText(cell.glyph, cell.x, cell.y);
        }
      }
      context.globalAlpha = 1;
    };

    // Runs only while something is uncovered; the last frame clears the canvas and stops.
    const tick = () => {
      const t = now();
      if (t - last >= 1 / FPS) {
        mutate(field, t - last, Math.random);
        last = t;
        traces = liveTraces(traces, t);
        draw(t);
        if (traces.length === 0) {
          frame = 0;
          return;
        }
      }
      frame = requestAnimationFrame(tick);
    };

    const onMove = (event: PointerEvent) => {
      const box = host.getBoundingClientRect();
      const x = event.clientX - box.left;
      const y = event.clientY - box.top;
      const newest = traces[traces.length - 1];
      if (newest && Math.hypot(x - newest.x, y - newest.y) < TRACE_SPACING) {
        newest.time = now();
      } else {
        traces.push({ x, y, time: now() });
      }
      if (frame === 0) {
        last = now();
        frame = requestAnimationFrame(tick);
      }
    };

    const observer = new ResizeObserver(resize);
    observer.observe(host);
    resize();
    host.addEventListener("pointermove", onMove);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
      host.removeEventListener("pointermove", onMove);
    };
  }, []);

  return <canvas ref={canvasRef} className="glyph-background" aria-hidden="true" />;
}
