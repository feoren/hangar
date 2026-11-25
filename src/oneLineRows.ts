// "One line": a single switch (the sidebar's group menu and the View menu)
// that sets every session row to one close line and back. On, the row takes the one-line preset
// with compact density and the layout from before is kept; off, that layout comes back. A layout
// the person built on one line in the editor counts as on as well, and turning it off then gives
// the standard row. Pure.

import { DEFAULT_ROW_LAYOUT, presetLayout, type RowLayout } from "./rowLayout";

/** The one-line row: title, context and state on one line, the lines below them gone. */
export const ONE_LINE_LAYOUT: RowLayout = { ...presetLayout("accordion"), density: "compact" };

/**
 * The row shows on one line, at any density: the preset "One line" in the row builder counts as
 * on, so the switch and the preset of the same name never disagree.
 */
export function isOneLine(layout: RowLayout): boolean {
  return layout.lines.filter((line) => line.left.length + line.right.length > 0).length <= 1;
}

/**
 * The switch's label names what a click does: a checked "One line" read as if nothing had
 * changed once the rows were on one line.
 */
export function oneLineLabelKey(layout: RowLayout): "sidebar.oneLineOn" | "sidebar.oneLineOff" {
  return isOneLine(layout) ? "sidebar.oneLineOff" : "sidebar.oneLineOn";
}

export interface OneLineState {
  rowLayout: RowLayout;
  /** The layout to bring back when "One line" goes off; `null` while off. */
  rowLayoutBefore: RowLayout | null;
}

/** Turns "One line" on or off, keeping the layout from before for the way back. */
export function setOneLine(state: OneLineState, on: boolean): OneLineState {
  if (on === isOneLine(state.rowLayout)) return state;
  if (on) return { rowLayout: ONE_LINE_LAYOUT, rowLayoutBefore: state.rowLayout };
  const before = state.rowLayoutBefore;
  return {
    rowLayout: before === null || isOneLine(before) ? DEFAULT_ROW_LAYOUT : before,
    rowLayoutBefore: null,
  };
}

export function toggleOneLine(state: OneLineState): OneLineState {
  return setOneLine(state, !isOneLine(state.rowLayout));
}
