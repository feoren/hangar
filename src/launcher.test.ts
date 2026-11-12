import { describe, expect, it } from "vitest";

import {
  firstSelectable,
  flatEntries,
  LAUNCHER_LIMIT,
  MATCH_LIMIT,
  moveSelection,
  rankEntries,
  SECTION_LIMIT,
  type LauncherEntry,
} from "./launcher";

const entries: LauncherEntry[] = [
  { key: "s1", section: "sessions", label: "Claude Code", detail: "~\\src\\hangar", primary: true },
  { key: "s2", section: "sessions", label: "PowerShell 7", detail: "~", primary: true },
  {
    key: "p1",
    section: "start",
    label: "Claude Code in hangar",
    detail: "C:\\src\\hangar",
  },
  { key: "p2", section: "start", label: "Codex in hangar", detail: "C:\\src\\hangar" },
  { key: "p3", section: "start", label: "Claude Code in notes", detail: "C:\\notes" },
  { key: "n1", section: "notes", label: "Notes: hangar", detail: "C:\\src\\hangar" },
  { key: "x1", section: "snippets", label: "Write tests", primary: true },
  { key: "c1", section: "commands", label: "View: grid of four", primary: true },
];

describe("rankEntries", () => {
  it("shows the primary entries in their order without a query", () => {
    const sections = rankEntries(entries, "");
    expect(sections.map((section) => section.section)).toEqual([
      "sessions",
      "snippets",
      "commands",
    ]);
    expect(flatEntries(sections).map((entry) => entry.key)).toEqual(["s1", "s2", "x1", "c1"]);
  });

  it("finds program and folder with two words", () => {
    const keys = flatEntries(rankEntries(entries, "hangar claude")).map((entry) => entry.key);
    expect(keys[0]).toBe("p1");
    expect(keys).toContain("s1");
    expect(keys).not.toContain("p2");
    expect(keys).not.toContain("p3");
  });

  it("groups by section in the order of each section's best entry", () => {
    const sections = rankEntries(entries, "hangar");
    expect(sections.map((section) => section.section)[0]).toBe("notes");
    expect(new Set(sections.map((section) => section.section)).size).toBe(sections.length);
  });

  it("highlights only characters of the label", () => {
    const sections = rankEntries(entries, "pow");
    expect(sections[0]?.items[0]?.positions).toEqual([0, 1, 2]);
    const viaDetail = rankEntries(entries, "notes").flatMap((section) => section.items);
    for (const item of viaDetail) {
      expect(item.positions.every((position) => position < item.entry.label.length)).toBe(true);
    }
  });

  it("searches hidden keywords", () => {
    const withKeywords: LauncherEntry[] = [
      { key: "a", section: "sessions", label: "Review", keywords: "Claude Code" },
    ];
    expect(flatEntries(rankEntries(withKeywords, "claude")).map((entry) => entry.key)).toEqual([
      "a",
    ]);
  });

  it("shows at most the idle limit without a query", () => {
    const many: LauncherEntry[] = Array.from({ length: 20 }, (_, index) => ({
      key: String(index),
      section: "sessions",
      label: `Session ${String(index)}`,
      primary: true,
    }));
    expect(flatEntries(rankEntries(many, ""))).toHaveLength(LAUNCHER_LIMIT);
    expect(flatEntries(rankEntries(many, "", { idle: 3 }))).toHaveLength(3);
  });

  it("caps each section while searching, so one section cannot push out the others", () => {
    const files: LauncherEntry[] = Array.from({ length: 2000 }, (_, index) => ({
      key: `f${String(index)}`,
      section: "files",
      label: `hangar${String(index)}.ts`,
    }));
    const sections = rankEntries([...files, ...entries], "hangar");
    const fileSection = sections.find((section) => section.section === "files");
    expect(fileSection?.items).toHaveLength(SECTION_LIMIT);
    expect(sections.some((section) => section.section === "start")).toBe(true);
    expect(sections.some((section) => section.section === "notes")).toBe(true);
    const total = flatEntries(sections).length;
    expect(total).toBeLessThanOrEqual(MATCH_LIMIT);
  });

  it("caps the whole list while searching", () => {
    const sectionsOf: LauncherEntry["section"][] = ["start", "files", "commits", "commands"];
    const many: LauncherEntry[] = sectionsOf.flatMap((section) =>
      Array.from({ length: 10 }, (_, index) => ({
        key: `${section}${String(index)}`,
        section,
        label: `Program ${String(index)}`,
      })),
    );
    expect(flatEntries(rankEntries(many, "program", { perSection: 4, total: 10 }))).toHaveLength(
      10,
    );
  });

  it("returns nothing when no entry matches", () => {
    expect(rankEntries(entries, "zzz")).toEqual([]);
  });
});

describe("selection", () => {
  const list = [{}, { disabled: true }, {}, { disabled: true }];

  it("moves with wrap around and skips disabled entries", () => {
    expect(moveSelection(list, 0, 1)).toBe(2);
    expect(moveSelection(list, 2, 1)).toBe(0);
    expect(moveSelection(list, 0, -1)).toBe(2);
  });

  it("finds the first selectable entry", () => {
    expect(firstSelectable(list)).toBe(0);
    expect(firstSelectable([{ disabled: true }, {}])).toBe(1);
    expect(firstSelectable([{ disabled: true }])).toBe(-1);
    expect(moveSelection([{ disabled: true }], 0, 1)).toBe(-1);
    expect(moveSelection([], -1, 1)).toBe(-1);
  });
});
