import { useEffect, useRef } from "react";
import {
  CELL,
  createField,
  type Field,
  GLYPHS,
  glowCentres,
  intensityAt,
  liveRipples,
  mutate,
  type Point,
  type Ripple,
} from "./glyphField";

/** Frames per second: the field moves slowly, more would only cost battery. */
const FPS = 24;
/** Opacity of an unlit glyph and the extra opacity at full light, per colour. */
const BASE_ALPHA = 0.035;
const TEXT_PEAK = 0.07;
const ACCENT_PEAK = 0.16;
/** Below this light a glyph gets no accent pass, which spares most of the draw calls. */
const ACCENT_THRESHOLD = 0.03;

/**
 * The glyph field behind the home view (model in `glyphField.ts`). It stays far in the
 * background: unlit glyphs are barely visible, the light is a faint accent tint. The canvas
 * ignores the pointer itself; moves and clicks are read on its parent, so every control in front
 * keeps working. With reduced motion it draws one still frame.
 */
export function GlyphBackground() {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const host = canvas?.parentElement;
    const context = canvas?.getContext("2d");
    if (!canvas || !host || !context) return;
    const still = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    let field: Field = createField(0, 0, Math.random);
    let width = 0;
    let height = 0;
    let pointer: Point | null = null;
    let ripples: Ripple[] = [];
    let colours = { text: "", accent: "", font: "" };
    let coloursRead = -Infinity;
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
      if (still) draw(0);
    };

    // The theme can switch while the home view shows; reading the tokens once a second follows it.
    const readColours = (t: number) => {
      if (t - coloursRead < 1) return;
      coloursRead = t;
      const style = getComputedStyle(canvas);
      colours = {
        text: style.getPropertyValue("--text").trim() || "#d6dde6",
        accent: style.getPropertyValue("--accent").trim() || "#5aa9ff",
        // The canvas font does not resolve CSS variables, so the token's value goes in itself.
        font: `${String(CELL * 0.62)}px ${style.getPropertyValue("--mono").trim() || "monospace"}`,
      };
    };

    const draw = (t: number) => {
      readColours(t);
      context.clearRect(0, 0, width, height);
      context.font = colours.font;
      context.textAlign = "center";
      context.textBaseline = "middle";
      const glows = glowCentres(t, width, height);
      ripples = liveRipples(ripples, t);
      const lit: { x: number; y: number; glyph: string; light: number }[] = [];
      context.fillStyle = colours.text;
      for (let row = 0; row < field.rows; row++) {
        for (let col = 0; col < field.cols; col++) {
          const x = col * CELL + CELL / 2;
          const y = row * CELL + CELL / 2;
          const glyph = GLYPHS[field.glyphs[row * field.cols + col] ?? 0] ?? "";
          const light = intensityAt({ x, y }, glows, pointer, ripples, t);
          context.globalAlpha = BASE_ALPHA + light * TEXT_PEAK;
          context.fillText(glyph, x, y);
          if (light > ACCENT_THRESHOLD) lit.push({ x, y, glyph, light });
        }
      }
      context.fillStyle = colours.accent;
      for (const cell of lit) {
        context.globalAlpha = cell.light * ACCENT_PEAK;
        context.fillText(cell.glyph, cell.x, cell.y);
      }
      context.globalAlpha = 1;
    };

    const tick = () => {
      frame = requestAnimationFrame(tick);
      const t = now();
      if (t - last < 1 / FPS) return;
      mutate(field, t - last, Math.random);
      last = t;
      draw(t);
    };

    const local = (event: PointerEvent): Point => {
      const box = host.getBoundingClientRect();
      return { x: event.clientX - box.left, y: event.clientY - box.top };
    };
    const onMove = (event: PointerEvent) => {
      pointer = local(event);
    };
    const onLeave = () => {
      pointer = null;
    };
    const onDown = (event: PointerEvent) => {
      ripples = [...ripples, { ...local(event), start: now() }];
    };

    const observer = new ResizeObserver(resize);
    observer.observe(host);
    resize();
    if (!still) {
      host.addEventListener("pointermove", onMove);
      host.addEventListener("pointerleave", onLeave);
      host.addEventListener("pointerdown", onDown);
      frame = requestAnimationFrame(tick);
    }
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
      host.removeEventListener("pointermove", onMove);
      host.removeEventListener("pointerleave", onLeave);
      host.removeEventListener("pointerdown", onDown);
    };
  }, []);

  return <canvas ref={canvasRef} className="glyph-background" aria-hidden="true" />;
}
