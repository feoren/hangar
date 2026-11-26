// @vitest-environment happy-dom

// A note written with CRLF (edited outside Hangar, or on another system) must open with its text:
// a cursor placed by the raw length pointed past the end of CodeMirror's LF document, creating
// the state threw, and the note opened empty.

import { afterEach, describe, expect, it } from "vitest";

import { createMarkdownEditor, editorText, type MarkdownEditor } from "./markdownEditor";

const CRLF = "# Title\r\n\r\n- [ ] first\r\n- [x] second\r\n";
const LF = "# Title\n\n- [ ] first\n- [x] second\n";

let editor: MarkdownEditor | null = null;
let host: HTMLElement | null = null;

afterEach(() => {
  editor?.destroy();
  host?.remove();
  editor = null;
  host = null;
});

function open(text: string): MarkdownEditor {
  host = document.createElement("div");
  document.body.append(host);
  editor = createMarkdownEditor(host, {
    text,
    label: "Note",
    placeholder: "",
    taskLabel: "Task",
    onChange: () => undefined,
    onSelection: () => undefined,
    onEscape: () => undefined,
  });
  return editor;
}

function shown(): string {
  return [...(host?.querySelectorAll(".cm-line") ?? [])].map((line) => line.textContent).join("\n");
}

describe("markdown editor line breaks", () => {
  it("turns CRLF and lone CR into LF", () => {
    expect(editorText(CRLF)).toBe(LF);
    expect(editorText("a\rb\r\nc\nd")).toBe("a\nb\nc\nd");
    expect(editorText(LF)).toBe(LF);
  });

  it("opens a CRLF note with its text instead of empty", () => {
    open(CRLF);
    expect(shown()).toContain("first");
    expect(shown()).toContain("second");
  });

  it("loads and replaces CRLF text without throwing", () => {
    const view = open("");
    expect(() => {
      view.load(CRLF, "Note");
    }).not.toThrow();
    expect(shown()).toContain("Title");
    expect(() => {
      view.setText("one\r\ntwo\r\n");
    }).not.toThrow();
    expect(shown()).toContain("two");
  });
});
