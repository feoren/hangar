import { describe, expect, it } from "vitest";

import { linkWord, type WordCell, type WordLine, type WordSource } from "./terminalLinkWord";

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
    const source = screen(row("a github.c", 10), row("om/acme", 10));
    expect(linkWord(source, { x: 2, y: 0 }, { x: 9, y: 0 })).toBe("github.c");
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
