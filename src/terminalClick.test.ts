import { describe, expect, it } from "vitest";

import {
  arrowsToColumn,
  clickMovesCursor,
  inputEnd,
  inputStart,
  promptEditable,
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
