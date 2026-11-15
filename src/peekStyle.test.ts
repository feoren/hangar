import { describe, expect, it } from "vitest";

import {
  cellStyle,
  lineRuns,
  paletteColor,
  plainLines,
  plainRun,
  readableColor,
  runStyle,
  sameLine,
  samePeekContent,
  styledTail,
  type BufferLike,
  type CellLike,
  type LineLike,
  type PeekContent,
} from "./peekStyle";
import { schemeColors } from "./terminalSchemes";
import { contrastRatio } from "./theme";

const colors = schemeColors("hangar", "dark");

type Color =
  { mode: "default" } | { mode: "palette"; index: number } | { mode: "rgb"; rgb: number };

interface FakeCell {
  chars: string;
  width?: number;
  fg?: Color;
  bg?: Color;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
  inverse?: boolean;
  invisible?: boolean;
  strike?: boolean;
}

const flag = (value: boolean | undefined): number => (value === true ? 1 : 0);

function cell(spec: FakeCell): CellLike {
  const fg = spec.fg ?? { mode: "default" };
  const bg = spec.bg ?? { mode: "default" };
  const value = (color: Color): number =>
    color.mode === "palette" ? color.index : color.mode === "rgb" ? color.rgb : 0;
  return {
    getWidth: () => spec.width ?? 1,
    getChars: () => spec.chars,
    getFgColor: () => value(fg),
    getBgColor: () => value(bg),
    isFgRGB: () => fg.mode === "rgb",
    isBgRGB: () => bg.mode === "rgb",
    isFgPalette: () => fg.mode === "palette",
    isBgPalette: () => bg.mode === "palette",
    isBold: () => flag(spec.bold),
    isDim: () => flag(spec.dim),
    isItalic: () => flag(spec.italic),
    isUnderline: () => flag(spec.underline),
    isInverse: () => flag(spec.inverse),
    isInvisible: () => flag(spec.invisible),
    isStrikethrough: () => flag(spec.strike),
  };
}

function line(cells: readonly FakeCell[], unreadable = false): LineLike {
  const text = cells.map((c) => (c.width === 0 ? "" : c.chars || " ")).join("");
  return {
    length: cells.length,
    getCell: (x) => {
      if (unreadable) return undefined;
      const spec = cells[x];
      return spec === undefined ? undefined : cell(spec);
    },
    translateToString: (trimRight) => (trimRight === true ? text.trimEnd() : text),
  };
}

const text = (value: string, extra: Omit<FakeCell, "chars"> = {}): FakeCell[] =>
  Array.from(value, (chars) => chars).map((chars) => ({ chars, ...extra }));

function buffer(lines: readonly LineLike[]): BufferLike {
  return { length: lines.length, getLine: (y) => lines[y] };
}

describe("paletteColor", () => {
  it("takes the 16 base colors from the scheme", () => {
    expect(paletteColor(1, colors)).toBe(colors.red);
    expect(paletteColor(15, colors)).toBe(colors.brightWhite);
  });

  it("computes the color cube and the gray ramp", () => {
    expect(paletteColor(16, colors)).toBe("#000000");
    expect(paletteColor(196, colors)).toBe("#ff0000");
    expect(paletteColor(231, colors)).toBe("#ffffff");
    expect(paletteColor(232, colors)).toBe("#080808");
    expect(paletteColor(255, colors)).toBe("#eeeeee");
  });
});

describe("cellStyle", () => {
  it("leaves default colors to the peek", () => {
    const style = cellStyle(cell({ chars: "a" }), colors);
    expect(style.fg).toBeNull();
    expect(style.bg).toBeNull();
  });

  it("writes truecolor as hex", () => {
    const style = cellStyle(cell({ chars: "a", fg: { mode: "rgb", rgb: 0x0a0b0c } }), colors);
    expect(style.fg).toBe("#0a0b0c");
  });

  it("draws bold base colors bright, like xterm", () => {
    const style = cellStyle(
      cell({ chars: "a", fg: { mode: "palette", index: 2 }, bold: true }),
      colors,
    );
    expect(style.fg).toBe(colors.brightGreen);
    expect(style.bold).toBe(true);
  });

  it("swaps colors for inverse, defaults included", () => {
    const style = cellStyle(cell({ chars: "a", inverse: true }), colors);
    expect(style.fg).toBe(colors.background);
    expect(style.bg).toBe(colors.foreground);
  });

  it("gives dim text a concrete color", () => {
    expect(cellStyle(cell({ chars: "a", dim: true }), colors).fg).toBe(colors.foreground);
  });

  it("hides invisible text in its background", () => {
    const style = cellStyle(
      cell({ chars: "a", invisible: true, bg: { mode: "palette", index: 4 } }),
      colors,
    );
    expect(style.fg).toBe(colors.blue);
  });
});

describe("readableColor", () => {
  it("keeps colors that already read well and any color when no minimum applies", () => {
    expect(readableColor("#000000", "#ffffff", 4.5)).toBe("#000000");
    expect(readableColor("#ffff00", "#ffffff", 1)).toBe("#ffff00");
  });

  it("darkens light text on a light background up to the ratio, like xterm", () => {
    const result = readableColor("#ffff00", "#ffffff", 4.5);
    expect(contrastRatio(result, "#ffffff")).toBeGreaterThanOrEqual(4.5);
    expect(contrastRatio(result, "#ffffff")).toBeLessThan(6);
  });

  it("brightens dark text on a dark background", () => {
    const result = readableColor("#101010", "#000000", 4.5);
    expect(contrastRatio(result, "#000000")).toBeGreaterThanOrEqual(4.5);
  });

  it("leaves colors it cannot parse alone", () => {
    expect(readableColor("red", "#ffffff", 4.5)).toBe("red");
  });
});

describe("cellStyle with a minimum contrast", () => {
  const light = schemeColors("hangar", "light");
  const yellow = { mode: "rgb", rgb: 0xffff00 } as const;

  it("raises truecolor meant for dark terminals on the light scheme", () => {
    const style = cellStyle(cell({ chars: "a", fg: yellow }), light, 4.5);
    expect(style.fg).not.toBeNull();
    expect(contrastRatio(style.fg ?? "", light.background)).toBeGreaterThanOrEqual(4.5);
  });

  it("asks half the ratio of dim text and none of invisible text", () => {
    const dim = cellStyle(cell({ chars: "a", fg: yellow, dim: true }), light, 4.5);
    expect(contrastRatio(dim.fg ?? "", light.background)).toBeGreaterThanOrEqual(2.25);
    expect(contrastRatio(dim.fg ?? "", light.background)).toBeLessThan(4.5);
    expect(cellStyle(cell({ chars: "a", invisible: true }), light, 4.5).fg).toBe(light.background);
  });
});

describe("lineRuns", () => {
  it("hands the reusable cell to every read", () => {
    const reuse = cell({ chars: "" });
    const seen: (CellLike | undefined)[] = [];
    const source = line(text("ab"));
    const spy: LineLike = {
      length: source.length,
      translateToString: (trim) => source.translateToString(trim),
      getCell: (x, into) => {
        seen.push(into);
        return source.getCell(x);
      },
    };
    lineRuns(spy, colors, 160, 1, reuse);
    expect(seen).toEqual([reuse, reuse]);
  });

  it("merges neighbors in the same style and trims the blank end", () => {
    const runs = lineRuns(
      line([
        ...text("ok ", { fg: { mode: "palette", index: 2 } }),
        ...text("done"),
        ...text("   "),
      ]),
      colors,
    );
    expect(runs.map((run) => run.text)).toEqual(["ok ", "done"]);
    expect(runs[0]?.fg).toBe(colors.green);
  });

  it("keeps trailing space that shows a background", () => {
    const runs = lineRuns(
      line([...text("a"), ...text("  ", { bg: { mode: "palette", index: 1 } })]),
      colors,
    );
    expect(runs.map((run) => run.text)).toEqual(["a", "  "]);
  });

  it("skips the cell after a wide character and fills empty cells", () => {
    const runs = lineRuns(
      line([{ chars: "中", width: 2 }, { chars: "", width: 0 }, { chars: "" }, { chars: "x" }]),
      colors,
    );
    expect(runs.map((run) => run.text).join("")).toBe("中 x");
  });

  it("reads at most the given columns", () => {
    expect(lineRuns(line(text("abcdef")), colors, 3).map((run) => run.text)).toEqual(["abc"]);
  });

  it("falls back to plain text when cells cannot be read", () => {
    expect(lineRuns(line(text("plain  "), true), colors)).toEqual([plainRun("plain")]);
  });
});

describe("styledTail", () => {
  it("picks the last rows with content, like the plain reader", () => {
    const rows = [line(text("one")), line(text("two")), line(text("three")), line(text("   "))];
    const lines = styledTail(buffer(rows), 2, colors);
    expect(lines.map((runs) => runs.map((run) => run.text).join(""))).toEqual(["two", "three"]);
  });

  it("reads nothing from an empty buffer", () => {
    expect(styledTail(buffer([line(text("  "))]), 5, colors)).toEqual([]);
  });
});

describe("plainLines", () => {
  it("turns text into unstyled runs", () => {
    expect(plainLines(["a", ""])).toEqual([[plainRun("a")], []]);
  });
});

describe("samePeekContent", () => {
  const content = (fg: string | null): PeekContent => ({
    lines: [[{ ...plainRun("x"), fg }]],
    background: "#000",
    foreground: "#fff",
  });

  it("compares text and style", () => {
    expect(samePeekContent(content("#f00"), content("#f00"))).toBe(true);
    expect(samePeekContent(content("#f00"), content("#0f0"))).toBe(false);
    expect(sameLine([plainRun("a")], [plainRun("a"), plainRun("b")])).toBe(false);
  });

  it("notices a changed scheme", () => {
    expect(samePeekContent(content(null), { ...content(null), background: "#111" })).toBe(false);
  });

  it("treats a missing peek as its own value", () => {
    expect(samePeekContent(null, null)).toBe(true);
    expect(samePeekContent(null, content(null))).toBe(false);
  });
});

describe("runStyle", () => {
  it("needs no style for default text", () => {
    expect(runStyle(plainRun("a"))).toBeUndefined();
  });

  it("maps the attributes to CSS", () => {
    expect(
      runStyle({
        text: "a",
        fg: "#ff0000",
        bg: "#000000",
        bold: true,
        dim: true,
        italic: true,
        underline: true,
        strike: true,
      }),
    ).toEqual({
      color: "color-mix(in srgb, #ff0000 50%, transparent)",
      background: "#000000",
      fontWeight: 700,
      fontStyle: "italic",
      textDecorationLine: "underline line-through",
    });
  });
});
