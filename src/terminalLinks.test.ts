import { describe, expect, it } from "vitest";

import {
  cellText,
  findLinks,
  linkSpans,
  pathAction,
  pathSegments,
  pickLinks,
  relativeInside,
  resolvePath,
  type TextLink,
} from "./terminalLinks";

/** The texts of all candidates, as they stand in the line. */
const texts = (line: string) => findLinks(line).map((link) => line.slice(link.start, link.end));

const only = (line: string): TextLink => {
  const links = findLinks(line);
  expect(links).toHaveLength(1);
  const [link] = links;
  if (link === undefined) throw new Error("no link");
  return link;
};

describe("findLinks", () => {
  it("finds web addresses and strips sentence punctuation", () => {
    expect(only("See https://example.com/docs.").target).toEqual({
      kind: "url",
      url: "https://example.com/docs",
    });
    expect(texts("(http://localhost:5173/app), then")).toEqual(["http://localhost:5173/app"]);
    expect(texts("https://en.wikipedia.org/wiki/Foo_(bar) ok")).toEqual([
      "https://en.wikipedia.org/wiki/Foo_(bar)",
    ]);
  });

  it("does not take parts of an address for paths", () => {
    expect(texts("open https://example.com/a/b.md now")).toEqual(["https://example.com/a/b.md"]);
  });

  it("finds relative paths with and without a line suffix", () => {
    expect(texts("Edited docs/wiki/index.md and ./foo and ../bar")).toEqual([
      "docs/wiki/index.md",
      "./foo",
      "../bar",
    ]);
    const link = only("Error in src/App.tsx:120:7, fix it");
    expect(link.target).toEqual({ kind: "path", path: "src/App.tsx", line: 120, column: 7 });
    expect(link.pathEnd - link.start).toBe("src/App.tsx".length);
    expect(only("src/App.tsx:120").target).toMatchObject({ line: 120, column: null });
  });

  it("finds absolute Windows and Unix paths", () => {
    expect(texts(String.raw`at C:\Users\me\file.txt and C:/x/y.md`)).toEqual([
      String.raw`C:\Users\me\file.txt`,
      "C:/x/y.md",
    ]);
    expect(only(String.raw`D:\repo\src\main.rs:3:9`).target).toEqual({
      kind: "path",
      path: String.raw`D:\repo\src\main.rs`,
      line: 3,
      column: 9,
    });
    expect(texts("wrote /home/me/notes.md.")).toEqual(["/home/me/notes.md"]);
  });

  it("finds bare file names with an extension", () => {
    expect(texts("Updated README.md and .gitignore")).toEqual(["README.md", ".gitignore"]);
    expect(texts("version 1.2.3 is out")).toEqual([]);
  });

  it("takes quoted paths whole, spaces included", () => {
    const line = 'Saved "C:\\Program Files\\Hangar\\a b.txt" now';
    expect(texts(line)).toContain(String.raw`C:\Program Files\Hangar\a b.txt`);
    expect(texts("Read `docs/my notes.md`.")).toContain("docs/my notes.md");
    expect(texts("Open 'src/a b.ts'")).toContain("src/a b.ts");
  });

  it("strips brackets and trailing punctuation around paths", () => {
    expect(texts("(see src/a.ts), [b/c.md]; d/e.md: and f/g.md!")).toEqual([
      "src/a.ts",
      "b/c.md",
      "d/e.md",
      "f/g.md",
    ]);
  });

  it("ignores apostrophes, labels, schemes and network paths", () => {
    expect(texts("it's done")).toEqual([]);
    expect(texts("error: failed")).toEqual([]);
    expect(texts("mailto:a@example.com")).toEqual([]);
    expect(texts(String.raw`\\server\share\file.txt`)).toEqual([]);
    expect(texts("//server/share")).toEqual([]);
    expect(texts("just words here")).toEqual([]);
    expect(texts("/")).toEqual([]);
  });
});

describe("pickLinks", () => {
  it("prefers the longest existing candidate and drops what overlaps it", () => {
    const line = "`pnpm test src/a.ts`";
    const links = findLinks(line);
    const all = pickLinks(links, () => true);
    expect(all.map((link) => line.slice(link.start, link.end))).toEqual(["pnpm test src/a.ts"]);
    const existing = pickLinks(links, (link) => line.slice(link.start, link.end) === "src/a.ts");
    expect(existing.map((link) => line.slice(link.start, link.end))).toEqual(["src/a.ts"]);
  });
});

describe("pathSegments", () => {
  it("splits at both separators", () => {
    const path = "ordnername/dateiname.md";
    expect(pathSegments(path).map((s) => path.slice(s.start, s.end))).toEqual([
      "ordnername",
      "dateiname.md",
    ]);
    const windows = String.raw`C:\x\y`;
    expect(pathSegments(windows).map((s) => windows.slice(0, s.end))).toEqual([
      "C:",
      String.raw`C:\x`,
      String.raw`C:\x\y`,
    ]);
    expect(pathSegments("/usr/bin").map((s) => "/usr/bin".slice(0, s.end))).toEqual([
      "/usr",
      "/usr/bin",
    ]);
  });
});

describe("linkSpans", () => {
  it("keeps a link whole without Shift", () => {
    const link = only("see docs/a.md:3");
    expect(linkSpans(link, false)).toEqual([
      { start: 4, end: 15, opens: "docs/a.md", whole: true },
    ]);
  });

  it("splits a path into segments with Shift; the last keeps the suffix", () => {
    const link = only("see docs/a.md:3");
    expect(linkSpans(link, true)).toEqual([
      { start: 4, end: 8, opens: "docs", whole: false },
      { start: 9, end: 15, opens: "docs/a.md", whole: true },
    ]);
  });

  it("never splits an address", () => {
    const link = only("https://example.com/a/b");
    expect(linkSpans(link, true)).toHaveLength(1);
  });
});

describe("resolvePath", () => {
  const cwd = String.raw`E:\repo\worktree`;

  it("resolves relative paths against a Windows folder", () => {
    expect(resolvePath(cwd, "docs/wiki/index.md")).toBe(
      String.raw`E:\repo\worktree\docs\wiki\index.md`,
    );
    expect(resolvePath(cwd, "./foo")).toBe(String.raw`E:\repo\worktree\foo`);
    expect(resolvePath(cwd, "../bar")).toBe(String.raw`E:\repo\bar`);
    expect(resolvePath(cwd, ".")).toBe(cwd);
    expect(resolvePath("C:\\", "..")).toBe("C:\\");
  });

  it("keeps absolute Windows paths and reads Git Bash drive paths", () => {
    expect(resolvePath(cwd, "C:/x/./y/../z.md")).toBe(String.raw`C:\x\z.md`);
    expect(resolvePath(cwd, "C:")).toBe("C:\\");
    expect(resolvePath(cwd, "/c/Users/me")).toBe(String.raw`C:\Users\me`);
  });

  it("refuses what it cannot place on Windows", () => {
    expect(resolvePath(cwd, "/help")).toBeNull();
    expect(resolvePath(cwd, "C:relative")).toBeNull();
    expect(resolvePath(String.raw`\\wsl$\Ubuntu\home`, "a.md")).toBeNull();
  });

  it("resolves Unix paths", () => {
    expect(resolvePath("/home/me/repo", "src/a.ts")).toBe("/home/me/repo/src/a.ts");
    expect(resolvePath("/home/me/repo", "../../x")).toBe("/home/x");
    expect(resolvePath("/home/me/repo", "/etc/hosts")).toBe("/etc/hosts");
    expect(resolvePath("/", "..")).toBe("/");
  });
});

describe("relativeInside", () => {
  it("gives the viewer's path inside the folder, without case on Windows", () => {
    expect(relativeInside(String.raw`E:\Repo`, String.raw`e:\repo\docs\a.md`)).toBe("docs/a.md");
    expect(relativeInside("E:\\Repo\\", String.raw`E:\Repo\a.md`)).toBe("a.md");
    expect(relativeInside(String.raw`E:\Repo`, String.raw`E:\Repository\a.md`)).toBeNull();
    expect(relativeInside(String.raw`E:\Repo`, String.raw`E:\Repo`)).toBeNull();
    expect(relativeInside("/home/me", "/home/me/a/b.md")).toBe("a/b.md");
    expect(relativeInside("/home/me", "/home/Me/a.md")).toBeNull();
  });
});

describe("pathAction", () => {
  const root = String.raw`E:\repo`;

  it("opens folders in the file manager", () => {
    expect(pathAction("dir", String.raw`E:\repo\src`, root)).toEqual({ kind: "folder" });
  });

  it("shows files of the session's folder in the viewer", () => {
    expect(pathAction("file", String.raw`E:\repo\src\App.tsx`, root)).toEqual({
      kind: "viewer",
      relative: "src/App.tsx",
    });
    expect(pathAction("file", String.raw`E:\repo\logo.png`, root)).toMatchObject({
      kind: "viewer",
    });
  });

  it("leaves other files to their own program", () => {
    expect(pathAction("file", String.raw`E:\repo\docs\manual.pdf`, root)).toEqual({
      kind: "system",
    });
    expect(pathAction("file", String.raw`C:\elsewhere\a.md`, root)).toEqual({ kind: "system" });
  });
});

describe("cellText", () => {
  it("maps characters to cells across wide characters and wrapped rows", () => {
    const row1 = [
      { chars: "a", width: 1 },
      { chars: "字", width: 2 },
      { chars: "", width: 0 },
      { chars: "b", width: 1 },
    ];
    const row2 = [
      { chars: "c", width: 1 },
      { chars: "", width: 1 },
    ];
    const cells = cellText([row1, row2], 9);
    expect(cells.text).toBe("a字bc ");
    expect(cells.from[1]).toEqual({ x: 2, y: 10 });
    expect(cells.to[1]).toEqual({ x: 3, y: 10 });
    expect(cells.from[2]).toEqual({ x: 4, y: 10 });
    expect(cells.from[3]).toEqual({ x: 1, y: 11 });
  });
});
