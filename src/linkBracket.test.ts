import { describe, expect, it } from "vitest";

import { linkBrackets, type BracketRow } from "./linkBracket";
import type { LinkMark } from "./links";

function mark(group: number, color: number, braked = false): LinkMark {
  return { group, color, name: `m${String(group)}`, braked, lead: false };
}

function rows(...entries: [number, string][]): BracketRow[] {
  return entries.map(([id, section]) => ({ id, section }));
}

describe("linkBrackets", () => {
  it("draws one run over members that follow each other, labelled at its top", () => {
    const marks = new Map([
      [1, mark(7, 2)],
      [2, mark(7, 2)],
      [3, mark(7, 2)],
    ]);
    const pieces = linkBrackets(rows([1, "a"], [2, "a"], [3, "a"], [4, "a"]), marks);
    expect(pieces.get(1)).toEqual({
      group: 7,
      color: 2,
      braked: false,
      first: true,
      last: false,
      label: true,
      members: 3,
    });
    expect(pieces.get(2)).toMatchObject({ first: false, last: false, label: false });
    expect(pieces.get(3)).toMatchObject({ first: false, last: true, label: false });
    expect(pieces.has(4)).toBe(false);
  });

  it("splits at a row that is no member, labelling only the first run", () => {
    const marks = new Map([
      [1, mark(7, 2)],
      [3, mark(7, 2)],
    ]);
    const pieces = linkBrackets(rows([1, "a"], [2, "a"], [3, "a"]), marks);
    expect(pieces.get(1)).toMatchObject({ first: true, last: true, label: true, members: 2 });
    expect(pieces.get(3)).toMatchObject({ first: true, last: true, label: false, members: 2 });
  });

  it("splits where the next row sits in another sidebar group", () => {
    const marks = new Map([
      [1, mark(7, 2)],
      [2, mark(7, 2)],
    ]);
    const pieces = linkBrackets(rows([1, "a"], [2, "b"]), marks);
    expect(pieces.get(1)).toMatchObject({ first: true, last: true, label: true });
    expect(pieces.get(2)).toMatchObject({ first: true, last: true, label: false });
  });

  it("keeps neighbouring groups apart, each with its own color and label", () => {
    const marks = new Map([
      [1, mark(7, 2)],
      [2, mark(7, 2)],
      [3, mark(8, 4, true)],
      [4, mark(8, 4, true)],
    ]);
    const pieces = linkBrackets(rows([1, "a"], [2, "a"], [3, "a"], [4, "a"]), marks);
    expect(pieces.get(2)).toMatchObject({ group: 7, last: true });
    expect(pieces.get(3)).toMatchObject({
      group: 8,
      color: 4,
      braked: true,
      first: true,
      label: true,
    });
  });

  it("counts members that the list does not show", () => {
    const marks = new Map([
      [1, mark(7, 2)],
      [2, mark(7, 2)],
      [9, mark(7, 2)],
    ]);
    expect(linkBrackets(rows([1, "a"], [2, "a"]), marks).get(1)?.members).toBe(3);
  });

  it("marks nothing without groups", () => {
    expect(linkBrackets(rows([1, "a"]), new Map()).size).toBe(0);
  });
});
