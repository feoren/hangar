import { describe, expect, it } from "vitest";

import {
  distinctNames,
  elideMiddle,
  ELLIPSIS,
  folderMenuSections,
  parentFolder,
} from "./folderSections";

describe("elideMiddle", () => {
  it("keeps short paths", () => {
    expect(elideMiddle("C:\\src\\hangar", 38)).toBe("C:\\src\\hangar");
  });

  it("keeps drive, first folder and the end", () => {
    expect(elideMiddle("C:\\Users\\ada\\Dropbox\\Repository\\Local\\hangar", 30)).toBe(
      `C:\\Users\\${ELLIPSIS}\\Local\\hangar`,
    );
  });

  it("works for POSIX and UNC paths", () => {
    expect(elideMiddle("/home/ada/projects/clients/acme/website", 26)).toBe(
      `/home/${ELLIPSIS}/acme/website`,
    );
    expect(elideMiddle("\\\\server\\share\\teams\\alpha\\repo", 24)).toBe(
      `\\\\server\\${ELLIPSIS}\\alpha\\repo`,
    );
  });

  it("always keeps the last folder", () => {
    const long = "a-very-long-project-folder-name-that-does-not-fit";
    expect(elideMiddle(`C:\\Users\\ada\\x\\${long}`, 20)).toBe(`C:\\Users\\${ELLIPSIS}\\${long}`);
  });
});

describe("parentFolder", () => {
  it("returns the containing folder", () => {
    expect(parentFolder("C:\\src\\hangar")).toBe("C:\\src");
    expect(parentFolder("C:\\src")).toBe("C:\\");
    expect(parentFolder("/home/ada")).toBe("/home");
    expect(parentFolder("/home")).toBeNull();
    expect(parentFolder("C:\\")).toBeNull();
  });
});

describe("distinctNames", () => {
  it("extends same names with their parents", () => {
    expect(
      distinctNames(["C:\\work\\hangar", "D:\\fork\\Hangar", "C:\\src\\notes", "/srv/x/notes2"]),
    ).toEqual(["work\\hangar", "fork\\Hangar", "notes", "notes2"]);
  });

  it("goes deeper until the names differ", () => {
    expect(distinctNames(["C:\\a\\x\\app", "C:\\b\\x\\app"])).toEqual(["a\\x\\app", "b\\x\\app"]);
  });

  it("keeps roots readable", () => {
    expect(distinctNames(["C:\\"])).toEqual(["C:"]);
  });
});

describe("folderMenuSections", () => {
  it("builds running, favorites and recent without duplicates", () => {
    const sections = folderMenuSections({
      running: ["C:\\src\\hangar"],
      prefs: {
        favorites: ["c:\\src\\HANGAR", "D:\\fork\\hangar", "C:\\docs"],
        recent: ["C:\\docs", "E:\\tmp"],
      },
      target: "C:\\src\\hangar",
    });
    expect(sections.map((s) => s.kind)).toEqual(["running", "favorites", "recent"]);
    expect(sections[0]?.entries).toEqual([
      {
        path: "C:\\src\\hangar",
        label: "src\\hangar",
        detail: "C:\\src",
        favorite: true,
        current: true,
      },
    ]);
    expect(sections[1]?.entries.map((e) => e.label)).toEqual(["fork\\hangar", "docs"]);
    expect(sections[1]?.entries.every((e) => e.favorite && !e.current)).toBe(true);
    expect(sections[2]?.entries.map((e) => [e.path, e.favorite])).toEqual([["E:\\tmp", false]]);
  });

  it("leaves out empty sections", () => {
    const sections = folderMenuSections({
      running: [],
      prefs: { favorites: [], recent: ["C:\\a"] },
      target: null,
    });
    expect(sections.map((s) => s.kind)).toEqual(["recent"]);
  });
});
