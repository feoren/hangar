import { describe, expect, it } from "vitest";

import {
  columnOf,
  excerpt,
  findInLine,
  foldCase,
  lineEndingAt,
  lineStartingAt,
  relocate,
  rowOfOffset,
  searchTargets,
  type BufferRowText,
  type RowSource,
  type SearchHooks,
} from "./sessionSearch";

/** A buffer from rows; a row starting with "\u0001" continues the one above (soft wrap). */
function buffer(rows: readonly string[]): RowSource {
  const parsed: BufferRowText[] = rows.map((row) =>
    row.startsWith("\u0001")
      ? { text: row.slice(1), wrapped: true }
      : { text: row, wrapped: false },
  );
  return { length: parsed.length, row: (index) => parsed[index] };
}

function hooks(patch: Partial<SearchHooks> = {}): SearchHooks {
  return { pause: () => Promise.resolve(), cancelled: () => false, ...patch };
}

describe("matching", () => {
  it("ignores case without moving offsets", () => {
    expect(findInLine("Build FAILED at step 3", foldCase("failed"), false)).toBe(6);
    expect(findInLine("Build FAILED", "failed", true)).toBe(-1);
    // The dotted capital I grows when lowercased; the offset of what follows stays the same.
    const text = "İstanbul error";
    expect(foldCase(text)).toHaveLength(text.length);
    expect(findInLine(text, "error", false)).toBe(9);
  });

  it("finds nothing for an empty term", () => {
    expect(findInLine("anything", "", false)).toBe(-1);
  });
});

describe("logical lines", () => {
  const source = buffer([
    "$ npm test",
    "a very long line that wr",
    "\u0001aps onto two",
    "done   ",
  ]);

  it("joins soft wrapped rows and trims the end", () => {
    expect(lineEndingAt(source, 2)).toEqual({
      row: 1,
      text: "a very long line that wraps onto two",
      rowStarts: [0, 24],
    });
    expect(lineStartingAt(source, 1)?.text).toBe("a very long line that wraps onto two");
    expect(lineStartingAt(source, 2)).toBeNull();
    expect(lineEndingAt(source, 3)?.text).toBe("done");
  });

  it("maps an offset back to its row", () => {
    const line = lineEndingAt(source, 2);
    if (line === null) throw new Error("line expected");
    expect(rowOfOffset(line, 3)).toEqual({ row: 1, offset: 3 });
    expect(rowOfOffset(line, 25)).toEqual({ row: 2, offset: 1 });
  });
});

describe("searchTargets", () => {
  it("searches every session, newest line first, one hit per line", async () => {
    const result = await searchTargets(
      [
        { sessionId: 1, source: buffer(["error one", "fine", "error two error"]) },
        { sessionId: 2, source: buffer(["no hit"]) },
        { sessionId: 3, source: buffer(["Error three"]) },
      ],
      "error",
      {},
      hooks(),
    );
    expect(result.matches.map((match) => [match.sessionId, match.row, match.start])).toEqual([
      [1, 2, 0],
      [1, 0, 0],
      [3, 0, 0],
    ]);
    expect(result.truncated).toBe(false);
  });

  it("finds a term across a soft wrap", async () => {
    const result = await searchTargets(
      [{ sessionId: 1, source: buffer(["compiling hang", "\u0001ar done"]) }],
      "hangar",
      {},
      hooks(),
    );
    expect(result.matches).toEqual([
      { sessionId: 1, row: 0, text: "compiling hangar done", start: 10, length: 6 },
    ]);
  });

  it("caps hits per session and in all", async () => {
    const many = buffer(Array.from({ length: 100 }, (_, index) => `hit ${String(index)}`));
    const perSession = await searchTargets(
      [
        { sessionId: 1, source: many },
        { sessionId: 2, source: buffer(["hit"]) },
      ],
      "hit",
      { perSession: 10 },
      hooks(),
    );
    expect(perSession.matches.filter((match) => match.sessionId === 1)).toHaveLength(10);
    expect(perSession.matches.some((match) => match.sessionId === 2)).toBe(true);
    expect(perSession.truncated).toBe(true);
    const total = await searchTargets(
      [{ sessionId: 1, source: many }],
      "hit",
      { limit: 5, perSession: 50 },
      hooks(),
    );
    expect(total.matches).toHaveLength(5);
    expect(total.truncated).toBe(true);
  });

  it("pauses between chunks, reports progress and stops when replaced", async () => {
    const rows = buffer(Array.from({ length: 50 }, () => "line with hit"));
    let pauses = 0;
    let reports = 0;
    const result = await searchTargets(
      [{ sessionId: 1, source: rows }],
      "hit",
      { chunkRows: 10, perSession: 1000, limit: 1000 },
      hooks({
        pause: () => {
          pauses += 1;
          return Promise.resolve();
        },
        cancelled: () => pauses >= 2,
        onProgress: () => {
          reports += 1;
        },
      }),
    );
    expect(result.cancelled).toBe(true);
    expect(result.matches).toHaveLength(20);
    expect(reports).toBe(2);
  });

  it("does not search for blanks", async () => {
    const result = await searchTargets(
      [{ sessionId: 1, source: buffer(["a b"]) }],
      "  ",
      {},
      hooks(),
    );
    expect(result.matches).toEqual([]);
  });
});

describe("finding a hit again", () => {
  it("finds the line where it was", () => {
    const source = buffer(["one", "two", "three"]);
    expect(relocate(source, 1, "two")?.row).toBe(1);
  });

  it("follows a line that new output moved up", () => {
    // The oldest rows were dropped: "target" moved from row 4 to row 1.
    const source = buffer(["c", "target", "d", "e", "f"]);
    expect(relocate(source, 4, "target")?.row).toBe(1);
  });

  it("gives up when the line is gone", () => {
    expect(relocate(buffer(["a", "b"]), 0, "target", 10)).toBeNull();
  });
});

describe("columnOf", () => {
  const cell = (chars: string, width = 1) => ({ chars, width });

  it("counts a wide character as two cells", () => {
    // "日本x": 日 and 本 take two cells each.
    const cells = [cell("日", 2), cell("", 0), cell("本", 2), cell("", 0), cell("x")];
    expect(columnOf(cells, 0)).toBe(0);
    expect(columnOf(cells, 1)).toBe(2);
    expect(columnOf(cells, 2)).toBe(4);
  });

  it("reads empty cells as spaces", () => {
    const cells = [cell("a"), cell(""), cell(""), cell("b")];
    expect(columnOf(cells, 3)).toBe(3);
  });

  it("counts a character outside the BMP as one cell", () => {
    const cells = [cell("😀", 2), cell("", 0), cell("a")];
    expect(columnOf(cells, 2)).toBe(2);
  });
});

describe("excerpt", () => {
  it("keeps short lines whole", () => {
    expect(excerpt("  error: missing file", 2, 5)).toEqual({
      before: "",
      match: "error",
      after: ": missing file",
    });
  });

  it("cuts long context on both sides", () => {
    const text = `${"x".repeat(100)}NEEDLE${"y".repeat(200)}`;
    const piece = excerpt(text, 100, 6, 10, 40);
    expect(piece.before).toBe(`…${"x".repeat(10)}`);
    expect(piece.match).toBe("NEEDLE");
    expect(piece.after.endsWith("…")).toBe(true);
    expect(piece.before.length + piece.match.length + piece.after.length).toBeLessThanOrEqual(41);
  });
});
