import { describe, expect, it } from "vitest";

import { parseExportResult, parseFileContent, parseFolderListing } from "./api";

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
  };

  it("reads a text file and a binary file", () => {
    expect(parseFileContent(valid)).toEqual(valid);
    const binary = { ...valid, name: "logo.png", text: null, binary: true };
    expect(parseFileContent(binary)).toEqual(binary);
    expect(parseFileContent({ ...valid, truncated: true, size: 9_000_000 })?.truncated).toBe(true);
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
