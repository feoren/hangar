import { describe, expect, it } from "vitest";

import {
  appendCapture,
  checklistProgress,
  isListLine,
  isTaskLine,
  noteExcerpt,
  taskMarkOffset,
  toggleTaskAtLine,
  wordCount,
} from "./noteText";

describe("checklists", () => {
  const text = "# Plan\n- [x] one\n- [ ] two\n  * [X] nested\n1. [ ] numbered\n- [] no\n";

  it("counts ticked and all tasks", () => {
    expect(checklistProgress(text)).toEqual({ done: 2, total: 4 });
    expect(checklistProgress("nothing")).toEqual({ done: 0, total: 0 });
  });

  it("ignores tasks inside code blocks", () => {
    expect(checklistProgress("```\n- [ ] code\n```\n- [ ] real\n~~~\n- [x] x\n~~~")).toEqual({
      done: 0,
      total: 1,
    });
  });

  it("toggles exactly the given line and keeps line endings", () => {
    expect(toggleTaskAtLine(text, 2)).toBe(text.replace("- [ ] two", "- [x] two"));
    expect(toggleTaskAtLine(text, 1)).toBe(text.replace("- [x] one", "- [ ] one"));
    expect(toggleTaskAtLine("a\r\n- [ ] b\r\n", 1)).toBe("a\r\n- [x] b\r\n");
    expect(toggleTaskAtLine(text, 0)).toBe(text);
    expect(toggleTaskAtLine(text, 99)).toBe(text);
    expect(toggleTaskAtLine("```\n- [ ] c\n```", 1)).toBe("```\n- [ ] c\n```");
  });

  it("finds task lines and the box position", () => {
    expect(isTaskLine(text, 3)).toBe(true);
    expect(isTaskLine(text, 5)).toBe(false);
    expect(taskMarkOffset("  - [ ] x")).toBe(5);
    expect(taskMarkOffset("- x")).toBeNull();
  });

  it("recognizes list items", () => {
    expect(isListLine("- a")).toBe(true);
    expect(isListLine("  12. a")).toBe(true);
    expect(isListLine("-a")).toBe(false);
    expect(isListLine("text")).toBe(false);
  });
});

describe("wordCount", () => {
  it("counts words, not symbols", () => {
    expect(wordCount("", "en")).toBe(0);
    expect(wordCount("  \n ", "en")).toBe(0);
    expect(wordCount("# Hello world\n- [ ] two words", "en")).toBe(4);
  });
});

describe("noteExcerpt", () => {
  it("skips the title line and Markdown characters", () => {
    expect(noteExcerpt("# Plan\n\n- [ ] write **tests**\n> quote", "Plan")).toBe(
      "write tests quote",
    );
    expect(noteExcerpt("Plan", "Plan")).toBe("");
  });

  it("shortens long text", () => {
    const excerpt = noteExcerpt("x\n" + "word ".repeat(80), "x");
    expect(Array.from(excerpt)).toHaveLength(120);
    expect(excerpt.endsWith("…")).toBe(true);
  });
});

describe("appendCapture", () => {
  it("adds time, source and the output as a code block", () => {
    expect(appendCapture("# Log\n\n", "error: x\r\n", "14:32", "Claude Code")).toBe(
      "# Log\n\n_14:32 · Claude Code_\n\n```\nerror: x\n```\n",
    );
    expect(appendCapture("", "a", "9:01", "")).toBe("_9:01_\n\n```\na\n```\n");
  });

  it("uses a longer fence when the output contains one", () => {
    expect(appendCapture("", "```js\nx\n```", "1", "")).toContain("````\n```js");
  });

  it("keeps the session title on one plain line in the heading", () => {
    // The program sets the title (OSC 0/2); it must not break out of the italic heading.
    expect(appendCapture("", "a", "9:01", "x_y *b* `c` [d](e)\n# Head\u0007")).toBe(
      "_9:01 · x\\_y \\*b\\* \\`c\\` \\[d\\](e) # Head_\n\n```\na\n```\n",
    );
    expect(appendCapture("", "a", "9:01", "\r\n\u001b\t")).toBe("_9:01_\n\n```\na\n```\n");
  });
});
