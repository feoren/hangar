// How wide each character is in the terminal. Agent CLIs (Claude Code with Ink and string-width,
// for example) count emoji presentation sequences such as "🏷️" (a text-default symbol plus
// VS16, U+FE0F) as two cells; the terminal must agree, or every cursor position after such a
// character is off and status lines wrap, clip or overlap. The Unicode 11 tables of
// @xterm/addon-unicode11 count those as one cell; the grapheme rules of
// @xterm/addon-unicode-graphemes (Unicode 15, whole grapheme clusters) count two, like
// string-width does. Measured in src/terminalUnicode.test.ts.

import type { Terminal } from "@xterm/xterm";
import { UnicodeGraphemesAddon } from "@xterm/addon-unicode-graphemes";

/** Needs `allowProposedApi`, because `term.unicode` is a proposed API. */
export function setupUnicode(term: Terminal): void {
  term.loadAddon(new UnicodeGraphemesAddon());
  term.unicode.activeVersion = "15-graphemes";
}
