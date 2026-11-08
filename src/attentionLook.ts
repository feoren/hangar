// Colors derived from the user's attention color, so any choice (preset or custom) stays readable:
// the text color on the status chip, and how strongly a waiting row breathes. The breathing tint
// is mixed onto the sidebar color; its peak is as strong as the row's text still allows.

import { normalizeColor, DEFAULT_ATTENTION_COLOR } from "./settings";
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
