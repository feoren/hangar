// CSS custom properties for the state looks (stateLook.ts). The app sets them on the root; the
// classes `look-working`, `look-done`, `look-asks`, `look-idle` and `look-failed` in styles.css hand
// them to a row, so the sidebar, the rail, the chips and the settings preview all follow one source.

import { attentionEdges, attentionTint, textOnColor } from "./attentionLook";
import {
  LOOK_STATES,
  lookWaits,
  pulseLevels,
  resolvedColor,
  themeToken,
  type LookState,
  type StateLooks,
} from "./stateLook";
import type { Theme } from "./theme";

const percent = (value: number) => `${String(value)}%`;

function stateVariables(looks: StateLooks, state: LookState, theme: Theme): [string, string][] {
  const look = looks[state];
  const color = resolvedColor(looks, state, theme);
  const waits = lookWaits(state);
  const levels = pulseLevels(attentionTint(color, theme).high, look.pulse, waits);
  const edges = attentionEdges(color, theme);
  // Working and pause without a chosen color keep the theme's own green, exactly as before, and
  // failed its red; so the default follows a theme switch without new values.
  const token = look.color === null ? themeToken(state) : null;
  const themed = token !== null;
  const prefix = `--look-${state}`;
  const tint = token === null ? color : `var(${token})`;
  // Red text needs a darker shade in the light theme to keep 4.5 : 1 (`--chip-failed-fg`).
  const themedText = state === "failed" ? "var(--chip-failed-fg)" : tint;
  return [
    [`${prefix}-color`, tint],
    // Still tint without a pulse and with reduced motion; none at all keeps the row plain.
    [
      `${prefix}-fill`,
      levels.still === 0
        ? "transparent"
        : `color-mix(in srgb, ${tint} ${percent(levels.still)}, var(--panel))`,
    ],
    [`${prefix}-strong`, themed ? tint : edges.strong],
    // The "working" chip label is text in this color, so it needs 4.5 : 1 instead of 3 : 1.
    [`${prefix}-text`, themed ? themedText : edges.text],
    [`${prefix}-ring`, edges.ring],
    [`${prefix}-chip-bg`, edges.chipBg],
    [`${prefix}-chip-fg`, edges.chipFg],
    [`${prefix}-low`, percent(levels.low)],
    [`${prefix}-high`, percent(levels.high)],
    [`${prefix}-still`, percent(levels.still)],
    [`${prefix}-seconds`, `${String(look.seconds)}s`],
    [`${prefix}-pulse`, levels.animated ? "row-breathe" : "none"],
    [`${prefix}-ring-pulse`, levels.animated ? "rail-ring" : "none"],
  ];
}

/**
 * Every variable for every state, plus the general attention variables (`--attention` and its
 * companions), which follow "done" so that other places marking a waiting session stay in step.
 */
export function lookVariables(looks: StateLooks, theme: Theme): Record<string, string> {
  const entries = LOOK_STATES.flatMap((state) => stateVariables(looks, state, theme));
  const done = resolvedColor(looks, "done", theme);
  const edges = attentionEdges(done, theme);
  entries.push(
    ["--attention", done],
    ["--on-attention", textOnColor(done)],
    ["--attention-strong", edges.strong],
    ["--attention-ring", edges.ring],
    ["--attention-chip-bg", edges.chipBg],
    ["--attention-chip-fg", edges.chipFg],
  );
  return Object.fromEntries(entries);
}
