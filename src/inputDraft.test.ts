import { describe, expect, it } from "vitest";

import { InputDraft } from "./inputDraft";

/** Feeds each entry as one chunk, the way xterm hands over a key or a paste. */
function after(chunks: readonly (string | { key: string })[]): boolean {
  const draft = new InputDraft();
  for (const chunk of chunks) {
    if (typeof chunk === "string") draft.feed(chunk);
    else draft.feed(chunk.key, true);
  }
  return draft.pending;
}

describe("InputDraft", () => {
  it("starts empty and holds a draft once the person types", () => {
    expect(new InputDraft().pending).toBe(false);
    expect(after(["f", "i", "x"])).toBe(true);
    expect(after(["café 👋"])).toBe(true);
  });

  it("counts pasted text, bracketed or not, as a draft", () => {
    expect(after(["\x1b[200~fix the tests\x1b[201~"])).toBe(true);
    expect(after(["\x1b[200~two\rlines\x1b[201~"])).toBe(true);
    expect(after(["plain paste"])).toBe(true);
    // An empty paste leaves the line as it was.
    expect(after(["\x1b[200~\x1b[201~"])).toBe(false);
  });

  it("is gone after Enter", () => {
    expect(after(["fix", "\r"])).toBe(false);
    expect(after(["\x1b[200~fix\x1b[201~", "\r"])).toBe(false);
  });

  it("keeps a draft whose Enter only continues the line after a backslash", () => {
    expect(after(["fix \\", "\r"])).toBe(true);
    expect(after(["fix \\", "\r", "more", "\r"])).toBe(false);
  });

  it("keeps a draft over a new line inside it (Shift+Enter, Alt+Enter, Ctrl+J)", () => {
    expect(after(["one", "\x1b\r", "two"])).toBe(true);
    expect(after(["one", "\x1b\r"])).toBe(true);
    expect(after(["one", "\n"])).toBe(true);
  });

  it("is gone after Ctrl+C or Ctrl+U", () => {
    expect(after(["fix", "\x03"])).toBe(false);
    expect(after(["fix", "\x15"])).toBe(false);
  });

  it("is gone once Backspace removed every character typed", () => {
    expect(after(["ab", "\x7f"])).toBe(true);
    expect(after(["ab", "\x7f", "\x7f"])).toBe(false);
    expect(after(["ab", "\b", "\b", "\b"])).toBe(false);
    expect(after(["👋", "\x7f"])).toBe(false);
  });

  it("is not gone by Backspace or Ctrl+U once the cursor moved inside the line", () => {
    expect(after(["ab", { key: "\x1b[D" }, "\x7f", "\x7f"])).toBe(true);
    expect(after(["ab", { key: "\x1b[1;5D" }, "\x15"])).toBe(true);
    expect(after(["ab", { key: "\x1b[H" }, "\x15"])).toBe(true);
    // Enter and Ctrl+C still send or clear the whole line.
    expect(after(["ab", { key: "\x1b[D" }, "\r"])).toBe(false);
    expect(after(["ab", { key: "\x1b[D" }, "\x03"])).toBe(false);
  });

  it("is not gone by Ctrl+U on a draft of several lines", () => {
    expect(after(["one", "\x1b\r", "two", "\x15"])).toBe(true);
  });

  it("is not gone by deleting a word: how much went is unknown", () => {
    expect(after(["fix it", "\x17"])).toBe(true);
    expect(after(["fix it", "\x1b\x7f"])).toBe(true);
    expect(after(["fix it", "\x0b"])).toBe(true);
    expect(after(["fix it", "\x17", "\r"])).toBe(false);
  });

  it("takes Up or Down as a line from the history, which may fill the line", () => {
    expect(after([{ key: "\x1b[A" }])).toBe(true);
    expect(after([{ key: "\x1bOA" }])).toBe(true);
    expect(after([{ key: "\x1b[B" }, "\x7f", "\x7f"])).toBe(true);
    expect(after([{ key: "\x1b[A" }, "\r"])).toBe(false);
    expect(after([{ key: "\x1b[A" }, "\x03"])).toBe(false);
  });

  it("ignores escape sequences that are no key: replies, focus, mouse and the wheel", () => {
    expect(after(["\x1b[12;40R", "\x1b[I", "\x1b[O", "\x1b[<0;10;5M", "\x1b[?62;22c"])).toBe(false);
    // The wheel scrolls a full-screen program with Up and Down, which are no keys there.
    expect(after(["\x1b[A", "\x1b[B", "\x1bOA"])).toBe(false);
    expect(after(["\x1b]11;rgb:0000/0000/0000\x07"])).toBe(false);
    // A lone Escape clears the line in some agents only; it never counts as clearing.
    expect(after([{ key: "\x1b" }])).toBe(false);
    expect(after(["ab", { key: "\x1b" }])).toBe(true);
    expect(after(["ab", { key: "\x1b" }, { key: "\x1b" }])).toBe(true);
  });

  it("does not take a reply inside a chunk for text", () => {
    expect(after(["\x1b[I\x1b[O"])).toBe(false);
    expect(after(["\x1b[1;1R", "\r"])).toBe(false);
  });

  it("never lets an Alt key's open sequence swallow the characters typed next", () => {
    // Alt+], Alt+[, Alt+O and Alt+Shift+P look like the start of OSC, CSI, SS3 and DCS.
    expect(after([{ key: "\x1b]" }, "fix"])).toBe(true);
    expect(after([{ key: "\x1b[" }, "123"])).toBe(true);
    expect(after([{ key: "\x1bO" }, "x"])).toBe(true);
    expect(after([{ key: "\x1bP" }, "x"])).toBe(true);
  });

  it("counts a paste whose end marker never came", () => {
    expect(after(["\x1b[200~fix", "\r"])).toBe(false);
    expect(after(["\x1b[200~fix"])).toBe(true);
    expect(after(["\x1b[200~fix\x1b[20"])).toBe(true);
  });

  it("ignores string replies and X10 mouse reports, whose bytes look like text", () => {
    expect(after(["\x1bP1$r0m\x1b\\"])).toBe(false);
    expect(after(["\x1b[M #!", "\x1b[M#$%"])).toBe(false);
    expect(after(["\x1b[M #!", "x"])).toBe(true);
  });

  it("ignores other control keys: Tab, Ctrl+O, Ctrl+L", () => {
    expect(after(["\t", "\x0f", "\x0c"])).toBe(false);
  });

  it("starts empty again on reset (a new process)", () => {
    const draft = new InputDraft();
    draft.feed("fix");
    draft.reset();
    expect(draft.pending).toBe(false);
  });
});
