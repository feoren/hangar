import { describe, expect, it } from "vitest";

import {
  defaultSnippets,
  moveSnippet,
  newSnippetId,
  parseSnippets,
  SNIPPET_LIMIT,
  SNIPPET_MAX_LENGTH,
  snippetLabel,
  snippetTextFromSelection,
  snippetWithText,
  usableSnippets,
} from "./snippets";

describe("defaultSnippets", () => {
  it("come in the UI language", () => {
    const en = defaultSnippets("en").map((snippet) => snippet.text);
    const de = defaultSnippets("de").map((snippet) => snippet.text);
    expect(en).toHaveLength(3);
    expect(de).toHaveLength(3);
    expect(en).not.toEqual(de);
    expect(new Set(defaultSnippets("en").map((snippet) => snippet.id)).size).toBe(3);
  });
});

describe("parseSnippets", () => {
  it("seeds the defaults when nothing was stored", () => {
    expect(parseSnippets(undefined, "en")).toEqual(defaultSnippets("en"));
    expect(parseSnippets("nonsense", "en")).toEqual(defaultSnippets("en"));
  });

  it("keeps an empty list: the user deleted them", () => {
    expect(parseSnippets([], "en")).toEqual([]);
  });

  it("drops invalid entries and repairs IDs", () => {
    expect(
      parseSnippets(
        [
          { id: "a", text: "First" },
          { id: "a", text: "Duplicate ID" },
          { id: "Bad ID!", text: "Bad ID" },
          { text: "   " },
          { id: "b" },
          null,
          "text",
        ],
        "en",
      ),
    ).toEqual([
      { id: "a", text: "First" },
      { id: "snippet-2", text: "Duplicate ID" },
      { id: "snippet-3", text: "Bad ID" },
    ]);
  });

  it("limits count and length", () => {
    const many = Array.from({ length: SNIPPET_LIMIT + 5 }, (_, index) => ({
      id: `s${String(index)}`,
      text: "x",
    }));
    expect(parseSnippets(many, "en")).toHaveLength(SNIPPET_LIMIT);
    const long = parseSnippets([{ id: "a", text: "y".repeat(SNIPPET_MAX_LENGTH + 10) }], "en");
    expect(long[0]?.text).toHaveLength(SNIPPET_MAX_LENGTH);
  });
});

describe("helpers", () => {
  it("label with the first non-empty line, shortened", () => {
    expect(snippetLabel({ text: "\n  Review the changes  \nmore" })).toBe("Review the changes");
    const label = snippetLabel({ text: "a".repeat(100) });
    expect(Array.from(label)).toHaveLength(60);
    expect(label.endsWith("…")).toBe(true);
    expect(snippetLabel({ text: "" })).toBe("");
  });

  it("create unused IDs", () => {
    expect(newSnippetId([])).toBe("snippet-1");
    expect(
      newSnippetId([
        { id: "snippet-2", text: "a" },
        { id: "x", text: "b" },
      ]),
    ).toBe("snippet-3");
  });

  it("skip empty snippets when inserting", () => {
    expect(
      usableSnippets([
        { id: "a", text: " " },
        { id: "b", text: "ok" },
      ]),
    ).toEqual([{ id: "b", text: "ok" }]);
  });

  it("move snippets within the edges", () => {
    const list = [
      { id: "a", text: "1" },
      { id: "b", text: "2" },
      { id: "c", text: "3" },
    ];
    expect(moveSnippet(list, "b", -1).map((s) => s.id)).toEqual(["b", "a", "c"]);
    expect(moveSnippet(list, "b", 1).map((s) => s.id)).toEqual(["a", "c", "b"]);
    expect(moveSnippet(list, "a", -1).map((s) => s.id)).toEqual(["a", "b", "c"]);
    expect(moveSnippet(list, "c", 1).map((s) => s.id)).toEqual(["a", "b", "c"]);
    expect(moveSnippet(list, "zz", 1).map((s) => s.id)).toEqual(["a", "b", "c"]);
  });
});

describe("snippetTextFromSelection", () => {
  it("normalizes line ends and drops blanks padding the rows", () => {
    expect(snippetTextFromSelection("first   \r\nsecond\t\rthird ")).toBe("first\nsecond\nthird");
  });

  it("drops empty rows around the selection but keeps inner ones", () => {
    expect(snippetTextFromSelection("\n   \nfirst\n\nsecond\n  \n")).toBe("first\n\nsecond");
  });

  it("removes the shared indentation, keeps the relative one", () => {
    expect(snippetTextFromSelection("    if (ok) {\n      run();\n\n    }")).toBe(
      "if (ok) {\n  run();\n\n}",
    );
  });

  it("keeps a single line as it is, apart from the blanks around it", () => {
    expect(snippetTextFromSelection("  git status  ")).toBe("git status");
  });

  it("gives nothing for a blank selection", () => {
    expect(snippetTextFromSelection(" \r\n \n")).toBe("");
  });

  it("cuts to the stored length without splitting a character", () => {
    expect(snippetTextFromSelection("y".repeat(SNIPPET_MAX_LENGTH + 10))).toHaveLength(
      SNIPPET_MAX_LENGTH,
    );
    const emoji = `${"y".repeat(SNIPPET_MAX_LENGTH - 1)}\u{1F680}`;
    expect(snippetTextFromSelection(emoji)).toBe("y".repeat(SNIPPET_MAX_LENGTH - 1));
  });

  it("gives the name from the first non-empty line, shortened", () => {
    const text = snippetTextFromSelection(`\n\n   ${"Explain ".repeat(20)}  \n   next line`);
    const label = snippetLabel({ text });
    expect(label.startsWith("Explain Explain")).toBe(true);
    expect(Array.from(label)).toHaveLength(60);
    expect(label.endsWith("…")).toBe(true);
  });
});

describe("snippetWithText", () => {
  const list = [
    { id: "a", text: "Review the changes" },
    { id: "b", text: "Write tests\nfor this" },
  ];

  it("finds a snippet with the same text after cleanup", () => {
    expect(snippetWithText(list, "  Review the changes  \r\n")?.id).toBe("a");
    expect(snippetWithText(list, "Write tests   \r\nfor this")?.id).toBe("b");
  });

  it("finds nothing for new or blank text", () => {
    expect(snippetWithText(list, "Review the change")).toBeUndefined();
    expect(snippetWithText(list, "  \n")).toBeUndefined();
  });
});
