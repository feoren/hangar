import { describe, expect, it } from "vitest";

import { clipboardWriteText, menuSelection, ProgramSelection } from "./programSelection";

/** OSC 52 payload as a program sends it: targets, then the UTF-8 text in base64. */
function osc52(text: string, targets = "c"): string {
  const bytes = new TextEncoder().encode(text);
  return `${targets};${btoa(String.fromCharCode(...bytes))}`;
}

describe("clipboardWriteText", () => {
  it("decodes the text of a clipboard write, UTF-8 included", () => {
    expect(clipboardWriteText(osc52("npm run build"))).toBe("npm run build");
    expect(clipboardWriteText(osc52("Größe: 3 × 4 → 12"))).toBe("Größe: 3 × 4 → 12");
    expect(clipboardWriteText(osc52("two\nlines", ""))).toBe("two\nlines");
  });

  it("treats an empty payload as clearing the clipboard", () => {
    expect(clipboardWriteText("c;")).toBe("");
  });

  it("ignores queries and malformed data", () => {
    expect(clipboardWriteText("c;?")).toBeNull();
    expect(clipboardWriteText("c")).toBeNull();
    expect(clipboardWriteText("x;aGVsbG8=")).toBeNull();
    expect(clipboardWriteText("c;not base64!")).toBeNull();
    expect(clipboardWriteText("c;a")).toBeNull();
  });

  it("ignores oversized payloads", () => {
    expect(clipboardWriteText(`c;${"A".repeat(4 * 1024 * 1024)}`)).toBeNull();
  });
});

describe("ProgramSelection", () => {
  it("keeps what the program copied after a drag in the terminal", () => {
    const selection = new ProgramSelection();
    selection.pointerDown(0);
    selection.clipboardWritten("selected output");
    expect(selection.current).toBe("selected output");
  });

  it("does not count a copy that no mouse press led to", () => {
    const selection = new ProgramSelection();
    selection.clipboardWritten("whole response");
    expect(selection.current).toBe("");
  });

  it("drops the selection on a typed key, and a later copy does not count", () => {
    const selection = new ProgramSelection();
    selection.pointerDown(0);
    selection.clipboardWritten("selected output");
    selection.keyTyped();
    expect(selection.current).toBe("");
    selection.clipboardWritten("copied by /copy");
    expect(selection.current).toBe("");
  });

  it("drops the selection on a new left or middle press", () => {
    const selection = new ProgramSelection();
    selection.pointerDown(0);
    selection.clipboardWritten("first");
    selection.pointerDown(0);
    expect(selection.current).toBe("");
    selection.clipboardWritten("second");
    selection.pointerDown(1);
    expect(selection.current).toBe("");
  });

  it("keeps the selection across the right click that opens the menu", () => {
    const selection = new ProgramSelection();
    selection.pointerDown(0);
    selection.clipboardWritten("selected output");
    selection.pointerDown(2);
    expect(selection.current).toBe("selected output");
  });

  it("forgets everything on reset", () => {
    const selection = new ProgramSelection();
    selection.pointerDown(0);
    selection.clipboardWritten("selected output");
    selection.reset();
    expect(selection.current).toBe("");
    selection.clipboardWritten("after restart");
    expect(selection.current).toBe("");
  });
});

describe("menuSelection", () => {
  it("prefers the terminal's own selection", () => {
    expect(menuSelection("xterm text", "program text")).toBe("xterm text");
  });

  it("falls back to the program's selection", () => {
    expect(menuSelection("", "program text")).toBe("program text");
    expect(menuSelection("", "")).toBe("");
  });
});
