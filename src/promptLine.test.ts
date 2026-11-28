import { describe, expect, it } from "vitest";

import { PromptLineTracker } from "./promptLine";

describe("PromptLineTracker", () => {
  it("returns the typed line on Enter, across chunks", () => {
    const tracker = new PromptLineTracker();
    expect(tracker.feed("fix the ")).toEqual([]);
    expect(tracker.feed("login")).toEqual([]);
    expect(tracker.feed("\r")).toEqual(["fix the login"]);
    expect(tracker.feed("next\r")).toEqual(["next"]);
  });

  it("applies Backspace, Ctrl+W, Ctrl+U and Ctrl+C", () => {
    const tracker = new PromptLineTracker();
    expect(tracker.feed("helo\x7f\x7fllo\r")).toEqual(["hello"]);
    expect(tracker.feed("one two \x17three\r")).toEqual(["one three"]);
    expect(tracker.feed("gone\x15kept\r")).toEqual(["kept"]);
    expect(tracker.feed("gone\x03kept\r")).toEqual(["kept"]);
  });

  it("skips arrows, focus reports and cursor position replies", () => {
    const tracker = new PromptLineTracker();
    expect(tracker.feed("\x1b[12;1R\x1b[Iab\x1b[D\x1bOAcd\r")).toEqual(["abcd"]);
  });

  it("keeps a split escape sequence out of the line", () => {
    const tracker = new PromptLineTracker();
    tracker.feed("a\x1b[");
    tracker.feed("1;");
    expect(tracker.feed("5Cb\r")).toEqual(["ab"]);
  });

  it("takes a lone Escape key without losing the next character", () => {
    const tracker = new PromptLineTracker();
    tracker.feed("\x1b");
    expect(tracker.feed("fix\r")).toEqual(["fix"]);
  });

  it("takes bracketed paste as text with its line breaks as spaces", () => {
    const tracker = new PromptLineTracker();
    tracker.feed("see \x1b[200~first\r\nsec");
    expect(tracker.feed("ond\x1b[201~ now\r")).toEqual(["see first  second now"]);
  });

  it("treats Alt+Enter as a line break inside the prompt", () => {
    const tracker = new PromptLineTracker();
    expect(tracker.feed("one\x1b\rtwo\r")).toEqual(["one two"]);
  });

  it("ignores characters of other scripts correctly on Backspace", () => {
    const tracker = new PromptLineTracker();
    expect(tracker.feed("ab\u{1F600}\x7fc\r")).toEqual(["abc"]);
  });
});
