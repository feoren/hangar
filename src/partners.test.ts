import { describe, expect, it } from "vitest";

import {
  EMPTY_BOOK,
  namedAt,
  parsePartnerBook,
  partnersOf,
  projectOfSession,
  suggestName,
  validName,
} from "./partners";

const book = parsePartnerBook({
  projects: [
    { name: "app", folder: "C:\\work\\app" },
    { name: "web", folder: "C:\\work\\web" },
    { name: "relay", folder: "/srv/relay" },
  ],
  pairs: [
    ["app", "web"],
    ["relay", "web"],
  ],
});

describe("partner projects", () => {
  it("accepts only short names that need no quoting", () => {
    expect(validName("web-2")).toBe(true);
    for (const bad of ["", "Web", "web site", "web_site", "wéb", "x".repeat(33), 'a"b']) {
      expect(validName(bad)).toBe(false);
    }
  });

  it("suggests a short name from the folder", () => {
    expect(suggestName("C:\\work\\Hangar Team Server")).toBe("hangar-team-server");
    expect(suggestName("/home/me/hangar.feoren.com/")).toBe("hangar-feoren-com");
    expect(suggestName("/home/me/Übersicht")).toBe("ubersicht");
    expect(suggestName("/home/me/日本")).toBe("");
    expect(suggestName(`/x/${"a".repeat(31)} b`)).toBe("a".repeat(31));
  });

  it("reads a pair in both directions", () => {
    expect(partnersOf(book, "web").map((p) => p.name)).toEqual(["app", "relay"]);
    expect(partnersOf(book, "app").map((p) => p.name)).toEqual(["web"]);
    expect(partnersOf(book, "relay").map((p) => p.name)).toEqual(["web"]);
    expect(partnersOf(book, "nobody")).toEqual([]);
  });

  it("finds a session's project by folder, subfolder or worktree", () => {
    expect(projectOfSession(book, { cwd: "c:\\WORK\\app\\src" })?.name).toBe("app");
    expect(
      projectOfSession(book, {
        cwd: "D:\\trees\\w1",
        worktree: { repoRoot: "C:\\work\\web" },
      })?.name,
    ).toBe("web");
    expect(projectOfSession(book, { cwd: "C:\\work\\app-old" })).toBeUndefined();
    expect(projectOfSession(book, { cwd: "/srv/relay/src" })?.name).toBe("relay");
    expect(namedAt(book, "C:\\Work\\Web\\")?.name).toBe("web");
  });

  it("reads anything malformed as no partners", () => {
    for (const bad of [
      null,
      "x",
      { projects: [], pairs: {} },
      { projects: [{ name: "Bad", folder: "/a" }], pairs: [] },
      { projects: [{ name: "a", folder: "/a" }], pairs: [["a", "b"]] },
      { projects: [{ name: "a", folder: "/a" }], pairs: [["a", "a"]] },
      { projects: [{ name: "a", folder: "/a\u202e" }], pairs: [] },
    ]) {
      expect(parsePartnerBook(bad)).toEqual(EMPTY_BOOK);
    }
  });
});
