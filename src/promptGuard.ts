// Whether the screen of an agent that went quiet shows a question instead of its idle input
// prompt. A permission dialog (Claude Code: "Do you want to proceed?", Codex: "Would you like to
// run the following command?") usually rings no bell, so the attention tracker sees it as "done".
// A queued message pasted there with Enter would confirm the preselected answer, mostly "yes".
// So before a queued message goes out, the bottom of the screen is checked; a match keeps the
// message ("asks"). A false match only keeps it longer, which is the safe direction.
//
// Patterns and their evidence: docs/wiki/pages/syntheses/queued-messages-and-stall-hint.md.
// Pure, so it can be tested with synthetic screens.

/** Only the newest lines count: an old dialog further up was answered long ago. */
export const GUARD_LINES = 16;

/**
 * A choice row with a selection pointer in front: "❯ 1. Yes", "› 2. No". Not "●": Claude Code
 * starts every answer with it, so a numbered answer would hold the message back; Gemini's dialogs
 * that use it are caught by their question phrase.
 */
const POINTED_CHOICE = /^[\s│┃|]*[❯›>▶►▸→*]\s*\d{1,2}[.)]\s+\S/u;
/** Any numbered choice row: "  2. Yes, and don't ask again". */
const CHOICE = /^[\s│┃|]*(?:[❯›>▶►▸●→*]\s*)?\d{1,2}[.)]\s+\S/u;

/** Questions and answer shapes of agent dialogs and classic terminal prompts. */
const QUESTION_PHRASES: readonly RegExp[] = [
  /\bdo you want to\b/iu,
  /\bwould you like to\b/iu,
  /\b(?:allow|approve)\b[^?]{0,80}\?/iu,
  /\ballow (?:once|always|execution)\b/iu,
  /\bapply this change\b/iu,
  /\bneeds your approval\b/iu,
  /\byes, and don't ask again\b/iu,
  /\bno, and tell \w+ what to do differently\b/iu,
  /\((?:y\/n|yes\/no)\)/iu,
  /\[(?:y\/n|yes\/no)\]/iu,
  /\besc to cancel\b/iu,
];

/** The last `count` lines that have text, trailing spaces removed. */
function tail(lines: readonly string[], count: number): string[] {
  const filled = lines.map((line) => line.trimEnd()).filter((line) => line.trim() !== "");
  return filled.slice(-count);
}

/**
 * The newest screen lines show a question or a choice: a numbered list with a selection pointer
 * on one row and at least one more choice row, or a known question phrase.
 */
export function looksLikeQuestion(lines: readonly string[]): boolean {
  const recent = tail(lines, GUARD_LINES);
  if (recent.some((line) => QUESTION_PHRASES.some((phrase) => phrase.test(line)))) return true;
  const pointed = recent.some((line) => POINTED_CHOICE.test(line));
  const choices = recent.filter((line) => CHOICE.test(line)).length;
  return pointed && choices >= 2;
}
