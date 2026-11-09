import { describe, expect, it } from "vitest";

import {
  cleanTitle,
  byLastChange,
  isUntitled,
  noteKey,
  parseNoteFolders,
  parseNotes,
  parseSearchHits,
  parseTrash,
  sameFolder,
  sameNote,
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

describe("isUntitled", () => {
  const bases = ["Notiz", "Note"];

  it("recognizes names of new notes in every language", () => {
    expect(isUntitled("Notiz 1", bases)).toBe(true);
    expect(isUntitled("note 12", bases)).toBe(true);
    expect(isUntitled("Notizen", bases)).toBe(false);
    expect(isUntitled("Notiz 1a", bases)).toBe(false);
    expect(isUntitled("Shopping", bases)).toBe(false);
  });
});

describe("lists", () => {
  it("sort by last change, notes without a time last", () => {
    const notes = [
      { title: "b", updatedAt: 1 },
      { title: "a", updatedAt: null },
      { title: "c", updatedAt: 9 },
      { title: "d", updatedAt: 1 },
    ];
    expect(byLastChange(notes).map((n) => n.title)).toEqual(["c", "b", "d", "a"]);
  });

  it("tell notes of different scopes apart", () => {
    const a = { folder: { path: "C:\\p" }, id: "1" };
    expect(sameNote(a, { folder: { path: "c:/p" }, id: "1" })).toBe(true);
    expect(sameNote(a, { folder: { path: null }, id: "1" })).toBe(false);
    expect(noteKey(a)).not.toBe(noteKey({ folder: { path: null }, id: "1" }));
  });
});

describe("parseSearchHits", () => {
  it("keeps valid hits with their scope", () => {
    expect(
      parseSearchHits([
        { folderKey: "general", path: null, id: "1", title: "A", excerpt: "x", updatedAt: 3 },
        { folderKey: "k", path: "/p", id: "2", title: "B" },
        { folderKey: "", path: null, id: "3", title: "C" },
        { folderKey: "k", path: 1, id: "4", title: "D" },
        "nonsense",
      ]),
    ).toEqual([
      {
        folderKey: "general",
        folder: { path: null },
        id: "1",
        title: "A",
        excerpt: "x",
        updatedAt: 3,
      },
      { folderKey: "k", folder: { path: "/p" }, id: "2", title: "B", excerpt: "", updatedAt: null },
    ]);
    expect(parseSearchHits(null)).toEqual([]);
  });
});

describe("parseTrash", () => {
  it("keeps entries with ID and title", () => {
    expect(
      parseTrash([{ id: "1", title: "Gone", deletedAt: 5 }, { id: "", title: "x" }, { id: "2" }]),
    ).toEqual([{ id: "1", title: "Gone", deletedAt: 5 }]);
    expect(parseTrash({})).toEqual([]);
  });
});
