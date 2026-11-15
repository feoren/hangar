// The size of a dialog the user resizes by dragging its edges, like a window. Pure, so the
// limits, the drag arithmetic and the stored value are testable without a DOM.
//
// The overlay centers the dialog, so it grows to both sides: dragging the right edge by 10 px
// makes it 20 px wider and the left edge moves out by 10 px as well. The edge stays under the
// pointer. What the user chose is kept as the preferred size; the shown size is that, clamped
// to the window, so a window made smaller and then larger again gets the chosen size back.

/** Width and height in CSS pixels. */
export interface DialogSize {
  width: number;
  height: number;
}

/** An edge or corner a dialog is resized by, as compass points. */
export type ResizeEdge = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";

export const RESIZE_EDGES: readonly ResizeEdge[] = ["n", "s", "e", "w", "ne", "nw", "se", "sw"];

/** Free space kept between the dialog and each window edge, as the stylesheet keeps it. */
export const DIALOG_MARGIN = 16;

/** One arrow key press on the resize handle, in CSS pixels. */
export const DIALOG_KEY_STEP = 16;

/** Which way an edge moves the size: +1 grows it with the pointer moving right or down. */
export function edgeAxes(edge: ResizeEdge): { x: -1 | 0 | 1; y: -1 | 0 | 1 } {
  const x = edge.includes("e") ? 1 : edge.includes("w") ? -1 : 0;
  const y = edge.includes("s") ? 1 : edge.includes("n") ? -1 : 0;
  return { x, y };
}

/** The largest size that fits: the window less the margin on every side. */
export function maxDialogSize(viewport: DialogSize): DialogSize {
  return {
    width: Math.max(0, Math.floor(viewport.width - 2 * DIALOG_MARGIN)),
    height: Math.max(0, Math.floor(viewport.height - 2 * DIALOG_MARGIN)),
  };
}

/**
 * The size within `min` and the window, in whole pixels. In a window smaller than `min` the
 * window wins: the dialog then fills it, as it did before it could be resized.
 */
export function clampDialogSize(
  size: DialogSize,
  viewport: DialogSize,
  min: DialogSize,
): DialogSize {
  const max = maxDialogSize(viewport);
  const clamp = (value: number, low: number, high: number) =>
    Math.round(Math.min(Math.max(value, low), high));
  return {
    width: clamp(size.width, min.width, max.width),
    height: clamp(size.height, min.height, max.height),
  };
}

/**
 * The size while an edge is dragged: from the size at the press, the pointer's way since then
 * counts twice (the centered dialog grows to both sides). Computed from the start every time, so
 * after hitting a limit the edge meets the pointer again where it left it.
 */
export function dragDialogSize(
  start: DialogSize,
  edge: ResizeEdge,
  delta: { x: number; y: number },
  viewport: DialogSize,
  min: DialogSize,
): DialogSize {
  const axes = edgeAxes(edge);
  return clampDialogSize(
    {
      width: start.width + 2 * axes.x * delta.x,
      height: start.height + 2 * axes.y * delta.y,
    },
    viewport,
    min,
  );
}

/**
 * The size after an arrow key on the resize handle: Left and Right change the width, Up and Down
 * the height. Null for any other key.
 */
export function stepDialogSize(
  size: DialogSize,
  key: string,
  viewport: DialogSize,
  min: DialogSize,
): DialogSize | null {
  const steps: Record<string, DialogSize> = {
    ArrowRight: { width: DIALOG_KEY_STEP, height: 0 },
    ArrowLeft: { width: -DIALOG_KEY_STEP, height: 0 },
    ArrowDown: { width: 0, height: DIALOG_KEY_STEP },
    ArrowUp: { width: 0, height: -DIALOG_KEY_STEP },
  };
  const step = steps[key];
  if (step === undefined) return null;
  return clampDialogSize(
    { width: size.width + step.width, height: size.height + step.height },
    viewport,
    min,
  );
}

/**
 * The size adjusted so that the centered dialog starts on a whole device pixel. At 125 % or 150 %
 * scaling a centered box can otherwise land between device pixels and its text blurs. Changes the
 * size by less than two device pixels.
 */
export function snapDialogSize(
  size: DialogSize,
  viewport: DialogSize,
  devicePixelRatio: number,
): DialogSize {
  const ratio = Number.isFinite(devicePixelRatio) && devicePixelRatio > 0 ? devicePixelRatio : 1;
  const snap = (value: number, room: number) => {
    const offset = Math.round(((room - value) / 2) * ratio) / ratio;
    return Math.max(0, room - 2 * offset);
  };
  return { width: snap(size.width, viewport.width), height: snap(size.height, viewport.height) };
}

/** A stored size: an object with positive finite `width` and `height`, else null. */
export function parseDialogSize(value: unknown): DialogSize | null {
  if (typeof value !== "object" || value === null) return null;
  const width: unknown = Reflect.get(value, "width");
  const height: unknown = Reflect.get(value, "height");
  if (typeof width !== "number" || typeof height !== "number") return null;
  if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) {
    return null;
  }
  return { width, height };
}

/** Reads a stored size from its JSON text; null without one or for anything malformed. */
export function parseStoredDialogSize(text: string | null): DialogSize | null {
  if (text === null) return null;
  try {
    const value: unknown = JSON.parse(text);
    return parseDialogSize(value);
  } catch {
    return null;
  }
}

/** The size remembered on this computer, or null (none, or no storage in this window). */
export function loadDialogSize(key: string): DialogSize | null {
  try {
    return parseStoredDialogSize(localStorage.getItem(key));
  } catch {
    return null;
  }
}

/** Remembers a size; null forgets it, so the dialog opens at its standard size again. */
export function saveDialogSize(key: string, size: DialogSize | null): void {
  try {
    if (size === null) localStorage.removeItem(key);
    else localStorage.setItem(key, JSON.stringify(size));
  } catch {
    // Without storage the dialog opens at its standard size next time.
  }
}
