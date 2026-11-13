// Sample output for the terminal look preview in Settings: a prompt and an agent's framed edit,
// drawn with the box characters agent CLIs use, so a font without them shows at once.

import type { AnsiColor } from "./theme";

/** `null` is the scheme's foreground. */
export type PreviewColor = AnsiColor | null;
export type PreviewSegment = readonly [PreviewColor, string];
export type PreviewLine = readonly PreviewSegment[];

/** Columns between the two vertical lines of the frame. */
const INNER = 30;

function framed(content: readonly PreviewSegment[]): PreviewLine {
  const used = content.reduce((width, [, text]) => width + text.length, 0);
  return [
    ["brightBlack", "│ "],
    ...content,
    [null, " ".repeat(Math.max(0, INNER - 2 - used))],
    ["brightBlack", " │"],
  ];
}

function rule(left: string, label: readonly PreviewSegment[], right: string): PreviewLine {
  // The corner and the first dash stand left of the label, so the label starts one column in.
  const used = 1 + label.reduce((width, [, text]) => width + text.length, 0);
  return [
    ["brightBlack", `${left}─`],
    ...label,
    ["brightBlack", `${"─".repeat(Math.max(1, INNER - used))}${right}`],
  ];
}

/** The lines before the last prompt; the preview adds the cursor after the last line. */
export const PREVIEW_LINES: readonly PreviewLine[] = [
  [
    ["green", "~/hangar"],
    [null, " "],
    ["blue", "main"],
    [null, " "],
    ["brightBlack", "$"],
    [null, " agent"],
  ],
  rule(
    "╭",
    [
      [null, " "],
      ["yellow", "●"],
      [null, " "],
      ["magenta", "Update"],
      ["cyan", " theme.ts "],
    ],
    "╮",
  ),
  framed([["green", '+ scheme = "fjord";']]),
  framed([["red", '- scheme = "dusk";']]),
  rule("╰", [["brightBlack", " esc to cancel "]], "╯"),
  [
    ["cyan", ">"],
    [null, " "],
  ],
];

/** The short sample in each scheme tile: a prompt and one changed line each way. */
export const TILE_LINES: readonly PreviewLine[] = [
  [
    ["green", "~/app"],
    [null, " "],
    ["blue", "main"],
    [null, " "],
    ["brightBlack", "$"],
    [null, " diff"],
  ],
  [["green", "+ ready: true"]],
  [["red", "- ready: false"]],
];

/** Width of a line in cells: every character used here is one cell wide and one UTF-16 unit. */
export function lineWidth(line: PreviewLine): number {
  return line.reduce((width, [, text]) => width + text.length, 0);
}
