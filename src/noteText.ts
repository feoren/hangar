// Text logic of a single note: checklists, word count, excerpt for the list, adding terminal
// output. Pure functions; the editor, the preview and the list use them alike.

/** A Markdown task line: list marker, then `[ ]` or `[x]`. Group 1: everything before the box. */
const TASK_LINE = /^(\s*(?:[-*+]|\d+[.)])\s+)\[([ xX])\](?=\s|$)/;
/** Any list item (bullet or numbered), with or without a task box. */
const LIST_LINE = /^\s*(?:[-*+]|\d+[.)])(?:\s|$)/;

export interface ChecklistProgress {
  done: number;
  total: number;
}

/** Ticked and all task items, outside fenced code blocks. */
export function checklistProgress(text: string): ChecklistProgress {
  let done = 0;
  let total = 0;
  forEachLine(text, (line) => {
    const match = TASK_LINE.exec(line);
    if (!match) return;
    total += 1;
    if (match[2] !== " ") done += 1;
  });
  return { done, total };
}

/** Calls `visit` for each line outside fenced code blocks (``` or ~~~), with its index. */
function forEachLine(text: string, visit: (line: string, index: number) => void): void {
  let fence: string | null = null;
  text.split(/\r?\n/).forEach((line, index) => {
    const marker = /^\s*(`{3,}|~{3,})/.exec(line)?.[1];
    if (marker !== undefined) {
      if (fence === null) fence = marker;
      else if (marker[0] === fence[0] && marker.length >= fence.length) fence = null;
      return;
    }
    if (fence === null) visit(line, index);
  });
}

/** Is line `index` (0-based) a task item outside a code block? */
export function isTaskLine(text: string, index: number): boolean {
  let found = false;
  forEachLine(text, (line, at) => {
    if (at === index && TASK_LINE.test(line)) found = true;
  });
  return found;
}

/**
 * Ticks or unticks the task on line `index` (0-based). Other lines, line endings and the rest of
 * the line stay untouched; a line that is no task returns the text unchanged.
 */
export function toggleTaskAtLine(text: string, index: number): string {
  if (!isTaskLine(text, index)) return text;
  const parts = text.split(/(\r?\n)/);
  // `split` with a capture group keeps the separators at odd positions.
  const position = index * 2;
  const line = parts[position];
  if (line === undefined) return text;
  parts[position] = line.replace(TASK_LINE, (_all, before: string, mark: string) =>
    mark === " " ? `${before}[x]` : `${before}[ ]`,
  );
  return parts.join("");
}

/** Offset in `line` of the character inside `[ ]`, or `null` if it is no task line. */
export function taskMarkOffset(line: string): number | null {
  const match = TASK_LINE.exec(line);
  return match?.[1] === undefined ? null : match[1].length + 1;
}

export function isListLine(line: string): boolean {
  return LIST_LINE.test(line);
}

/** Words in the text, as the UI language separates them (CJK without spaces counts per word). */
export function wordCount(text: string, language: string): number {
  if (text.trim() === "") return 0;
  try {
    const segmenter = new Intl.Segmenter(language, { granularity: "word" });
    let count = 0;
    for (const part of segmenter.segment(text)) if (part.isWordLike === true) count += 1;
    return count;
  } catch {
    return text.split(/\s+/).filter((word) => /[\p{L}\p{N}]/u.test(word)).length;
  }
}

/** Longest excerpt below the title in the list. */
export const EXCERPT_LENGTH = 120;

/**
 * Preview text for the list: the content without the first line that names the note, without
 * Markdown characters, whitespace collapsed, at most EXCERPT_LENGTH characters.
 */
export function noteExcerpt(text: string, title: string): string {
  const lines = text
    .split(/\r?\n/)
    .map((raw) =>
      raw
        .replace(/^\s*(?:#{1,6}\s+|>\s*|[-*+]\s+(?:\[[ xX]\]\s+)?|\d+[.)]\s+)*/, "")
        .replace(/^(`{3,}|~{3,}).*$/, "")
        .replace(/[*_`~]+/g, "")
        .trim(),
    )
    .filter((line) => line !== "" && !/^[-=#>|]+$/.test(line));
  if (lines[0] !== undefined && lines[0].toLowerCase() === title.trim().toLowerCase()) {
    lines.shift();
  }
  const joined = lines.join(" ").replace(/\s+/g, " ").trim();
  const chars = Array.from(joined);
  return chars.length > EXCERPT_LENGTH
    ? `${chars
        .slice(0, EXCERPT_LENGTH - 1)
        .join("")
        .trimEnd()}…`
    : joined;
}

/** A code fence that does not occur in `text`, so the captured output cannot end it early. */
function fenceFor(text: string): string {
  let fence = "```";
  while (text.includes(fence)) fence += "`";
  return fence;
}

/** C0 and C1 controls, DEL, line and paragraph separators. */
// eslint-disable-next-line no-control-regex -- control characters are exactly the target here.
const HEADING_BREAKERS = new RegExp("[\u0000-\u001f\u007f-\u009f\u2028\u2029]", "g");

/**
 * A session title as plain text inside the italic heading: the program sets the title (OSC 0
 * and 2), so control characters and line breaks become spaces and Markdown characters are
 * escaped. Otherwise a title could end the heading or add lines to the note.
 */
function headingText(source: string): string {
  return source
    .replace(HEADING_BREAKERS, " ")
    .replace(/\s+/g, " ")
    .trim()
    .replace(/[\\_*`[\]]/g, "\\$&");
}

/**
 * Adds terminal output at the end of a note: a line with time and source, then the text as a
 * code block (spacing and symbols of terminal output stay as they are). `existing` may be empty.
 */
export function appendCapture(
  existing: string,
  captured: string,
  stamp: string,
  source: string,
): string {
  const body = captured.replace(/\r\n?/g, "\n").replace(/\s+$/, "");
  const fence = fenceFor(body);
  const label = headingText(source);
  const heading = label === "" ? `_${stamp}_` : `_${stamp} · ${label}_`;
  const block = `${heading}\n\n${fence}\n${body}\n${fence}\n`;
  const trimmed = existing.replace(/\s+$/, "");
  return trimmed === "" ? block : `${trimmed}\n\n${block}`;
}
