import { describe, expect, it } from "vitest";

import { SubmittedLines } from "./handover";
import { selectionDeleteKeys, type ClickCell } from "./terminalClick";
import { TypedInput } from "./typedInput";

/** A session's readers as the terminal wires them: handover request, title lines, prompt bar. */
function session() {
  const handover = new SubmittedLines();
  const lines: string[] = [];
  const bar: string[][] = [];
  const input = new TypedInput({
    line: (line, agent) => {
      handover.record(line, agent);
      lines.push(line);
    },
    lastPrompts: (texts) => {
      bar.push([...texts]);
    },
  });
  return { input, handover, lines, bar, shown: () => bar.at(-1)?.at(-1) ?? null };
}

/** The prompt row as the agent draws it: its prompt sign, then what the user typed. */
function row(text: string): ClickCell[] {
  return Array.from(text, (char) => ({ width: 1, text: char, dim: false, inverse: false }));
}

describe("TypedInput", () => {
  it("gives the prompt bar and the handover the same line after deleting a selection", () => {
    const { input, handover, lines, shown } = session();
    input.feed("fix the login bug now", true);
    // "now" marked at the end of "> fix the login bug now", the cursor right after it.
    const keys = selectionDeleteKeys(
      row("> fix the login bug now"),
      23,
      { start: 20, end: 23 },
      false,
    );
    expect(keys).toBe("\x7f".repeat(3));
    input.feed(keys ?? "", true);
    input.feed("\r", true);

    expect(lines).toEqual(["fix the login bug "]);
    expect(handover.request).toBe("fix the login bug");
    expect(shown()).toBe("fix the login bug");
  });

  it("passes a shell's lines on but keeps them out of the prompt bar and the handover", () => {
    const { input, handover, lines, bar } = session();
    input.feed("git status\r", false);
    expect(lines).toEqual(["git status"]);
    expect(handover.request).toBeNull();
    expect(bar).toEqual([]);
  });

  it("calls the prompt bar once per chunk, with the lines worth showing only", () => {
    const { input, lines, bar } = session();
    input.feed("first task\r/clear\rsecond task\r", true);
    expect(lines).toEqual(["first task", "/clear", "second task"]);
    expect(bar).toEqual([["first task", "second task"]]);
    // A chunk with nothing to show leaves the bar alone.
    input.feed("/model\r", true);
    expect(bar).toHaveLength(1);
  });

  it("keeps the line across chunks until Enter, wherever the keys came from", () => {
    const { input, lines, shown } = session();
    input.feed("add ", true);
    input.feed("tests", true);
    expect(lines).toEqual([]);
    input.feed("\r", true);
    expect(shown()).toBe("add tests");
  });
});
