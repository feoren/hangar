import { describe, expect, it } from "vitest";

import { fuzzyMatch, highlightRuns, matchWord } from "./fuzzy";

function score(query: string, text: string): number {
  const match = fuzzyMatch(query, text);
  if (match === null) throw new Error(`"${query}" should match "${text}"`);
  return match.score;
}

describe("matchWord", () => {
  it("matches letters in order, ignoring case", () => {
    expect(matchWord("cld", "Claude")?.positions).toEqual([0, 1, 4]);
    expect(matchWord("CLAUDE", "claude code")?.positions).toEqual([0, 1, 2, 3, 4, 5]);
  });

  it("rejects letters out of order or missing", () => {
    expect(matchWord("dc", "cd")).toBeNull();
    expect(matchWord("x", "Claude")).toBeNull();
    expect(matchWord("claudes", "claude")).toBeNull();
  });

  it("prefers word starts", () => {
    // "cc" at the starts of "Claude Code" beats "cc" inside "accent".
    expect(matchWord("cc", "Claude Code")?.positions).toEqual([0, 7]);
    expect(score("cc", "Claude Code")).toBeGreaterThan(score("cc", "accent"));
  });

  it("prefers consecutive letters", () => {
    expect(score("han", "hangar")).toBeGreaterThan(score("han", "hxaxnx"));
  });

  it("treats camelCase humps and path separators as word starts", () => {
    expect(matchWord("on", "openNotes")?.positions).toEqual([0, 4]);
    expect(matchWord("sh", "C:\\src\\hangar")?.positions).toEqual([3, 7]);
  });

  it("keeps indices aligned for characters with a longer lowercase form", () => {
    expect(matchWord("x", "\u0130x")?.positions).toEqual([1]);
  });
});

describe("fuzzyMatch", () => {
  it("requires every word to match, anywhere in the text", () => {
    const text = "Claude Code in hangar C:\\src\\hangar";
    expect(fuzzyMatch("hangar claude", text)).not.toBeNull();
    expect(fuzzyMatch("claude hangar", text)).not.toBeNull();
    expect(fuzzyMatch("hangar codex", text)).toBeNull();
  });

  it("adds up the scores of the words", () => {
    expect(score("hangar claude", "Claude Code in hangar")).toBeGreaterThan(
      score("hangar", "Claude Code in hangar"),
    );
  });

  it("matches everything for an empty query", () => {
    expect(fuzzyMatch("  ", "anything")).toEqual({ score: 0, positions: [] });
  });

  it("ranks a prefix above a scattered match", () => {
    expect(score("pow", "PowerShell 7")).toBeGreaterThan(score("pow", "Pick other window"));
  });
});

describe("highlightRuns", () => {
  it("splits the text into plain and matched runs", () => {
    expect(highlightRuns("Claude", [0, 1, 4])).toEqual([
      { text: "Cl", match: true },
      { text: "au", match: false },
      { text: "d", match: true },
      { text: "e", match: false },
    ]);
    expect(highlightRuns("abc", [])).toEqual([{ text: "abc", match: false }]);
  });
});
