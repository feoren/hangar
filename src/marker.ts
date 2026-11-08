// Session marker: an optional color and a short emoji per session, so sessions can be told apart
// at a glance (sidebar, rail, pane header). Saved in the workspace; the backend validates again
// (`#rrggbb`, emoji 1 to 16 bytes UTF-8 without control characters, never an empty string).

import type { MessageKey } from "./i18n";

export interface Marker {
  /** `#rrggbb`, lowercase. */
  color: string | null;
  emoji: string | null;
}

export const NO_MARKER: Marker = { color: null, emoji: null };

/**
 * Fixed palette: medium tones that stay visible on the dark and the light sidebar. A stored color
 * outside the palette (hand-edited workspace) is shown as it is.
 */
export const MARKER_COLORS = [
  { id: "red", color: "#e5534b" },
  { id: "orange", color: "#e0823d" },
  { id: "yellow", color: "#c9a227" },
  { id: "green", color: "#46a758" },
  { id: "teal", color: "#2fa6a0" },
  { id: "blue", color: "#4a8fe7" },
  { id: "purple", color: "#986ee2" },
  { id: "pink", color: "#d6609e" },
] as const;

export type MarkerColorId = (typeof MARKER_COLORS)[number]["id"];

export const MARKER_COLOR_NAMES: Record<MarkerColorId, MessageKey> = {
  red: "marker.red",
  orange: "marker.orange",
  yellow: "marker.yellow",
  green: "marker.green",
  teal: "marker.teal",
  blue: "marker.blue",
  purple: "marker.purple",
  pink: "marker.pink",
};

/** Quick picks; any other emoji can be typed or pasted. */
export const MARKER_EMOJIS = [
  "\u{1F525}",
  "\u{2B50}",
  "\u{1F680}",
  "\u{1F41B}",
  "\u{1F9EA}",
  "\u{1F4DD}",
  "\u{1F512}",
  "\u{2705}",
] as const;

export const MAX_EMOJI_BYTES = 16;

const encoder = new TextEncoder();

export function normalizeMarkerColor(value: unknown): string | null {
  return typeof value === "string" && /^#[0-9a-f]{6}$/i.test(value) ? value.toLowerCase() : null;
}

/**
 * Validated emoji: trimmed, 1 to 16 bytes UTF-8, no control characters. Empty input means no
 * emoji (`null`); anything else that does not fit is `undefined` (invalid, show a hint).
 */
export function parseEmoji(value: unknown): string | null | undefined {
  if (value === null || value === undefined) return null;
  if (typeof value !== "string") return undefined;
  const text = value.trim();
  if (text === "") return null;
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f-\u009f]/.test(text)) return undefined;
  if (encoder.encode(text).length > MAX_EMOJI_BYTES) return undefined;
  return text;
}

/** For stored data: invalid values become "no emoji" instead of an error. */
export function storedEmoji(value: unknown): string | null {
  return parseEmoji(value) ?? null;
}

export function parseMarker(value: unknown): Marker {
  if (typeof value !== "object" || value === null) return NO_MARKER;
  return {
    color: normalizeMarkerColor(Reflect.get(value, "color")),
    emoji: storedEmoji(Reflect.get(value, "emoji")),
  };
}

export function hasMarker(marker: Marker | undefined): marker is Marker {
  return marker !== undefined && (marker.color !== null || marker.emoji !== null);
}

/** Name of a palette color for screen readers and tooltips; `null` outside the palette. */
export function markerColorName(color: string | null): MessageKey | null {
  if (color === null) return null;
  const found = MARKER_COLORS.find((entry) => entry.color === color.toLowerCase());
  return found ? MARKER_COLOR_NAMES[found.id] : null;
}
