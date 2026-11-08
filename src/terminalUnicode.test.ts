import { Terminal } from "@xterm/xterm";
import { describe, expect, it } from "vitest";

import { setupUnicode } from "./terminalUnicode";

/** Cells the cursor moved after writing `text` at the start of a line. */
async function cellWidth(term: Terminal, text: string): Promise<number> {
  await new Promise<void>((resolve) => {
    term.write(`\x1b[H\x1b[2K${text}`, resolve);
  });
  return term.buffer.active.cursorX;
}

function terminal(): Terminal {
  // Not opened: the buffer and the width rules work without a DOM.
  const term = new Terminal({ allowProposedApi: true, cols: 120, rows: 4 });
  setupUnicode(term);
  return term;
}

describe("terminal character widths", () => {
  it("counts emoji presentation sequences (VS16) as two cells, as agent CLIs do", async () => {
    const term = terminal();
    // From Claude Code's status line: label, pencil, stopwatch.
    for (const emoji of ["\u{1F3F7}\u{FE0F}", "\u{270F}\u{FE0F}", "\u{23F1}\u{FE0F}"]) {
      expect(await cellWidth(term, emoji), JSON.stringify(emoji)).toBe(2);
    }
    term.dispose();
  });

  it("keeps the widths that were already right", async () => {
    const term = terminal();
    const cases: [string, number][] = [
      ["\u{1F50B}", 2], // battery, emoji by default
      ["\u{1F4C1}", 2], // folder
      ["⚡", 2], // high voltage
      ["中", 2], // CJK
      ["█", 1], // full block of a progress bar
      ["↻", 1], // clockwise arrow
      ["é", 1], // combining accent
      ["abc", 3],
    ];
    for (const [text, width] of cases) {
      expect(await cellWidth(term, text), JSON.stringify(text)).toBe(width);
    }
    term.dispose();
  });

  it("counts a whole grapheme cluster as one emoji", async () => {
    const term = terminal();
    const cases: [string, number][] = [
      ["\u{1F468}‍\u{1F469}‍\u{1F467}‍\u{1F466}", 2], // ZWJ family
      ["\u{1F469}\u{1F3FD}‍\u{1F4BB}", 2], // skin tone and ZWJ
      ["\u{1F1E9}\u{1F1EA}", 2], // flag
      ["\u{1F1E9}\u{1F1EA}\u{1F1FA}\u{1F1F8}", 4], // two flags in a row
      ["#\u{FE0F}\u{20E3}", 2], // keycap
      ["e\u{0301}", 1], // decomposed accent
      ["日本語", 6],
    ];
    for (const [text, width] of cases) {
      expect(await cellWidth(term, text), JSON.stringify(text)).toBe(width);
    }
    term.dispose();
  });

  it("keeps the line drawing of TUIs at one cell per character", async () => {
    const term = terminal();
    const cases = [
      "─│┌┐└┘├┤┬┴┼═║╔╗╚╝╭╮╯╰", // box drawing
      "▀▄▌▐░▒▓█", // blocks
      "⠋⠙⠹", // braille spinner
      "\u{E0A0}\u{E0B0}\u{E0B1}\u{E0B2}\u{E0B3}\u{E0B4}\u{E0B6}", // powerline (private use)
      "\u{23FA}\u{273B}\u{273D}\u{2722}·\u{23BF}\u{276F}✓✔●…→", // agent CLI symbols without VS16
    ];
    for (const text of cases) {
      expect(await cellWidth(term, text), JSON.stringify(text)).toBe(Array.from(text).length);
    }
    term.dispose();
  });

  it("puts the rest of a status line where the agent expects it", async () => {
    const term = terminal();
    // "🏷️ x" is four cells for string-width: two for the emoji, the space, the letter.
    expect(await cellWidth(term, "\u{1F3F7}\u{FE0F} x")).toBe(4);
    expect(await cellWidth(term, "\u{23F1}\u{FE0F} 5h 07m | \u{1F50B} 5h 37%")).toBe(21);
    term.dispose();
  });
});
