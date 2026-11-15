import { describe, expect, it } from "vitest";

import { claudeInputBlock, isInputStartRow, isRuleRow } from "./promptBlock";
import {
  arrowsToLinePoint,
  lineSelectionDeleteKeys,
  type ClickCell,
  type WrappedLine,
} from "./terminalClick";

const CL = "\x1b[D";
const CR = "\x1b[C";
const BS = "\x7f";
const WIDE = /[一-鿿]/u;

/**
 * A screen row as xterm stores it: blanks are untouched cells (ConPTY draws the blanks between
 * words with cursor moves), a wide character is its cell plus an empty right half.
 */
function row(text: string, cols = 50): ClickCell[] {
  const cells: ClickCell[] = [];
  for (const char of text) {
    const blank = char === " ";
    cells.push({
      width: WIDE.test(char) ? 2 : 1,
      text: blank ? "" : char,
      dim: false,
      inverse: false,
    });
    if (WIDE.test(char)) cells.push({ width: 0, text: "", dim: false, inverse: false });
  }
  while (cells.length < cols) cells.push({ width: 1, text: "", dim: false, inverse: false });
  return cells;
}

const rule = (cols = 50) => row("─".repeat(cols), cols);

/** The input block of a measured screen: the text rows and the rule below them. */
function block(texts: string[], cols = 50): ClickCell[][] {
  return [...texts.map((text) => row(text, cols)), rule(cols)];
}

function line(texts: string[], cols = 50): WrappedLine {
  const joined = claudeInputBlock(block(texts, cols), cols);
  if (joined === null) throw new Error("expected an input block");
  return joined;
}

/** Run A of the measurement (Claude Code 2.1.325, 50 columns): words, wrapped at word boundaries. */
const WORDS = [
  "❯ alpha bravo charlie delta echo foxtrot golf",
  "  hotel india juliet kilo lima mike november",
  "  oscar papa quebec romeo sierra tango",
];

describe("claudeInputBlock", () => {
  it("recognises the input mark, the indented rows and the rule below", () => {
    expect(isInputStartRow(row("❯ abc"))).toBe(true);
    expect(isInputStartRow(row("> abc"))).toBe(false);
    expect(isRuleRow(rule())).toBe(true);
    expect(isRuleRow(row("── x"))).toBe(false);
    const joined = line(WORDS);
    expect(joined.rowStarts).toEqual([0, 46, 89]);
    expect(joined.rowSkips).toEqual([0, 2, 2]);
  });

  it("moves the cursor over the boundaries as Left and Right did in the measurement", () => {
    const joined = line(WORDS);
    // Cursor at the start of "oscar" (row 9 col 3 on the screen). Left once reached the cell after
    // "november" (row 8 col 45), Left twice the cell before its last letter (col 44).
    const start = { row: 2, col: 2 };
    expect(arrowsToLinePoint(joined, start, { row: 1, col: 44 }, false)).toBe(CL);
    expect(arrowsToLinePoint(joined, start, { row: 1, col: 43 }, false)).toBe(CL.repeat(2));
    // And back: Right from the boundary cell reached the start of "oscar", once more "s".
    const boundary = { row: 1, col: 44 };
    expect(arrowsToLinePoint(joined, boundary, { row: 2, col: 2 }, false)).toBe(CR);
    expect(arrowsToLinePoint(joined, boundary, { row: 2, col: 3 }, false)).toBe(CR.repeat(2));
  });

  it("deletes over a boundary with one Backspace for it, as measured", () => {
    const joined = line(WORDS);
    // Measured: from the cell before "papa" (row 9 col 9) seven Backspaces left "novemberpapa":
    // the blank after "oscar", "oscar" itself (5) and the boundary: 7 characters.
    expect(
      lineSelectionDeleteKeys(
        joined,
        { row: 2, col: 8 },
        { start: { row: 1, col: 44 }, end: { row: 2, col: 8 } },
        false,
      ),
    ).toBe(BS.repeat(7));
  });

  it("walks to the end of a selection that starts in the upper row", () => {
    const joined = line(WORDS);
    // "golf", the boundary and "hotel" = 10 characters; 74 characters stand between the selection
    // and the cursor at the end of the text.
    expect(
      lineSelectionDeleteKeys(
        joined,
        { row: 2, col: 38 },
        { start: { row: 0, col: 41 }, end: { row: 1, col: 7 } },
        false,
      ),
    ).toBe(CL.repeat(74) + BS.repeat(10));
  });

  it("still edits inside one row", () => {
    const joined = line(WORDS);
    expect(
      lineSelectionDeleteKeys(
        joined,
        { row: 2, col: 38 },
        { start: { row: 2, col: 32 }, end: { row: 2, col: 38 } },
        false,
      ),
    ).toBe(BS.repeat(6));
  });

  it("does not take the prompt mark, the indent or the margin as text", () => {
    const joined = line(WORDS);
    const cursor = { row: 2, col: 38 };
    // From the prompt mark.
    expect(
      lineSelectionDeleteKeys(
        joined,
        cursor,
        { start: { row: 0, col: 0 }, end: { row: 1, col: 7 } },
        false,
      ),
    ).toBeNull();
    // Into the blank space after the text.
    expect(
      lineSelectionDeleteKeys(
        joined,
        cursor,
        { start: { row: 2, col: 30 }, end: { row: 2, col: 45 } },
        false,
      ),
    ).toBeNull();
    // A selection that starts in the indent of a row starts at the row's first character.
    expect(
      lineSelectionDeleteKeys(
        joined,
        { row: 1, col: 44 },
        { start: { row: 1, col: 0 }, end: { row: 1, col: 7 } },
        false,
      ),
    ).toBe(CL.repeat(37) + BS.repeat(5));
  });

  it("counts a Shift+Enter line break as one character", () => {
    // Run E: "hello", ESC CR, "world". Left from the start of "world" reached the cell after
    // "hello" (row 7 col 8), and one Backspace there joined the lines.
    const joined = line(["❯ hello", "  world"]);
    expect(arrowsToLinePoint(joined, { row: 1, col: 2 }, { row: 0, col: 7 }, false)).toBe(CL);
    expect(
      lineSelectionDeleteKeys(
        joined,
        { row: 1, col: 2 },
        { start: { row: 0, col: 7 }, end: { row: 1, col: 2 } },
        false,
      ),
    ).toBe(BS);
    // An empty line between two others is one character too.
    const gap = line(["❯ hello", "  ", "  world"]);
    expect(
      lineSelectionDeleteKeys(
        gap,
        { row: 2, col: 7 },
        { start: { row: 0, col: 7 }, end: { row: 2, col: 2 } },
        false,
      ),
    ).toBe(CL.repeat(5) + BS.repeat(2));
  });

  it("counts a wrap as one character although two blanks were typed (known limit)", () => {
    // Run B1: 43 "a", two blanks, "word". Both blanks sit at the end of the upper row and are not
    // drawn, so the screen looks like a single blank. Backspaces never exceed the count shown.
    const joined = line([`❯ ${"a".repeat(43)}`, "  word"]);
    expect(
      lineSelectionDeleteKeys(
        joined,
        { row: 1, col: 6 },
        { start: { row: 0, col: 45 }, end: { row: 1, col: 6 } },
        false,
      ),
    ).toBe(BS.repeat(5));
  });

  it("handles a row filled to the edge followed by a blank and a word", () => {
    // Run C1: 10 "a", blank, 35 "b" fill the row; the blank typed next is drawn nowhere.
    const filled = `❯ ${"a".repeat(10)} ${"b".repeat(35)}`;
    const joined = line([filled, "  next"]);
    // The last "b", the blank and "next": six characters, measured with six Lefts.
    expect(
      lineSelectionDeleteKeys(
        joined,
        { row: 1, col: 6 },
        { start: { row: 0, col: 47 }, end: { row: 1, col: 6 } },
        false,
      ),
    ).toBe(BS.repeat(6));
    // At the start of the lower row the cursor shows the same cell before and after the blank.
    expect(arrowsToLinePoint(joined, { row: 1, col: 2 }, { row: 0, col: 40 }, false)).toBe("");
    expect(
      lineSelectionDeleteKeys(
        joined,
        { row: 1, col: 2 },
        { start: { row: 0, col: 40 }, end: { row: 0, col: 44 } },
        false,
      ),
    ).toBeNull();
    // One column further the cursor is certain again.
    expect(arrowsToLinePoint(joined, { row: 1, col: 3 }, { row: 1, col: 2 }, false)).toBe(CL);
    // The pending position right of a filled row names no single position either.
    expect(arrowsToLinePoint(joined, { row: 0, col: 48 }, { row: 1, col: 3 }, false)).toBe("");
  });

  it("holds for another width: the text area ends two columns before the edge", () => {
    const filled = `❯ ${"abcdefgh ".repeat(8)}wxyz`;
    expect(filled.length).toBe(78);
    const joined = line([filled, "  next"], 80);
    expect(
      lineSelectionDeleteKeys(
        joined,
        { row: 1, col: 6 },
        { start: { row: 0, col: 77 }, end: { row: 1, col: 6 } },
        false,
      ),
    ).toBe(BS.repeat(6));
    expect(claudeInputBlock(block([filled, "  next"], 80), 50)).toBeNull();
  });

  it("gives up where the screen cannot tell a blank from a split word", () => {
    // Run C2: one word of 50 characters is split inside the word at the edge.
    expect(claudeInputBlock(block([`❯ ${"a".repeat(46)}`, "  aaaa"]), 50)).toBeNull();
    // A word that would fit on one row is never split, so a blank must follow the filled row.
    expect(claudeInputBlock(block([`❯ ${"a".repeat(46)}`, "  "]), 50)).not.toBeNull();
    // A filled row whose last word and the start of the next one could be one long word.
    const long = `❯ ab ${"x".repeat(43)}`;
    expect(claudeInputBlock(block([long, `  ${"x".repeat(10)} next`]), 50)).toBeNull();
  });

  it("gives up at a wide character at a boundary", () => {
    // Run D: 45 "a" and a wide character that does not fit at the edge: no character at the
    // boundary although the row ends before the edge.
    expect(claudeInputBlock(block([`❯ ${"a".repeat(45)}`, "  中bc"]), 50)).toBeNull();
    // A wide character elsewhere is fine.
    expect(claudeInputBlock(block(["❯ 中bc def", "  ghi"]), 50)).not.toBeNull();
  });

  it("gives up when the rows are not Claude Code's input block", () => {
    // No input mark.
    expect(claudeInputBlock(block(["> alpha", "  beta"]), 50)).toBeNull();
    // A continuation row with another indent, a prompt mark or a box side.
    expect(claudeInputBlock(block(["❯ alpha", "   beta"]), 50)).toBeNull();
    expect(claudeInputBlock(block(["❯ alpha", " beta"]), 50)).toBeNull();
    expect(claudeInputBlock(block(["❯ alpha", "  ❯ beta"]), 50)).toBeNull();
    expect(claudeInputBlock(block(["❯ alpha", "  │ beta"]), 50)).toBeNull();
    expect(claudeInputBlock(block(["❯ alpha", "beta"]), 50)).toBeNull();
    // No rule below the block, or only the one row.
    expect(claudeInputBlock([row("❯ alpha"), row("  beta"), row("  gamma")], 50)).toBeNull();
    expect(claudeInputBlock(block(["❯ alpha"]), 50)).toBeNull();
    // Text in the right margin.
    expect(claudeInputBlock(block(["❯ alpha", `  ${"b".repeat(47)}`]), 50)).toBeNull();
  });

  it("gives up while the cursor is where two positions look alike", () => {
    const joined = line(WORDS);
    // Beyond the boundary cell of an upper row.
    expect(arrowsToLinePoint(joined, { row: 1, col: 46 }, { row: 2, col: 3 }, false)).toBe("");
    expect(
      lineSelectionDeleteKeys(
        joined,
        { row: 1, col: 46 },
        { start: { row: 2, col: 2 }, end: { row: 2, col: 5 } },
        false,
      ),
    ).toBeNull();
    // On the rule row or beyond the block.
    expect(arrowsToLinePoint(joined, { row: 3, col: 5 }, { row: 2, col: 3 }, false)).toBe("");
  });
});
