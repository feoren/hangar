import { describe, expect, it } from "vitest";

import {
  cleanTitle,
  folderLabel,
  isAutoTitle,
  moveNote,
  neighbourAfterDelete,
  parseNoteFolders,
  parseNotes,
  sameFolder,
  titleFromText,
  untitledTitle,
} from "./notes";

describe("parseNoteFolders", () => {
  it("takes valid scopes in backend order", () => {
    expect(
      parseNoteFolders([
        { folderKey: "general", path: null, noteCount: 0, updatedAt: null },
        { folderKey: "ab12", path: "C:\\p\\hangar", noteCount: 3, updatedAt: 5 },
        { folderKey: "", path: null, noteCount: 1 },
        { folderKey: "x", path: 4, noteCount: 1 },
        { folderKey: "y", path: "/y", noteCount: -1 },
      ]),
    ).toEqual([
      { folderKey: "general", path: null, noteCount: 0, updatedAt: null },
      { folderKey: "ab12", path: "C:\\p\\hangar", noteCount: 3, updatedAt: 5 },
    ]);
    expect(parseNoteFolders({})).toEqual([]);
  });
});

describe("parseNotes", () => {
  it("sorts by order and drops duplicate and broken entries", () => {
    expect(
      parseNotes([
        { id: "b", title: "Two", updatedAt: null, order: 2 },
        { id: "a", title: "One", updatedAt: 1, order: 1 },
        { id: "a", title: "Duplicate", order: 3 },
        { id: 5, title: "x" },
        "nonsense",
      ]).map((note) => note.title),
    ).toEqual(["One", "Two"]);
    expect(parseNotes(null)).toEqual([]);
  });
});

describe("scopes", () => {
  it("compare Windows paths ignoring case and separator differences", () => {
    expect(sameFolder({ path: "C:\\Repo\\Hangar\\" }, { path: "c:/repo/hangar" })).toBe(true);
    expect(sameFolder({ path: "/home/a/Repo" }, { path: "/home/a/repo" })).toBe(false);
    expect(sameFolder({ path: null }, { path: null })).toBe(true);
    expect(sameFolder({ path: null }, { path: "/" })).toBe(false);
  });

  it("are labeled with the short folder name or General", () => {
    expect(folderLabel({ path: "C:\\Users\\x\\hangar" }, "General")).toBe("x/hangar");
    expect(folderLabel({ path: null }, "General")).toBe("General");
  });
});

describe("titles", () => {
  it("takes the lowest free number", () => {
    expect(untitledTitle("Note", [])).toBe("Note 1");
    expect(untitledTitle("Note", ["note 1", "Note 3"])).toBe("Note 2");
  });

  it("validates like the backend", () => {
    expect(cleanTitle("  Ideas ")).toBe("Ideas");
    expect(cleanTitle("   ")).toBeNull();
    expect(cleanTitle("a\nb")).toBeNull();
    expect(cleanTitle("x".repeat(201))).toBeNull();
  });
});

describe("tab order", () => {
  it("moves to the target position", () => {
    expect(moveNote(["a", "b", "c"], "a", 2)).toEqual(["b", "c", "a"]);
    expect(moveNote(["a", "b", "c"], "c", 0)).toEqual(["c", "a", "b"]);
    expect(moveNote(["a", "b", "c"], "b", 9)).toEqual(["a", "c", "b"]);
    expect(moveNote(["a", "b"], "x", 0)).toEqual(["a", "b"]);
  });

  it("selects the right neighbour after deleting, otherwise the left one", () => {
    expect(neighbourAfterDelete(["a", "b", "c"], "b")).toBe("c");
    expect(neighbourAfterDelete(["a", "b", "c"], "c")).toBe("b");
    expect(neighbourAfterDelete(["a"], "a")).toBeNull();
  });
});

describe("titleFromText", () => {
  it("takes the first non-empty line without Markdown characters", () => {
    expect(titleFromText("# Shopping\n- Milk")).toBe("Shopping");
    expect(titleFromText("\n\n  ## **Plan** for `tomorrow`  \n")).toBe("Plan for tomorrow");
    expect(titleFromText("- [ ] Build release")).toBe("Build release");
    expect(titleFromText("> Quote")).toBe("Quote");
    expect(titleFromText("1. Step one")).toBe("Step one");
    expect(titleFromText("---\nText")).toBe("Text");
  });

  it("returns null without content", () => {
    expect(titleFromText("")).toBeNull();
    expect(titleFromText("   \n#\n- \n")).toBeNull();
  });

  it("shortens long lines with an ellipsis", () => {
    const title = titleFromText("a".repeat(100));
    expect(title).toBe(`${"a".repeat(59)}…`);
    expect(Array.from(title ?? "")).toHaveLength(60);
  });
});

describe("isAutoTitle", () => {
  const bases = ["Notiz", "Note"];

  it("recognizes names of new notes in every language", () => {
    expect(isAutoTitle("Notiz 1", bases, "")).toBe(true);
    expect(isAutoTitle("note 12", bases, "")).toBe(true);
    expect(isAutoTitle("Notizen", bases, "")).toBe(false);
    expect(isAutoTitle("Notiz 1a", bases, "")).toBe(false);
  });

  it("follows the text while the title is derived from it", () => {
    expect(isAutoTitle("Shopping", bases, "# Shopping\nMilk")).toBe(true);
    expect(isAutoTitle("My list", bases, "# Shopping\nMilk")).toBe(false);
    expect(isAutoTitle("Shopping", bases, "")).toBe(false);
  });
});
