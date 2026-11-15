import { describe, expect, it } from "vitest";

import {
  arrowsToColumn,
  arrowsToPoint,
  clickMovesCursor,
  inputEnd,
  inputStart,
  joinWrappedRows,
  lineOffset,
  promptEditable,
  rowSelectionDeleteKeys,
  selectionDeleteKeys,
  suggestionStart,
  type ClickCell,
} from "./terminalClick";

const cell = (text: string, width = 1, dim = false, inverse = false): ClickCell => ({
  width,
  text,
  dim,
  inverse,
});

/** Plain cells of the given widths (0 marks the right half of a wide character). */
const cells = (widths: number[]): ClickCell[] => widths.map((width) => cell("x", width));

/** A line: `typed` in normal style, then `faint` as a dim suggestion, padded with blanks. */
function line(typed: string, faint = "", pad = 6): ClickCell[] {
  return [
    ...Array.from(typed, (char) => cell(char)),
    ...Array.from(faint, (char) => cell(char, 1, true)),
    ...Array.from({ length: pad }, () => cell("")),
  ];
}

/** A wide character: its left cell and the empty right half. */
const wide = (char: string): ClickCell[] => [cell(char, 2), cell("", 0)];

const CR = "\x1b[C";
const CL = "\x1b[D";
const BS = "\x7f";

describe("clickMovesCursor", () => {
  const plain = {
    shell: "agent",
    mouseTracking: "none",
    selected: false,
    modifier: false,
  } as const;

  it("moves only on a plain click in an agent session that does not track the mouse", () => {
    expect(clickMovesCursor(plain)).toBe(true);
    expect(clickMovesCursor({ ...plain, shell: "powershell" })).toBe(false);
    expect(clickMovesCursor({ ...plain, shell: "wsl" })).toBe(false);
    expect(clickMovesCursor({ ...plain, mouseTracking: "vt200" })).toBe(false);
    expect(clickMovesCursor({ ...plain, selected: true })).toBe(false);
    expect(clickMovesCursor({ ...plain, modifier: true })).toBe(false);
  });

  it("edits the prompt only in agent sessions that leave the mouse alone", () => {
    expect(promptEditable("agent", "none")).toBe(true);
    expect(promptEditable("posix", "none")).toBe(false);
    expect(promptEditable("agent", "any")).toBe(false);
  });
});

describe("arrowsToColumn", () => {
  const ascii = cells([1, 1, 1, 1, 1, 1, 1, 1]);

  it("sends one arrow per character between cursor and click", () => {
    expect(arrowsToColumn(ascii, 2, 5, false)).toBe(CR.repeat(3));
    expect(arrowsToColumn(ascii, 5, 3, false)).toBe(CL.repeat(2));
    expect(arrowsToColumn(ascii, 4, 4, false)).toBe("");
  });

  it("uses the application cursor form when DECCKM is set", () => {
    expect(arrowsToColumn(ascii, 0, 2, true)).toBe("\x1bOC\x1bOC");
  });

  it("counts a wide character once and maps its right half to it", () => {
    // Cells: a, [wide], [wide], b  ->  widths 1, 2, 0, 2, 0, 1
    const mixed = cells([1, 2, 0, 2, 0, 1]);
    expect(arrowsToColumn(mixed, 0, 3, false)).toBe(CR.repeat(2));
    // Click on the right half (col 2) of the first wide character means col 1.
    expect(arrowsToColumn(mixed, 4, 2, false)).toBe(CL.repeat(2));
    expect(arrowsToColumn(mixed, 4, 1, false)).toBe(CL.repeat(2));
  });

  it("stops at the end of the typed text when clicked into blank space after it", () => {
    // "> abc" with the cursor after "a", a click far to the right.
    expect(arrowsToColumn(line("> abc"), 3, 9, false)).toBe(CR.repeat(2));
    expect(arrowsToColumn(line("> abc"), 5, 9, false)).toBe("");
  });

  it("never moves into a faint suggestion, which Right arrow would take", () => {
    // Typed "> fix", cursor at its end, suggestion " the tests" in dim.
    const suggested = line("> fix", " the tests");
    expect(arrowsToColumn(suggested, 5, 8, false)).toBe("");
    expect(arrowsToColumn(suggested, 5, 20, false)).toBe("");
    // A click inside the typed text still moves.
    expect(arrowsToColumn(suggested, 5, 3, false)).toBe(CL.repeat(2));
  });

  it("does not move on a click into the suggestion in an empty prompt", () => {
    const empty = line("> ", "write the changelog");
    expect(arrowsToColumn(empty, 2, 10, false)).toBe("");
  });

  it("treats Ink's reverse cursor on the suggestion's first character as the suggestion", () => {
    const empty = [cell(">"), cell(" "), cell("w", 1, false, true), ...line("", "rite it")];
    expect(suggestionStart(empty, 2)).toBe(2);
    expect(arrowsToColumn(empty, 2, 6, false)).toBe("");
  });

  it("stops at the first typed character when the prompt mark is clicked", () => {
    expect(arrowsToColumn(line("> abc"), 5, 0, false)).toBe(CL.repeat(3));
    expect(arrowsToColumn(line("❯ abc"), 5, 1, false)).toBe(CL.repeat(3));
  });
});

describe("prompt bounds", () => {
  it("starts after a box side, the prompt mark and one space", () => {
    expect(inputStart(line("> abc"))).toBe(2);
    expect(inputStart(line("│ > abc"))).toBe(4);
    expect(inputStart(line("› abc"))).toBe(2);
    // A continuation line without a mark starts at its first character.
    expect(inputStart(line("  abc"))).toBe(2);
  });

  it("ends after the last typed character, before a closing box side", () => {
    expect(inputEnd(line("> abc"), 2)).toBe(5);
    expect(inputEnd(line("│ > abc   │"), 7)).toBe(7);
    expect(inputEnd([...line("> ", "", 0), ...wide("中"), ...line("")], 2)).toBe(4);
    // Typed blanks before the cursor count.
    expect(inputEnd(line("> ab  "), 6)).toBe(6);
  });

  it("finds no suggestion in plain typed text", () => {
    expect(suggestionStart(line("> abc"), 3)).toBeNull();
    expect(suggestionStart(line("> abc", " def"), 5)).toBe(6);
  });
});

describe("selectionDeleteKeys", () => {
  it("moves to the end of the selection and deletes its characters", () => {
    // "> hello world", cursor at the end (13), "world" selected (cols 8 to 13).
    const typed = line("> hello world");
    expect(selectionDeleteKeys(typed, 13, { start: 8, end: 13 }, false)).toBe(BS.repeat(5));
    // "hello" selected: six Left presses from col 13 to col 7, then five deletes.
    expect(selectionDeleteKeys(typed, 13, { start: 2, end: 7 }, false)).toBe(
      CL.repeat(6) + BS.repeat(5),
    );
    // The cursor left of the selection moves right first.
    expect(selectionDeleteKeys(typed, 2, { start: 2, end: 7 }, true)).toBe(
      "\x1bOC".repeat(5) + BS.repeat(5),
    );
  });

  it("counts a wide character as one and takes it whole", () => {
    const text = [...line("> a", "", 0), ...wide("中"), ...wide("文"), cell("b"), ...line("")];
    // Cols: 2 a, 3-4 中, 5-6 文, 7 b; the cursor after b (8).
    expect(selectionDeleteKeys(text, 8, { start: 3, end: 7 }, false)).toBe(CL + BS.repeat(2));
    // A selection ending in the right half of 文 still takes 文.
    expect(selectionDeleteKeys(text, 8, { start: 4, end: 6 }, false)).toBe(CL + BS.repeat(2));
    // Only the right half of 中: the whole character, nothing around it.
    expect(selectionDeleteKeys(text, 8, { start: 4, end: 5 }, false)).toBe(CL.repeat(2) + BS);
  });

  it("deletes a selection left of a cursor in the middle of the text", () => {
    // "> abcdef", cursor on "e" (6), "bc" selected (3 to 5): one Left, two deletes.
    expect(selectionDeleteKeys(line("> abcdef"), 6, { start: 3, end: 5 }, false)).toBe(
      CL + BS.repeat(2),
    );
  });

  it("refuses a selection under the reverse cursor on a suggestion", () => {
    const empty = [cell(">"), cell(" "), cell("w", 1, false, true), ...line("", "rite it")];
    expect(selectionDeleteKeys(empty, 2, { start: 2, end: 5 }, false)).toBeNull();
  });

  it("leaves selections outside the typed text to the usual key", () => {
    const typed = line("> abc");
    // Takes in the prompt mark.
    expect(selectionDeleteKeys(typed, 5, { start: 0, end: 4 }, false)).toBeNull();
    // Reaches into the blank space after the text.
    expect(selectionDeleteKeys(typed, 5, { start: 3, end: 8 }, false)).toBeNull();
    // Reaches into a suggestion.
    expect(
      selectionDeleteKeys(line("> fix", " the tests"), 5, { start: 2, end: 9 }, false),
    ).toBeNull();
    // Empty.
    expect(selectionDeleteKeys(typed, 5, { start: 3, end: 3 }, false)).toBeNull();
  });
});

/** Screen rows `width` cells wide, as xterm stores a soft-wrapped line: never written cells are "". */
function wrappedRows(text: string, width = 8): ClickCell[][] {
  const rows: ClickCell[][] = [];
  for (let at = 0; at < text.length || rows.length === 0; at += width) {
    const part = Array.from(text.slice(at, at + width), (char) => cell(char));
    rows.push([...part, ...Array.from({ length: width - part.length }, () => cell(""))]);
  }
  return rows;
}

describe("wrapped lines", () => {
  // "> hello world, bye" on rows of 8: "> hello " | "world, b" | "ye".
  const rows = wrappedRows("> hello world, bye");
  const end = { row: 2, col: 2 };

  it("joins the rows into one line and maps row points onto it", () => {
    const line = joinWrappedRows(rows);
    expect(line.cells).toHaveLength(24);
    expect(line.rowStarts).toEqual([0, 8, 16]);
    expect(lineOffset(line, { row: 1, col: 3 })).toBe(11);
    // The exclusive end at a row's right edge is the next row's start.
    expect(lineOffset(line, { row: 0, col: 8 })).toBe(8);
    expect(lineOffset(line, { row: 0, col: 30 })).toBe(8);
    expect(lineOffset(line, { row: 3, col: 0 })).toBeNull();
    expect(lineOffset(line, { row: -1, col: 0 })).toBeNull();
  });

  it("deletes a selection on another row of the line", () => {
    // "world" (row 1, cols 0 to 5): five Left presses from the end, five deletes.
    expect(
      rowSelectionDeleteKeys(
        rows,
        end,
        { start: { row: 1, col: 0 }, end: { row: 1, col: 5 } },
        false,
      ),
    ).toBe(CL.repeat(5) + BS.repeat(5));
  });

  it("deletes a selection that spans the wrap", () => {
    // "hello world" from row 0 col 2 to row 1 col 5.
    expect(
      rowSelectionDeleteKeys(
        rows,
        end,
        { start: { row: 0, col: 2 }, end: { row: 1, col: 5 } },
        false,
      ),
    ).toBe(CL.repeat(5) + BS.repeat(11));
    // "hello " up to the right edge of row 0.
    expect(
      rowSelectionDeleteKeys(
        rows,
        end,
        { start: { row: 0, col: 2 }, end: { row: 0, col: 8 } },
        false,
      ),
    ).toBe(CL.repeat(10) + BS.repeat(6));
  });

  it("moves right across rows when the cursor sits before the selection", () => {
    expect(
      rowSelectionDeleteKeys(
        rows,
        { row: 0, col: 4 },
        { start: { row: 2, col: 0 }, end: { row: 2, col: 2 } },
        false,
      ),
    ).toBe(CR.repeat(14) + BS.repeat(2));
  });

  it("drops the cell a wide character left blank at a row's end", () => {
    // "> abcde" and one never written cell, then 中 wrapped onto the next row, then "f".
    const padded = [
      [...Array.from("> abcde", (char) => cell(char)), cell("")],
      [...wide("中"), cell("f"), ...Array.from({ length: 5 }, () => cell(""))],
    ];
    expect(joinWrappedRows(padded).rowStarts).toEqual([0, 7]);
    // "e中" selected, the cursor after "f": one Left, two deletes (not three).
    expect(
      rowSelectionDeleteKeys(
        padded,
        { row: 1, col: 3 },
        { start: { row: 0, col: 6 }, end: { row: 1, col: 2 } },
        false,
      ),
    ).toBe(CL + BS.repeat(2));
  });

  it("keeps a blank cell at a row's end that no wide character explains", () => {
    const line = joinWrappedRows([
      [cell("a"), cell("")],
      [cell("b"), cell("")],
    ]);
    expect(line.cells).toHaveLength(4);
  });

  it("leaves selections that reach beyond the line to the usual key", () => {
    // From the row above the line (a real line break, not wrapped onto it).
    expect(
      rowSelectionDeleteKeys(
        rows,
        end,
        { start: { row: -1, col: 4 }, end: { row: 1, col: 5 } },
        false,
      ),
    ).toBeNull();
    // Into the row below it.
    expect(
      rowSelectionDeleteKeys(
        rows,
        end,
        { start: { row: 1, col: 0 }, end: { row: 3, col: 1 } },
        false,
      ),
    ).toBeNull();
    // Into the blank space after the text on the last row.
    expect(
      rowSelectionDeleteKeys(
        rows,
        end,
        { start: { row: 2, col: 0 }, end: { row: 2, col: 5 } },
        false,
      ),
    ).toBeNull();
    // Over the prompt mark on the first row.
    expect(
      rowSelectionDeleteKeys(
        rows,
        end,
        { start: { row: 0, col: 0 }, end: { row: 1, col: 2 } },
        false,
      ),
    ).toBeNull();
  });

  it("moves the cursor to a click on another row of the line", () => {
    expect(arrowsToPoint(rows, end, { row: 1, col: 2 }, false)).toBe(CL.repeat(8));
    expect(arrowsToPoint(rows, { row: 0, col: 2 }, { row: 2, col: 1 }, false)).toBe(CR.repeat(15));
    // A click past the text on the last row stops at its end.
    expect(arrowsToPoint(rows, { row: 0, col: 2 }, { row: 2, col: 6 }, false)).toBe(CR.repeat(16));
    expect(arrowsToPoint(rows, end, { row: 3, col: 0 }, false)).toBe("");
  });

  it("behaves like the single-line rules on one row", () => {
    const single = [line("> hello world")];
    expect(
      rowSelectionDeleteKeys(
        single,
        { row: 0, col: 13 },
        { start: { row: 0, col: 2 }, end: { row: 0, col: 7 } },
        false,
      ),
    ).toBe(selectionDeleteKeys(line("> hello world"), 13, { start: 2, end: 7 }, false));
    expect(arrowsToPoint(single, { row: 0, col: 13 }, { row: 0, col: 3 }, false)).toBe(
      arrowsToColumn(line("> hello world"), 13, 3, false),
    );
  });
});
