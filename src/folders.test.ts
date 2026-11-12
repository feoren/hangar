/// <reference types="node" />
import { readFileSync } from "node:fs";

import { afterEach, describe, expect, it, vi } from "vitest";

import {
  EMPTY_FOLDERS,
  folderColor,
  folderColorCss,
  folderKey,
  forgetFolder,
  folderName,
  isFavorite,
  loadFolderPrefs,
  moveFavorite,
  parseFolderPrefs,
  RECENT_LIMIT,
  recentOnly,
  recordRecent,
  samePath,
  saveFolderPrefs,
  setFolderColor,
  toggleFavorite,
} from "./folders";
import { MARKER_COLORS } from "./marker";

describe("folderKey", () => {
  it("ignores case and trailing separators for Windows paths", () => {
    expect(folderKey("C:\\Users\\Ada\\")).toBe("c:\\users\\ada");
    expect(folderKey("c:/users/ada")).toBe("c:\\users\\ada");
    expect(samePath("C:\\Projekte\\Hangar", "c:\\projekte\\hangar\\\\")).toBe(true);
    expect(folderKey("\\\\Server\\Share\\")).toBe("\\\\server\\share");
  });

  it("keeps roots as roots", () => {
    expect(folderKey("C:\\")).toBe("c:\\");
    expect(folderKey("C:")).toBe("c:\\");
    expect(folderKey("/")).toBe("/");
  });

  it("keeps case for POSIX paths", () => {
    expect(folderKey("/home/ada/")).toBe("/home/ada");
    expect(samePath("/home/Ada", "/home/ada")).toBe(false);
  });
});

describe("recent folders", () => {
  it("puts the newest first without duplicates", () => {
    let prefs = recordRecent(EMPTY_FOLDERS, "C:\\a");
    prefs = recordRecent(prefs, "C:\\b");
    prefs = recordRecent(prefs, "c:\\A\\");
    expect(prefs.recent).toEqual(["c:\\A\\", "C:\\b"]);
  });

  it("keeps at most the limit", () => {
    let prefs = EMPTY_FOLDERS;
    for (let index = 0; index < RECENT_LIMIT + 3; index += 1) {
      prefs = recordRecent(prefs, `/p/${String(index)}`);
    }
    expect(prefs.recent).toHaveLength(RECENT_LIMIT);
    expect(prefs.recent[0]).toBe(`/p/${String(RECENT_LIMIT + 2)}`);
  });

  it("returns the same object when nothing changes", () => {
    const prefs = recordRecent(EMPTY_FOLDERS, "/x");
    expect(recordRecent(prefs, "/x")).toBe(prefs);
    expect(recordRecent(prefs, "  ")).toBe(prefs);
  });
});

describe("favorites", () => {
  it("adds and removes regardless of spelling", () => {
    const added = toggleFavorite(EMPTY_FOLDERS, "C:\\Work");
    expect(isFavorite(added, "c:\\work\\")).toBe(true);
    const removed = toggleFavorite(added, "c:/work");
    expect(removed.favorites).toEqual([]);
  });

  it("leaves favorites out of the recent part of the menu", () => {
    let prefs = recordRecent(EMPTY_FOLDERS, "/a");
    prefs = recordRecent(prefs, "/b");
    prefs = toggleFavorite(prefs, "/a");
    expect(recentOnly(prefs)).toEqual(["/b"]);
  });
});

describe("forgetFolder", () => {
  it("takes a folder off both lists, matching Windows paths without case", () => {
    const prefs = {
      favorites: ["C:/Work/a", "C:/b"],
      recent: ["c:/work/a/", "D:/c"],
      colors: {},
    };
    expect(forgetFolder(prefs, String.raw`C:\work\A`)).toEqual({
      favorites: ["C:/b"],
      recent: ["D:/c"],
      colors: {},
    });
    expect(forgetFolder(prefs, "E:/none")).toEqual(prefs);
  });

  it("drops the folder's color, others keep theirs", () => {
    let prefs = toggleFavorite(EMPTY_FOLDERS, "C:\\Work");
    prefs = setFolderColor(prefs, "C:\\Work", "green");
    prefs = setFolderColor(prefs, "/other", "blue");
    const forgotten = forgetFolder(prefs, "c:/work/");
    expect(folderColor(forgotten, "C:\\Work")).toBeNull();
    expect(folderColor(forgotten, "/other")).toBe("blue");
  });
});

describe("moveFavorite", () => {
  const prefs = { ...EMPTY_FOLDERS, favorites: ["/a", "/b", "/c", "/d"] };

  it("places a favorite before or after another one", () => {
    expect(moveFavorite(prefs, "/d", "/b", false).favorites).toEqual(["/a", "/d", "/b", "/c"]);
    expect(moveFavorite(prefs, "/a", "/c", true).favorites).toEqual(["/b", "/c", "/a", "/d"]);
    // One step down: after the next one; one step up: before the previous one.
    expect(moveFavorite(prefs, "/b", "/c", true).favorites).toEqual(["/a", "/c", "/b", "/d"]);
    expect(moveFavorite(prefs, "/c", "/b", false).favorites).toEqual(["/a", "/c", "/b", "/d"]);
  });

  it("matches Windows paths without case", () => {
    const windows = { ...EMPTY_FOLDERS, favorites: ["C:\\One", "C:\\Two"] };
    expect(moveFavorite(windows, "c:/two/", "c:\\one", false).favorites).toEqual([
      "C:\\Two",
      "C:\\One",
    ]);
  });

  it("returns the same prefs when nothing moves", () => {
    expect(moveFavorite(prefs, "/b", "/c", false)).toBe(prefs);
    expect(moveFavorite(prefs, "/b", "/b", true)).toBe(prefs);
    expect(moveFavorite(prefs, "/x", "/b", true)).toBe(prefs);
    expect(moveFavorite(prefs, "/b", "/x", true)).toBe(prefs);
  });
});

describe("folder colors", () => {
  it("sets, changes and clears a color, regardless of spelling", () => {
    const colored = setFolderColor(EMPTY_FOLDERS, "C:\\Work", "red");
    expect(folderColor(colored, "c:/work/")).toBe("red");
    const changed = setFolderColor(colored, "c:\\work", "teal");
    expect(changed.colors).toEqual({ "c:\\work": "teal" });
    expect(folderColor(setFolderColor(changed, "C:\\Work", null), "C:\\Work")).toBeNull();
  });

  it("returns the same prefs when nothing changes", () => {
    const colored = setFolderColor(EMPTY_FOLDERS, "/a", "red");
    expect(setFolderColor(colored, "/a", "red")).toBe(colored);
    expect(setFolderColor(EMPTY_FOLDERS, "/a", null)).toBe(EMPTY_FOLDERS);
    expect(setFolderColor(EMPTY_FOLDERS, " ", "red")).toBe(EMPTY_FOLDERS);
  });

  it("draws a color through its design token", () => {
    expect(folderColorCss("teal")).toBe("var(--folder-teal)");
  });

  it("has a design token per palette color with the marker's value", () => {
    const styles = readFileSync(new URL("./styles.css", import.meta.url), "utf8");
    for (const { id, color } of MARKER_COLORS) {
      expect(styles).toContain(`--folder-${id}: ${color};`);
    }
  });

  it("does not read inherited object keys as colors", () => {
    expect(folderColor(EMPTY_FOLDERS, "constructor")).toBeNull();
    expect(folderColor(EMPTY_FOLDERS, "__proto__")).toBeNull();
  });
});

describe("parseFolderPrefs", () => {
  it("drops invalid entries, duplicates and excess", () => {
    const parsed = parseFolderPrefs({
      favorites: ["/a", 3, "", "/A", "/a/", "bad\u0007path", null],
      recent: Array.from({ length: 20 }, (_, index) => `/r/${String(index)}`),
    });
    expect(parsed.favorites).toEqual(["/a", "/A"]);
    expect(parsed.recent).toHaveLength(RECENT_LIMIT);
  });

  it("reads data from before folder colors with no colors", () => {
    expect(parseFolderPrefs({ favorites: ["/a"], recent: ["/b"] })).toEqual({
      favorites: ["/a"],
      recent: ["/b"],
      colors: {},
    });
  });

  it("keeps known colors on valid folders and drops the rest", () => {
    const parsed = parseFolderPrefs({
      favorites: [],
      recent: [],
      colors: {
        "C:\\Work\\": "green",
        "/a": "chartreuse",
        "/b": 3,
        "": "red",
        "bad\u0007path": "red",
      },
    });
    expect(parsed.colors).toEqual({ "c:\\work": "green" });
    for (const colors of [null, "red", ["red"], 5]) {
      expect(parseFolderPrefs({ colors }).colors).toEqual({});
    }
  });

  it("survives wrong roots", () => {
    for (const value of [null, 3, "x", [], { favorites: "no" }]) {
      expect(parseFolderPrefs(value)).toEqual(EMPTY_FOLDERS);
    }
  });
});

describe("storage", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("round-trips through localStorage", () => {
    const store = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => {
        store.set(key, value);
      },
    });
    const prefs = setFolderColor(
      toggleFavorite(recordRecent(EMPTY_FOLDERS, "/r"), "/f"),
      "/f",
      "pink",
    );
    saveFolderPrefs(prefs);
    expect(loadFolderPrefs()).toEqual(prefs);
  });

  it("falls back to empty lists when storage fails or holds garbage", () => {
    vi.stubGlobal("localStorage", {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
    });
    expect(loadFolderPrefs()).toEqual(EMPTY_FOLDERS);
    expect(() => {
      saveFolderPrefs(EMPTY_FOLDERS);
    }).not.toThrow();
    vi.stubGlobal("localStorage", { getItem: () => "{broken" });
    expect(loadFolderPrefs()).toEqual(EMPTY_FOLDERS);
  });
});

describe("folderName", () => {
  it("shows the last folder, roots unchanged", () => {
    expect(folderName("C:\\Users\\Ada\\Hangar")).toBe("Hangar");
    expect(folderName("/home/ada/")).toBe("ada");
    expect(folderName("C:\\")).toBe("C:\\");
    expect(folderName("/")).toBe("/");
  });
});
