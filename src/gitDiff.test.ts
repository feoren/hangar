import { describe, expect, it } from "vitest";

import {
  changeEntries,
  changeKind,
  autoOpenPath,
  isFileOpen,
  NO_CHOICES,
  toggleFile,
  lineKind,
  parseGitDiff,
  splitDiff,
  splitPath,
  unquotePath,
  type GitDiff,
} from "./gitDiff";

const DIFF = [
  "diff --git a/src/app.ts b/src/app.ts",
  "index 1111111..2222222 100644",
  "--- a/src/app.ts",
  "+++ b/src/app.ts",
  "@@ -1,3 +1,3 @@",
  " keep",
  "-old",
  "+new",
  "\\ No newline at end of file",
  "diff --git a/gone.txt b/gone.txt",
  "deleted file mode 100644",
  "--- a/gone.txt",
  "+++ /dev/null",
  "@@ -1 +0,0 @@",
  "-bye",
  "diff --git a/logo.png b/logo.png",
  "Binary files a/logo.png and b/logo.png differ",
  "diff --git a/old name.md b/new name.md",
  "similarity index 100%",
  "rename from old name.md",
  "rename to new name.md",
  "",
].join("\n");

describe("parseGitDiff", () => {
  it("reads the backend's answer and drops malformed files", () => {
    const parsed = parseGitDiff({
      isRepo: true,
      files: [
        { path: "a.ts", status: " M", additions: 2, deletions: 1 },
        { path: "", status: "??" },
        { path: "b.ts", status: "??", additions: -1 },
        "x",
      ],
      diff: "",
      truncated: true,
    });
    expect(parsed).toEqual({
      isRepo: true,
      files: [
        { path: "a.ts", status: " M", additions: 2, deletions: 1 },
        { path: "b.ts", status: "??", additions: 0, deletions: 0 },
      ],
      diff: "",
      truncated: true,
    });
  });

  it("refuses anything that is not the contract", () => {
    expect(parseGitDiff(null)).toBeNull();
    expect(parseGitDiff({ isRepo: "yes", files: [], diff: "" })).toBeNull();
    expect(parseGitDiff({ isRepo: true, files: {}, diff: "" })).toBeNull();
  });
});

describe("splitDiff", () => {
  it("splits per file with paths of modified, deleted, binary and renamed files", () => {
    const files = splitDiff(DIFF);
    expect(files.map((file) => [file.path, file.binary, file.lines.length])).toEqual([
      ["src/app.ts", false, 5],
      ["gone.txt", false, 2],
      ["logo.png", true, 0],
      ["new name.md", false, 0],
    ]);
    expect(files[0]?.lines).toEqual([
      "@@ -1,3 +1,3 @@",
      " keep",
      "-old",
      "+new",
      "\\ No newline at end of file",
    ]);
  });

  it("handles CRLF output and an empty diff", () => {
    expect(splitDiff("")).toEqual([]);
    const files = splitDiff(DIFF.replace(/\n/g, "\r\n"));
    expect(files[0]?.lines[2]).toBe("-old");
  });

  it("decodes quoted paths with UTF-8 octal escapes", () => {
    expect(unquotePath('"a/\\303\\244rger.txt"')).toBe("a/ärger.txt");
    expect(unquotePath('"tab\\there"')).toBe("tab\there");
    expect(unquotePath("plain.txt")).toBe("plain.txt");
    const files = splitDiff(
      [
        'diff --git "a/\\303\\244.txt" "b/\\303\\244.txt"',
        '--- "a/\\303\\244.txt"',
        '+++ "b/\\303\\244.txt"',
        "@@ -1 +1 @@",
        "-x",
        "+y",
      ].join("\n"),
    );
    expect(files[0]?.path).toBe("ä.txt");
  });

  it("splits a large diff quickly enough for the panel", () => {
    const body = Array.from({ length: 40_000 }, (_, i) => `+line ${String(i)} ${"x".repeat(40)}`);
    const big = ["diff --git a/big.txt b/big.txt", "@@ -0,0 +1,40000 @@", ...body].join("\n");
    const started = performance.now();
    const files = splitDiff(big);
    expect(files[0]?.lines).toHaveLength(40_001);
    expect(performance.now() - started).toBeLessThan(500);
  });
});

describe("lineKind", () => {
  it("classifies diff lines", () => {
    expect(lineKind("+added")).toBe("add");
    expect(lineKind("-removed")).toBe("del");
    expect(lineKind(" same")).toBe("context");
    expect(lineKind("")).toBe("context");
    expect(lineKind("@@ -1 +1 @@ fn main")).toBe("hunk");
    expect(lineKind("\\ No newline at end of file")).toBe("note");
  });
});

describe("changeKind", () => {
  it("reads the porcelain code", () => {
    expect(changeKind(" M")).toBe("modified");
    expect(changeKind("M ")).toBe("modified");
    expect(changeKind("A ")).toBe("added");
    expect(changeKind(" D")).toBe("deleted");
    expect(changeKind("R ")).toBe("renamed");
    expect(changeKind("??")).toBe("untracked");
    expect(changeKind("UU")).toBe("conflict");
    expect(changeKind("AA")).toBe("conflict");
  });
});

describe("changeEntries", () => {
  const result: GitDiff = {
    isRepo: true,
    files: [
      { path: "src/app.ts", status: " M", additions: 1, deletions: 1 },
      { path: "notes.txt", status: "??", additions: 0, deletions: 0 },
    ],
    diff: DIFF,
    truncated: false,
  };

  it("pairs the file list with the diff and keeps git's order", () => {
    const entries = changeEntries(result);
    expect(entries.map((entry) => [entry.path, entry.kind, entry.diff !== null])).toEqual([
      ["src/app.ts", "modified", true],
      ["notes.txt", "untracked", false],
      ["gone.txt", "modified", true],
      ["logo.png", "modified", true],
      ["new name.md", "modified", true],
    ]);
    // Counted from the diff when the list has no entry for a file.
    expect(entries[2]).toMatchObject({ additions: 0, deletions: 1 });
  });

  it("opens only the last file that has a diff", () => {
    // The binary file and the pure rename after it have no lines to show.
    expect(autoOpenPath(changeEntries(result))).toBe("gone.txt");
    const big: GitDiff = {
      ...result,
      diff: [
        "diff --git a/big.txt b/big.txt",
        "@@ -0,0 +1,500 @@",
        ...Array.from({ length: 500 }, () => "+x"),
      ].join("\n"),
      files: [
        { path: "big.txt", status: "A ", additions: 500, deletions: 0 },
        { path: "later.txt", status: "??", additions: 0, deletions: 0 },
      ],
    };
    // A file without a diff (untracked, not read) is skipped; size does not matter.
    expect(autoOpenPath(changeEntries(big))).toBe("big.txt");
    expect(autoOpenPath([])).toBeNull();
  });

  it("keeps the user's choices over the automatic one", () => {
    expect(isFileOpen("a", "a", NO_CHOICES)).toBe(true);
    expect(isFileOpen("b", "a", NO_CHOICES)).toBe(false);
    const closedA = toggleFile(NO_CHOICES, "a", true);
    expect(isFileOpen("a", "a", closedA)).toBe(false);
    const openedB = toggleFile(closedA, "b", false);
    expect(isFileOpen("b", "a", openedB)).toBe(true);
    // A new last file opens by itself; the earlier one closes unless the user opened it.
    expect(isFileOpen("a", "c", openedB)).toBe(false);
    expect(isFileOpen("b", "c", openedB)).toBe(true);
    expect(isFileOpen("c", "c", openedB)).toBe(true);
    const reopened = toggleFile(openedB, "a", false);
    expect(isFileOpen("a", "c", reopened)).toBe(true);
  });
});

describe("splitPath", () => {
  it("separates folder and name", () => {
    expect(splitPath("src/app.ts")).toEqual({ folder: "src", name: "app.ts" });
    expect(splitPath("README.md")).toEqual({ folder: "", name: "README.md" });
  });
});

describe("hidden characters", () => {
  it("flags files whose path or lines hold bidi or invisible characters", () => {
    const entries = changeEntries({
      isRepo: true,
      files: [
        { path: "ok.ts", status: " M", additions: 1, deletions: 0 },
        { path: "evil.ts", status: " M", additions: 1, deletions: 0 },
        { path: "name\u202e.ts", status: "??", additions: 0, deletions: 0 },
      ],
      diff: [
        "diff --git a/ok.ts b/ok.ts",
        "@@ -0,0 +1 @@",
        "+fine",
        "diff --git a/evil.ts b/evil.ts",
        "@@ -0,0 +1 @@",
        "+/* \u202e } \u2066if (isAdmin)\u2069 \u2066 { */",
      ].join("\n"),
      truncated: false,
    });
    expect(entries.map((entry) => [entry.path, entry.hidden])).toEqual([
      ["ok.ts", false],
      ["evil.ts", true],
      ["name\u202e.ts", true],
    ]);
  });
});
