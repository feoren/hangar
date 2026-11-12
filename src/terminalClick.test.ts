import { describe, expect, it } from "vitest";

import { arrowsToColumn, clickMovesCursor, type ClickCell } from "./terminalClick";

const cells = (widths: number[]): ClickCell[] => widths.map((width) => ({ width }));

describe("clickMovesCursor", () => {
  const plain = {
    shell: "agent",
    mouseTracking: "none",
    selected: false,
    modifier: false,
  } as const;

  it("moves only for a plain click in an agent that does not track the mouse", () => {
    expect(clickMovesCursor(plain)).toBe(true);
    expect(clickMovesCursor({ ...plain, shell: "powershell" })).toBe(false);
    expect(clickMovesCursor({ ...plain, shell: "wsl" })).toBe(false);
    expect(clickMovesCursor({ ...plain, mouseTracking: "vt200" })).toBe(false);
    expect(clickMovesCursor({ ...plain, selected: true })).toBe(false);
    expect(clickMovesCursor({ ...plain, modifier: true })).toBe(false);
  });
});

describe("arrowsToColumn", () => {
  const ascii = cells([1, 1, 1, 1, 1, 1, 1, 1]);

  it("presses Right or Left once per character", () => {
    expect(arrowsToColumn(ascii, 2, 5, false)).toBe("\x1b[C\x1b[C\x1b[C");
    expect(arrowsToColumn(ascii, 5, 3, false)).toBe("\x1b[D\x1b[D");
    expect(arrowsToColumn(ascii, 4, 4, false)).toBe("");
  });

  it("uses the application cursor form when the program asked for it", () => {
    expect(arrowsToColumn(ascii, 0, 2, true)).toBe("\x1bOC\x1bOC");
  });

  it("counts a wide character as one press, from either of its halves", () => {
    // "a", wide "字" in cells 1 and 2, "b", "c".
    const wide = cells([1, 2, 0, 1, 1]);
    expect(arrowsToColumn(wide, 0, 3, false)).toBe("\x1b[C\x1b[C");
    // From "c" back to "字": past "b" and "字", two presses either way.
    expect(arrowsToColumn(wide, 4, 2, false)).toBe("\x1b[D\x1b[D");
    expect(arrowsToColumn(wide, 4, 1, false)).toBe("\x1b[D\x1b[D");
  });

  it("treats cells past the stored line as blanks", () => {
    expect(arrowsToColumn(cells([1, 1]), 1, 4, false)).toBe("\x1b[C\x1b[C\x1b[C");
  });
});
