// A plain click in an agent's prompt moves the cursor there.
//
// A terminal has no caret the mouse can place: the agent's input line reads keys only. So, like
// iTerm2's Option+click, Hangar turns a click on the cursor's own line into Left or Right arrow
// presses. Only on that line: Up and Down would page through the agent's prompt history. Never
// for plain shells (they get xterm's own Alt+click), never while the program tracks the mouse
// itself, and never when the press became a selection.

import type { ShellKind } from "./pathQuote";

/** One cell of the cursor's line: wide characters take two cells, the second with width 0. */
export interface ClickCell {
  width: number;
}

/** What decides whether a click may move the cursor. */
export interface ClickContext {
  shell: ShellKind;
  /** xterm's `modes.mouseTrackingMode`; anything but "none" means the program wants the clicks. */
  mouseTracking: string;
  /** A selection exists after the press: the user marked text and did not mean to move. */
  selected: boolean;
  /** The press carried a modifier (Alt+click is xterm's own, Shift extends a selection). */
  modifier: boolean;
}

export function clickMovesCursor(context: ClickContext): boolean {
  return (
    context.shell === "agent" &&
    context.mouseTracking === "none" &&
    !context.selected &&
    !context.modifier
  );
}

/**
 * The arrow presses that move the cursor from `cursorCol` to the character under `clickCol`, both
 * cell columns on the same line. Counts characters, not cells, so a wide character is one press.
 * Empty when the click is on the cursor. `applicationCursor` (DECCKM) selects the `ESC O` form.
 */
export function arrowsToColumn(
  cells: readonly ClickCell[],
  cursorCol: number,
  clickCol: number,
  applicationCursor: boolean,
): string {
  const target = startOfCharacter(cells, clickCol);
  if (target === cursorCol) return "";
  const [from, to] = target > cursorCol ? [cursorCol, target] : [target, cursorCol];
  let presses = 0;
  for (let col = from; col < to; col += 1) {
    // A cell past the line's stored content counts as one blank character.
    if ((cells[col]?.width ?? 1) > 0) presses += 1;
  }
  const prefix = applicationCursor ? "\x1bO" : "\x1b[";
  return (prefix + (target > cursorCol ? "C" : "D")).repeat(presses);
}

/** A click on the right half of a wide character means that character. */
function startOfCharacter(cells: readonly ClickCell[], col: number): number {
  return col > 0 && cells[col]?.width === 0 ? col - 1 : col;
}
