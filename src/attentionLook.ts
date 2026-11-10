// Colors derived from the user's attention color, so any choice (preset or custom) stays readable:
// the text color on the status chip, and how strongly a waiting row breathes. The breathing tint
// is mixed onto the sidebar color; its peak is as strong as the row's text still allows.

import { CREAM as DEFAULT_ATTENTION_COLOR, normalizeColor } from "./stateLook";
import { contrastRatio, type Theme } from "./theme";

const INK = "#1a1d21";
const WHITE = "#ffffff";

/** Sidebar background, text and muted text per theme; mirrors `--panel`, `--text`, `--muted`. */
const ROW_COLORS: Record<Theme, { panel: string; text: string; muted: string }> = {
  dark: { panel: "#151a21", text: "#d6dde6", muted: "#8592a3" },
  light: { panel: "#f3f5f8", text: "#1f2328", muted: "#59636e" },
};

/** Strongest and weakest peak of the breathing tint, in percent of the attention color. */
const PEAK: Record<Theme, { min: number; max: number }> = {
  dark: { min: 10, max: 30 },
  light: { min: 20, max: 72 },
};

/** The calm end of the breath, relative to its peak. */
const LOW_SHARE = 0.3;

/**
 * Text color that stays readable on this background: dark ink on light colors, white on dark
 * ones, whichever has the higher WCAG contrast.
 */
export function textOnColor(color: string): string {
  const background = normalizeColor(color) ?? DEFAULT_ATTENTION_COLOR;
  return contrastRatio(background, INK) >= contrastRatio(background, WHITE) ? INK : WHITE;
}

/** `color-mix(in srgb, color percent%, base)`, as CSS computes it. */
export function mixColors(color: string, percent: number, base: string): string {
  const share = percent / 100;
  const channel = (hex: string, offset: number) => parseInt(hex.slice(offset, offset + 2), 16);
  const parts = [1, 3, 5].map((offset) =>
    Math.round(channel(color, offset) * share + channel(base, offset) * (1 - share))
      .toString(16)
      .padStart(2, "0"),
  );
  return `#${parts.join("")}`;
}

export interface AttentionTint {
  /** Percent of the attention color at the calm end of the breath. */
  low: number;
  /** Percent at the peak (and the still highlight with reduced motion). */
  high: number;
}

/**
 * The strongest tint at which the row's text keeps 4.5 : 1 and its muted text 3 : 1. Light
 * colors in the light theme (and dark ones in the dark theme) reach the full peak; others get
 * a weaker tint, never below the minimum.
 */
export function attentionTint(color: string, theme: Theme): AttentionTint {
  const tint = normalizeColor(color) ?? DEFAULT_ATTENTION_COLOR;
  const row = ROW_COLORS[theme];
  const { min, max } = PEAK[theme];
  let high = min;
  for (let percent = max; percent > min; percent -= 1) {
    const background = mixColors(tint, percent, row.panel);
    if (contrastRatio(background, row.text) >= 4.5 && contrastRatio(background, row.muted) >= 3) {
      high = percent;
      break;
    }
  }
  return { low: Math.round(high * LOW_SHARE), high };
}

/** Edge, ring and chip of a waiting session; non-text parts need 3 : 1 against the sidebar. */
const EDGE_CONTRAST = 3;
/** Text in the state color itself (the "working" chip label) needs 4.5 : 1. */
const TEXT_CONTRAST = 4.5;

export interface AttentionEdges {
  /** Edge, dot and outline of a waiting row (`--attention-strong`). */
  strong: string;
  /** Thin ring around the row, already mixed onto the sidebar color (`--attention-ring`). */
  ring: string;
  chipBg: string;
  chipFg: string;
  /** Text in the state color directly on the sidebar, at 4.5 : 1. */
  text: string;
}

/**
 * Moves `color` toward `target` (white in the dark theme, black in the light one) in small steps
 * until it reaches `ratio` against `panel`. Colors that already reach it stay unchanged.
 */
function towardContrast(color: string, target: string, panel: string, ratio: number): string {
  for (let share = 0; share <= 100; share += 5) {
    const candidate = mixColors(target, share, color);
    if (contrastRatio(candidate, panel) >= ratio) return candidate;
  }
  return target;
}

/** The weakest mix of `color` onto `panel`, from `start` percent, that still reaches 3 : 1. */
function ringOnPanel(color: string, start: number, panel: string): string {
  for (let percent = start; percent <= 100; percent += 5) {
    const candidate = mixColors(color, percent, panel);
    if (contrastRatio(candidate, panel) >= EDGE_CONTRAST) return candidate;
  }
  return color;
}

/**
 * Colors for edge, ring and chip that stay visible on the sidebar in both themes: the light theme
 * darkens the attention color (as before, 45 % black, more if needed), the dark theme lightens a
 * color that is too dark for its sidebar. Light presets in the dark theme stay as chosen.
 */
export function attentionEdges(color: string, theme: Theme): AttentionEdges {
  const base = normalizeColor(color) ?? DEFAULT_ATTENTION_COLOR;
  const panel = ROW_COLORS[theme].panel;
  const target = theme === "light" ? "#000000" : WHITE;
  const strong = towardContrast(
    theme === "light" ? mixColors("#000000", 45, base) : base,
    target,
    panel,
    EDGE_CONTRAST,
  );
  const ring = ringOnPanel(strong, theme === "light" ? 70 : 55, panel);
  // Dark theme: the chip keeps the chosen color when it is bright enough, else the lightened one.
  const chipBg = strong;
  const text = towardContrast(strong, target, panel, TEXT_CONTRAST);
  return { strong, ring, chipBg, chipFg: textOnColor(chipBg), text };
}
