import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { FileContent, HeadDiff } from "./api";
import {
  diffNote,
  editorPhrases,
  initialMode,
  isMarkdown,
  MISSING_STAMP,
  pollStamps,
  stampKey,
  viewerBody,
  viewerModes,
  diffWorks,
  refreshInterval,
} from "./fileViewer";
import { createTranslator } from "./i18n";

const content = (extra: Partial<FileContent> = {}): FileContent => ({
  name: "main.ts",
  path: "C:\\work\\app\\src\\main.ts",
  relativePath: "src/main.ts",
  size: 12,
  text: "const a = 1;",
  truncated: false,
  binary: false,
  modified: 1000,
  image: null,
  imageTooLarge: false,
  ...extra,
});

const png = { mime: "image/png" as const, data: "iVBORw0KGgo=" };

describe("viewer modes", () => {
  it("offers a preview for Markdown and SVG, a diff for text files in a repository", () => {
    expect(viewerModes(null, true)).toEqual({ preview: false, diff: false });
    expect(viewerModes(content(), true)).toEqual({ preview: false, diff: true });
    expect(viewerModes(content({ name: "README.MD" }), true)).toEqual({
      preview: true,
      diff: true,
    });
    const svg = content({ name: "logo.svg", image: { mime: "image/svg+xml", data: "PHN2Zz4=" } });
    expect(viewerModes(svg, true)).toEqual({ preview: true, diff: true });
    // A PNG has only its picture.
    const raster = content({ name: "a.png", text: null, binary: true, image: png });
    expect(viewerModes(raster, true)).toEqual({ preview: false, diff: false });
    expect(initialMode(raster)).toBe("preview");
    expect(initialMode(svg)).toBe("preview");
    expect(initialMode(content({ name: "a.md" }))).toBe("code");
  });

  it("hides the diff switch where a comparison cannot work", () => {
    expect(viewerModes(content(), false)).toEqual({ preview: false, diff: false });
    const answer = (state: HeadDiff["state"]): HeadDiff => ({ state, diff: "", truncated: false });
    expect(diffWorks(null)).toBe(false);
    expect(diffWorks(answer("notRepo"))).toBe(false);
    expect(diffWorks(answer("noHead"))).toBe(false);
    for (const state of ["notInHead", "same", "changed"] as const) {
      expect(diffWorks(answer(state))).toBe(true);
    }
  });

  it("asks less often for images", () => {
    expect(refreshInterval(null)).toBe(1000);
    expect(refreshInterval(content())).toBe(1000);
    expect(refreshInterval(content({ name: "a.png", text: null, binary: true, image: png }))).toBe(
      4000,
    );
    expect(refreshInterval(content({ text: null, binary: true, imageTooLarge: true }))).toBe(4000);
  });

  it("knows Markdown by its name", () => {
    for (const name of ["a.md", "B.Markdown", "c.mdown", "d.mkd"])
      expect(isMarkdown(name)).toBe(true);
    for (const name of ["md", "a.mdx", "a.txt", "a.md.bak"]) expect(isMarkdown(name)).toBe(false);
  });

  it("shows the body of each mode", () => {
    const md = content({ name: "a.md", text: "# x" });
    expect(viewerBody(md, null, "preview")).toEqual({
      kind: "markdown",
      text: "# x",
      truncated: false,
    });
    expect(viewerBody(md, null, "code")).toMatchObject({ kind: "text" });
    expect(viewerBody(md, null, "diff")).toEqual({ kind: "diff" });
    const raster = content({ name: "a.png", text: null, binary: true, image: png });
    // Whatever the mode, a raster image shows its picture, as a data address only.
    for (const mode of ["code", "preview", "diff"] as const) {
      expect(viewerBody(raster, null, mode)).toEqual({
        kind: "image",
        src: "data:image/png;base64,iVBORw0KGgo=",
      });
    }
    const big = content({ name: "a.png", text: null, binary: true, imageTooLarge: true });
    expect(viewerBody(big, null, "preview")).toEqual({ kind: "imageTooLarge" });
    // An error wins over every mode.
    expect(viewerBody(md, "gone", "preview")).toEqual({ kind: "failed", message: "gone" });
  });

  it("names why there is no diff", () => {
    expect(diffNote({ state: "same", diff: "", truncated: false })).toBe("viewer.diffSame");
    expect(diffNote({ state: "notInHead", diff: "", truncated: false })).toBe(
      "viewer.diffNotInHead",
    );
    expect(diffNote({ state: "changed", diff: "@@", truncated: false })).toBeNull();
  });
});

describe("pollStamps", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  function setup(stamps: (string | Error)[], shown: string | null = "a") {
    let current = shown;
    const reloads: number[] = [];
    let asked = 0;
    let paused = false;
    const stop = pollStamps({
      read: () => {
        const next = stamps[Math.min(asked, stamps.length - 1)];
        asked += 1;
        return next instanceof Error ? Promise.reject(next) : Promise.resolve(next ?? "a");
      },
      shown: () => current,
      reload: () => {
        reloads.push(asked);
        current = stamps[asked - 1] instanceof Error ? MISSING_STAMP : String(stamps[asked - 1]);
      },
      paused: () => paused,
      intervalMs: () => 1000,
    });
    return {
      stop,
      reloads,
      asked: () => asked,
      pause: (value: boolean) => {
        paused = value;
      },
    };
  }

  it("reloads once a new stamp held for two asks", async () => {
    const poll = setup(["a", "b", "c", "c", "c"]);
    await vi.advanceTimersByTimeAsync(3000);
    expect(poll.reloads).toEqual([]);
    await vi.advanceTimersByTimeAsync(1000);
    expect(poll.reloads).toEqual([4]);
    await vi.advanceTimersByTimeAsync(3000);
    expect(poll.reloads).toEqual([4]);
    poll.stop();
  });

  it("treats a file that cannot be read as missing", async () => {
    const poll = setup([new Error("gone"), new Error("gone")]);
    await vi.advanceTimersByTimeAsync(2000);
    expect(poll.reloads).toEqual([2]);
    poll.stop();
  });

  it("asks nothing while paused and nothing after it stopped", async () => {
    const poll = setup(["b"]);
    poll.pause(true);
    await vi.advanceTimersByTimeAsync(5000);
    expect(poll.asked()).toBe(0);
    poll.pause(false);
    await vi.advanceTimersByTimeAsync(1000);
    expect(poll.asked()).toBe(1);
    poll.stop();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(poll.asked()).toBe(1);
    expect(poll.reloads).toEqual([]);
  });

  it("builds stamps from time and size", () => {
    expect(stampKey({ modified: 5, size: 3 })).toBe("5:3");
    expect(stampKey({ modified: null, size: 3 })).toBe("null:3");
  });
});

describe("viewerBody", () => {
  it("shows loading, then the text", () => {
    expect(viewerBody(null, null)).toEqual({ kind: "loading" });
    expect(viewerBody(content(), null)).toEqual({
      kind: "text",
      text: "const a = 1;",
      truncated: false,
    });
    expect(viewerBody(content({ truncated: true }), null)).toMatchObject({ truncated: true });
  });

  it("shows binary files as such and an error over everything", () => {
    expect(viewerBody(content({ text: null, binary: true }), null)).toEqual({ kind: "binary" });
    expect(viewerBody(content(), "The file no longer exists")).toEqual({
      kind: "failed",
      message: "The file no longer exists",
    });
  });
});

describe("editorPhrases", () => {
  it("translates CodeMirror's search and folding texts", () => {
    const de = editorPhrases(createTranslator("de"));
    expect(de.Find).toBe("Suchen");
    expect(de["match case"]).toBe("Groß/klein");
    const en = editorPhrases(createTranslator("en"));
    expect(en["Go to line"]).toBe("Go to line");
    for (const language of ["de", "en", "ja", "zh"] as const) {
      for (const text of Object.values(editorPhrases(createTranslator(language)))) {
        expect(text.trim()).not.toBe("");
      }
    }
  });
});
