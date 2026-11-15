import { afterEach, describe, expect, it, vi } from "vitest";

import {
  EMPTY_FOLDERS,
  folderKey,
  forgetFolder,
  folderName,
  isFavorite,
  FOLDERS_STORAGE_KEY,
  LAST_CWD_STORAGE_KEY,
  legacyFolderPrefs,
  moveFavorite,
  parseFolderPrefs,
  RECENT_LIMIT,
  recentOnly,
  recordRecent,
  samePath,
  toggleFavorite,
} from "./folders";

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
    };
    expect(forgetFolder(prefs, String.raw`C:\work\A`)).toEqual({
      favorites: ["C:/b"],
      recent: ["D:/c"],
    });
    expect(forgetFolder(prefs, "E:/none")).toEqual(prefs);
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

describe("parseFolderPrefs", () => {
  it("drops invalid entries, duplicates and excess", () => {
    const parsed = parseFolderPrefs({
      favorites: ["/a", 3, "", "/A", "/a/", "bad\u0007path", null],
      recent: Array.from({ length: 20 }, (_, index) => `/r/${String(index)}`),
    });
    expect(parsed.favorites).toEqual(["/a", "/A"]);
    expect(parsed.recent).toHaveLength(RECENT_LIMIT);
  });

  it("ignores the folder colors earlier versions stored", () => {
    for (const colors of [{ "C:\\Work": "green" }, null, "red", ["red"], 5]) {
      expect(parseFolderPrefs({ favorites: ["/a"], recent: ["/b"], colors })).toEqual({
        favorites: ["/a"],
        recent: ["/b"],
      });
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

  it("reads the lists an older version stored, once, for the migration", () => {
    const prefs = toggleFavorite(recordRecent(EMPTY_FOLDERS, "/r"), "/f");
    const store = new Map([[FOLDERS_STORAGE_KEY, JSON.stringify(prefs)]]);
    const storage = { getItem: (key: string) => store.get(key) ?? null };
    expect(FOLDERS_STORAGE_KEY).toBe("hangar.folders");
    expect(legacyFolderPrefs(storage)).toEqual(prefs);
  });

  it("drops stored folder colors", () => {
    const text = JSON.stringify({ favorites: ["/f"], recent: [], colors: { "/f": "red" } });
    const storage = { getItem: (key: string) => (key === FOLDERS_STORAGE_KEY ? text : null) };
    expect(legacyFolderPrefs(storage)).toEqual({ favorites: ["/f"], recent: [] });
  });

  it("takes the start folder of still older versions as the first recent folder", () => {
    const store = new Map([[LAST_CWD_STORAGE_KEY, "C:\\Work"]]);
    const storage = { getItem: (key: string) => store.get(key) ?? null };
    expect(LAST_CWD_STORAGE_KEY).toBe("hangar.lastCwd");
    expect(legacyFolderPrefs(storage)).toEqual({ favorites: [], recent: ["C:\\Work"] });
    // A recent list wins; an invalid old folder is left out.
    store.set(FOLDERS_STORAGE_KEY, JSON.stringify({ favorites: [], recent: ["/r"] }));
    expect(legacyFolderPrefs(storage).recent).toEqual(["/r"]);
    store.delete(FOLDERS_STORAGE_KEY);
    store.set(LAST_CWD_STORAGE_KEY, "a\nb");
    expect(legacyFolderPrefs(storage)).toEqual(EMPTY_FOLDERS);
  });

  it("falls back to empty lists when storage fails or holds garbage", () => {
    const blocked = {
      getItem: (): string | null => {
        throw new Error("blocked");
      },
    };
    expect(legacyFolderPrefs(blocked)).toEqual(EMPTY_FOLDERS);
    const broken = { getItem: (key: string) => (key === FOLDERS_STORAGE_KEY ? "{broken" : null) };
    expect(legacyFolderPrefs(broken)).toEqual(EMPTY_FOLDERS);
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
