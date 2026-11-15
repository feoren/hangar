import { describe, expect, it } from "vitest";

import { osc8DecisionOf } from "./termSafety";
import { schemeColors } from "./terminalSchemes";
import {
  cellHidden,
  linkTexts,
  linkWord,
  type WordCell,
  type WordLine,
  type WordSource,
} from "./terminalLinkWord";

/** Wide characters (CJK, most emoji) take two cells, the second empty with width 0. */
const WIDE = /[\u3000-\u9FFF\uFF00-\uFFEF]|\p{Extended_Pictographic}/u;

function row(text: string, cols: number, isWrapped = false): WordLine {
  const cells: WordCell[] = [];
  for (const char of text) {
    if (WIDE.test(char)) {
      cells.push({ chars: char, width: 2 }, { chars: "", width: 0 });
    } else cells.push({ chars: char === " " ? "" : char, width: 1 });
  }
  while (cells.length < cols) cells.push({ chars: "", width: 1 });
  return { isWrapped, length: cols, cell: (x) => cells[x] };
}

function screen(...lines: WordLine[]): WordSource {
  return (y) => lines[y];
}

describe("linkWord", () => {
  it("is the link's own text when spaces surround it", () => {
    const source = screen(row("see github.com here", 30));
    expect(linkWord(source, { x: 4, y: 0 }, { x: 13, y: 0 })).toBe("github.com");
  });

  it("takes plain text printed right before or after the link", () => {
    // `https://github.com/` is plain text, only `acme` is the link.
    const source = screen(row("x https://github.com/acme.", 30));
    expect(linkWord(source, { x: 21, y: 0 }, { x: 24, y: 0 })).toBe("https://github.com/acme.");
  });

  it("keeps the spaces inside a link of several words", () => {
    const source = screen(row("> Open the pull request <", 30));
    expect(linkWord(source, { x: 2, y: 0 }, { x: 22, y: 0 })).toBe("Open the pull request");
  });

  it("follows the word through wrapped rows in both directions", () => {
    const source = screen(
      row("prompt", 10),
      row("xx https:/", 10),
      row("/github.co", 10, true),
      row("m/acme  ok", 10, true),
    );
    // The link covers only the middle row.
    expect(linkWord(source, { x: 0, y: 2 }, { x: 9, y: 2 })).toBe("https://github.com/acme");
  });

  it("does not run into the next row when it is not a wrap", () => {
    // The row ends in an empty cell: a hard line break, not a full row that goes on.
    const source = screen(row("a github.c", 12), row("om/acme", 12));
    expect(linkWord(source, { x: 2, y: 0 }, { x: 9, y: 0 })).toBe("github.c");
    const below = screen(row("github.com/", 12), row("acme", 12));
    expect(linkWord(below, { x: 0, y: 1 }, { x: 3, y: 1 })).toBe("acme");
  });

  it("joins a hard line break that looks like a wrap: a full row and text right below", () => {
    // `https://github.com/` fills the row up to the last column, the link `acme` starts the next
    // row without being marked as wrapped. On screen it reads as one address.
    const source = screen(row(" https://github.com/", 20), row("acme", 20));
    expect(linkWord(source, { x: 0, y: 1 }, { x: 3, y: 1 })).toBe("https://github.com/acme");
    const after = screen(row("x https://github.co", 19), row("m/acme", 19));
    expect(linkWord(after, { x: 2, y: 0 }, { x: 18, y: 0 })).toBe("https://github.com/acme");
  });

  it("treats invisible cells as gaps, so hidden text joins no word", () => {
    const cells: WordCell[] = Array.from("github.com/acme", (chars) => ({ chars, width: 1 }));
    for (const cell of cells.slice(0, 11)) cell.hidden = true;
    const line: WordLine = { isWrapped: false, length: cells.length, cell: (x) => cells[x] };
    expect(linkWord(screen(line), { x: 11, y: 0 }, { x: 14, y: 0 })).toBe("acme");
  });

  it("reads wide CJK and emoji cells as one character each", () => {
    const source = screen(row("見る github.com/acme 😀ok", 40));
    // "見る " fills 5 cells; the link is "github.com" at 5..14, "/acme" follows without a gap.
    expect(linkWord(source, { x: 5, y: 0 }, { x: 14, y: 0 })).toBe("github.com/acme");
    const joined = screen(row("日本github.com😀x", 40));
    expect(linkWord(joined, { x: 4, y: 0 }, { x: 13, y: 0 })).toBe("日本github.com😀x");
  });

  it("is empty for a range outside the buffer", () => {
    expect(linkWord(screen(row("abc", 3)), { x: 5, y: 3 }, { x: 6, y: 3 })).toBe("");
  });
});

describe("linkTexts", () => {
  it("gives the link's own cells and the word around them", () => {
    const source = screen(row("x https://github.com/acme.", 30));
    expect(linkTexts(source, { x: 21, y: 0 }, { x: 24, y: 0 })).toEqual({
      own: "acme",
      word: "https://github.com/acme.",
    });
  });

  it("asks for a link at the last column when the next row starts with text", () => {
    // A hard break that looks like a wrap: `1 file changed` below glues `1` to the host.
    const cols = 20;
    const top = row("Docs: github.com".padStart(cols, " "), cols);
    const source = screen(top, row("1 file changed", cols));
    const texts = linkTexts(source, { x: cols - 10, y: 0 }, { x: cols - 1, y: 0 });
    expect(texts).toEqual({ own: "github.com", word: "github.com1" });
    expect(osc8DecisionOf([texts.own, texts.word], "https://evil.example/")).toBe("confirm");
  });

  it("reads hidden cells of the link itself as spaces", () => {
    const cells: WordCell[] = Array.from("ab", (chars) => ({ chars, width: 1 }));
    const hidden = cells[1];
    if (hidden !== undefined) hidden.hidden = true;
    const line: WordLine = { isWrapped: false, length: cells.length, cell: (x) => cells[x] };
    expect(linkTexts(screen(line), { x: 0, y: 0 }, { x: 1, y: 0 }).own).toBe("a ");
  });
});

describe("cellHidden", () => {
  const plain = {
    isInvisible: () => 0,
    isBold: () => 0,
    getFgColor: () => 0,
    getBgColor: () => 0,
    isFgRGB: () => false,
    isBgRGB: () => false,
    isFgPalette: () => false,
    isBgPalette: () => false,
  };

  it("hides cells drawn invisible or in the color of their background", () => {
    expect(cellHidden({ ...plain, isInvisible: () => 1 })).toBe(true);
    const palette = { ...plain, isFgPalette: () => true, isBgPalette: () => true };
    expect(cellHidden({ ...palette, getFgColor: () => 4, getBgColor: () => 4 })).toBe(true);
    const rgb = { ...plain, isFgRGB: () => true, isBgRGB: () => true };
    expect(cellHidden({ ...rgb, getFgColor: () => 0x101010, getBgColor: () => 0x101010 })).toBe(
      true,
    );
    expect(cellHidden({ ...rgb, getFgColor: () => 0x101011, getBgColor: () => 0x101010 })).toBe(
      true,
    );
  });

  it("keeps text in its background color visible where xterm lifts the contrast", () => {
    const palette = { ...plain, isFgPalette: () => true, isBgPalette: () => true };
    const same = { ...palette, getFgColor: () => 4, getBgColor: () => 4 };
    expect(cellHidden(same, 1)).toBe(true);
    expect(cellHidden(same, 4.5)).toBe(false);
    expect(cellHidden({ ...plain, isInvisible: () => 1 }, 4.5)).toBe(true);
  });

  it("keeps normal, colored and bold cells visible", () => {
    expect(cellHidden(plain)).toBe(false);
    const palette = { ...plain, isFgPalette: () => true, isBgPalette: () => true };
    expect(cellHidden({ ...palette, getFgColor: () => 2, getBgColor: () => 4 })).toBe(false);
    // Bold text of a base color is drawn in its bright variant.
    const bold = { ...palette, isBold: () => 1, getFgColor: () => 4, getBgColor: () => 4 };
    expect(cellHidden(bold)).toBe(false);
    const rgb = { ...plain, isFgRGB: () => true, isBgRGB: () => true };
    expect(cellHidden({ ...rgb, getFgColor: () => 0xffffff, getBgColor: () => 0x000000 })).toBe(
      false,
    );
    expect(cellHidden({})).toBe(false);
  });
});

describe("cellHidden with the color scheme", () => {
  const colors = schemeColors("hangar", "dark");
  const rgbOf = (color: string) => parseInt(color.slice(1), 16);
  const base = {
    isInvisible: () => 0,
    isBold: () => 0,
    isDim: () => 0,
    isInverse: () => 0,
    getFgColor: () => 0,
    getBgColor: () => 0,
    isFgRGB: () => false,
    isBgRGB: () => false,
    isFgPalette: () => false,
    isBgPalette: () => false,
    isFgDefault: () => true,
    isBgDefault: () => true,
  };

  it("hides RGB text in the scheme's background on the default background", () => {
    const cell = {
      ...base,
      isFgDefault: () => false,
      isFgRGB: () => true,
      getFgColor: () => rgbOf(colors.background),
    };
    expect(cellHidden(cell, 1, colors)).toBe(true);
    // Where xterm lifts the contrast, it is seen.
    expect(cellHidden(cell, 4.5, colors)).toBe(false);
  });

  it("hides a palette color drawn on an RGB background of the same color", () => {
    const cell = {
      ...base,
      isFgDefault: () => false,
      isBgDefault: () => false,
      isFgPalette: () => true,
      isBgRGB: () => true,
      getFgColor: () => 1,
      getBgColor: () => rgbOf(colors.red),
    };
    expect(cellHidden(cell, 1, colors)).toBe(true);
  });

  it("hides default text on an RGB background in the scheme's foreground", () => {
    const cell = {
      ...base,
      isBgDefault: () => false,
      isBgRGB: () => true,
      getBgColor: () => rgbOf(colors.foreground),
    };
    expect(cellHidden(cell, 1, colors)).toBe(true);
    expect(cellHidden({ ...cell, isInverse: () => 1 }, 1, colors)).toBe(true);
  });

  it("reads dim text as half way to its background", () => {
    // Dark gray on black: readable, but its half-way mix is too close to the background.
    const cell = {
      ...base,
      isFgDefault: () => false,
      isBgDefault: () => false,
      isFgRGB: () => true,
      isBgRGB: () => true,
      getFgColor: () => 0x1c1c1c,
      getBgColor: () => 0x000000,
    };
    expect(cellHidden(cell, 1, colors)).toBe(false);
    expect(cellHidden({ ...cell, isDim: () => 1 }, 1, colors)).toBe(true);
  });

  it("keeps text visible that differs in hue though not in lightness", () => {
    // Red on green of about the same lightness: a contrast near 1, but it reads well.
    const cell = {
      ...base,
      isFgDefault: () => false,
      isBgDefault: () => false,
      isFgRGB: () => true,
      isBgRGB: () => true,
      getFgColor: () => 0xff0000,
      getBgColor: () => 0x009a00,
    };
    expect(cellHidden(cell, 1, colors)).toBe(false);
  });

  it("keeps the scheme's own text visible", () => {
    expect(cellHidden(base, 1, colors)).toBe(false);
    expect(cellHidden({ ...base, isDim: () => 1 }, 1, colors)).toBe(false);
    expect(cellHidden({ ...base, isInverse: () => 1 }, 1, colors)).toBe(false);
  });
});
