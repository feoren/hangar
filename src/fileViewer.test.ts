import { describe, expect, it } from "vitest";

import type { FileContent } from "./api";
import { editorPhrases, viewerBody } from "./fileViewer";
import { createTranslator } from "./i18n";

const content = (extra: Partial<FileContent> = {}): FileContent => ({
  name: "main.ts",
  path: "C:\\work\\app\\src\\main.ts",
  relativePath: "src/main.ts",
  size: 12,
  text: "const a = 1;",
  truncated: false,
  binary: false,
  ...extra,
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
