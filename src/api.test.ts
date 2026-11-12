import { describe, expect, it } from "vitest";

import {
  parseExportResult,
  parseFileContent,
  parseFileStamp,
  parseFolderListing,
  parseHeadDiff,
  parsePathIndex,
} from "./api";

describe("parseFolderListing", () => {
  it("reads a listing", () => {
    expect(
      parseFolderListing({
        root: "C:\\Projects\\app",
        repoRoot: "C:\\Projects\\app",
        entries: [
          { name: "src", kind: "dir", ignored: false },
          { name: "debug.log", kind: "file", ignored: true },
          { name: "shared", kind: "symlink", ignored: false },
        ],
        truncated: false,
      }),
    ).toEqual({
      root: "C:\\Projects\\app",
      repoRoot: "C:\\Projects\\app",
      entries: [
        { name: "src", kind: "dir", ignored: false },
        { name: "debug.log", kind: "file", ignored: true },
        { name: "shared", kind: "symlink", ignored: false },
      ],
      truncated: false,
    });
  });

  it("treats a missing repository and flags as absent", () => {
    expect(parseFolderListing({ root: "/home/ada/app", repoRoot: null, entries: [] })).toEqual({
      root: "/home/ada/app",
      repoRoot: null,
      entries: [],
      truncated: false,
    });
    expect(
      parseFolderListing({ root: "/a", entries: [{ name: "x", kind: "file" }], truncated: true }),
    ).toEqual({
      root: "/a",
      repoRoot: null,
      entries: [{ name: "x", kind: "file", ignored: false }],
      truncated: true,
    });
  });

  it("drops malformed entries", () => {
    const listing = parseFolderListing({
      root: "/a",
      repoRoot: "/a",
      entries: [
        { name: "ok", kind: "dir" },
        { name: "", kind: "file" },
        { name: "a/b", kind: "file" },
        { name: "x", kind: "socket" },
        { kind: "file" },
        null,
        "name",
      ],
      truncated: false,
    });
    expect(listing?.entries).toEqual([{ name: "ok", kind: "dir", ignored: false }]);
  });

  it("rejects anything that is not a listing", () => {
    expect(parseFolderListing(null)).toBeNull();
    expect(parseFolderListing([])).toBeNull();
    expect(parseFolderListing("listing")).toBeNull();
    expect(parseFolderListing({ root: "", entries: [] })).toBeNull();
    expect(parseFolderListing({ root: "/a", entries: {} })).toBeNull();
    expect(parseFolderListing({ entries: [] })).toBeNull();
  });
});

describe("parseExportResult", () => {
  it("returns the saved path", () => {
    expect(parseExportResult("C:\\Users\\Ada\\log.txt")).toBe("C:\\Users\\Ada\\log.txt");
  });

  it("treats null as cancelled", () => {
    expect(parseExportResult(null)).toBeNull();
    expect(parseExportResult(undefined)).toBeNull();
  });

  it("rejects anything else", () => {
    expect(() => parseExportResult(42)).toThrow();
    expect(() => parseExportResult("")).toThrow();
    expect(() => parseExportResult({ path: "x" })).toThrow();
  });
});

describe("parseFileContent", () => {
  const valid = {
    name: "main.ts",
    path: "C:\\work\\app\\src\\main.ts",
    relativePath: "src/main.ts",
    size: 2048,
    text: "const a = 1;\r\n",
    truncated: false,
    binary: false,
    modified: 1_700_000_000_000,
    image: null,
    imageTooLarge: false,
  };

  it("reads a text file and a binary file", () => {
    expect(parseFileContent(valid)).toEqual(valid);
    const binary = { ...valid, name: "logo.png", text: null, binary: true };
    expect(parseFileContent(binary)).toEqual(binary);
    expect(parseFileContent({ ...valid, truncated: true, size: 9_000_000 })?.truncated).toBe(true);
  });

  it("reads images and leaves out a missing stamp", () => {
    const png = {
      ...valid,
      name: "a.png",
      text: null,
      binary: true,
      image: { mime: "image/png", data: "iVBORw0KGgo=" },
    };
    expect(parseFileContent(png)?.image).toEqual({ mime: "image/png", data: "iVBORw0KGgo=" });
    expect(parseFileContent({ ...valid, modified: null })?.modified).toBeNull();
    expect(parseFileContent({ ...valid, imageTooLarge: true })?.imageTooLarge).toBe(true);
    // An image that is not one of the five types, or data that is not Base64, is refused whole.
    expect(parseFileContent({ ...png, image: { mime: "text/html", data: "AA==" } })).toBeNull();
    expect(parseFileContent({ ...png, image: { mime: "image/png", data: "<svg>" } })).toBeNull();
  });

  it("refuses malformed answers", () => {
    for (const wrong of [
      null,
      "text",
      [valid],
      { ...valid, name: "" },
      { ...valid, name: 3 },
      { ...valid, path: null },
      { ...valid, relativePath: undefined },
      { ...valid, size: -1 },
      { ...valid, size: 1.5 },
      { ...valid, size: "12" },
      { ...valid, text: 12 },
      { ...valid, truncated: "no" },
      { ...valid, binary: undefined },
      // Text and the binary flag must agree.
      { ...valid, binary: true },
      { ...valid, text: null },
    ]) {
      expect(parseFileContent(wrong)).toBeNull();
    }
  });
});

describe("parsePathIndex", () => {
  it("reads paths and leaves out malformed ones", () => {
    const index = parsePathIndex({
      entries: [
        { path: "src", kind: "dir" },
        { path: "src/a b.ts", kind: "file" },
        { path: "/etc", kind: "file" },
        { path: "x/", kind: "dir" },
        { path: "", kind: "file" },
        { path: "y", kind: "pipe" },
      ],
      truncated: true,
    });
    expect(index).toEqual({
      entries: [
        { path: "src", kind: "dir" },
        { path: "src/a b.ts", kind: "file" },
      ],
      truncated: true,
    });
    expect(parsePathIndex({ entries: "x" })).toBeNull();
    expect(parsePathIndex(null)).toBeNull();
  });
});

describe("parseFileStamp and parseHeadDiff", () => {
  it("reads stamps", () => {
    expect(parseFileStamp({ modified: 5, size: 3 })).toEqual({ modified: 5, size: 3 });
    expect(parseFileStamp({ modified: null, size: 0 })).toEqual({ modified: null, size: 0 });
    expect(parseFileStamp({ modified: "5", size: 3 })).toBeNull();
    expect(parseFileStamp({ modified: 5, size: -1 })).toBeNull();
  });

  it("reads diffs", () => {
    expect(parseHeadDiff({ state: "changed", diff: "@@ x", truncated: false })).toEqual({
      state: "changed",
      diff: "@@ x",
      truncated: false,
    });
    expect(parseHeadDiff({ state: "notInHead", diff: "" })?.state).toBe("notInHead");
    expect(parseHeadDiff({ state: "other", diff: "" })).toBeNull();
    expect(parseHeadDiff({ state: "same" })).toBeNull();
  });
});
