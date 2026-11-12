// Characters that change how text looks without being visible themselves: bidi controls (the
// "Trojan Source" trick, CVE-2021-42574), zero-width characters, the byte order mark and C0
// controls. Text from git (diffs, file and branch names) shows them as visible placeholders, so a
// line that reads one way cannot run another.

// Bidi embeddings and overrides, isolates, zero-width and direction marks, the Arabic letter
// mark, the word joiner and invisible operators, line and paragraph separators, the byte order
// mark, C0 controls except tab, DEL and C1 controls.
const CLASS =
  "[\\u202a-\\u202e\\u2066-\\u2069\\u200b-\\u200f\\u061c\\u2060-\\u2064\\u2028\\u2029\\ufeff" +
  "\\u0000-\\u0008\\u000a-\\u001f\\u007f-\\u009f]";
const INVISIBLE = new RegExp(CLASS);
const INVISIBLE_ALL = new RegExp(CLASS, "g");

export interface TextSegment {
  text: string;
  /** A placeholder such as `<U+202E>` standing for one invisible character. */
  hidden: boolean;
}

export function hasInvisible(text: string): boolean {
  return INVISIBLE.test(text);
}

/** `<U+202E>`: the code point in four hex digits. */
export function placeholder(char: string): string {
  const code = char.codePointAt(0) ?? 0;
  return `<U+${code.toString(16).toUpperCase().padStart(4, "0")}>`;
}

/** Text with every invisible character replaced by its placeholder. */
export function revealInvisible(text: string): string {
  return text.replace(INVISIBLE_ALL, placeholder);
}

/** The same as segments, so the UI can render placeholders apart from the real text. */
export function invisibleSegments(text: string): TextSegment[] {
  if (!hasInvisible(text)) return [{ text, hidden: false }];
  const segments: TextSegment[] = [];
  let last = 0;
  for (const match of text.matchAll(INVISIBLE_ALL)) {
    const at = match.index;
    if (at > last) segments.push({ text: text.slice(last, at), hidden: false });
    segments.push({ text: placeholder(match[0]), hidden: true });
    last = at + match[0].length;
  }
  if (last < text.length) segments.push({ text: text.slice(last), hidden: false });
  return segments;
}

/**
 * Invisible characters the CodeMirror views (notes, file viewer) draw as a marker, so a text
 * cannot read one way and run another. CodeMirror's default set (@codemirror/view 6.43) already has
 * C0 and C1 controls, U+200B, U+200E, U+200F, U+202D, U+202E, U+2066, U+2067, U+2069 and the
 * byte order mark; this adds the other bidi embeddings and isolates (U+202A to U+202C, U+2068)
 * and the word joiner and invisible operators (U+2060 to U+2064), the rest of src/invisible.ts.
 * U+200C and U+200D stay unmarked: they join emoji and letters in Persian or Indic scripts, and
 * a marker would break that text. The send warning (appPaste.ts) still names them.
 */
export const MORE_SPECIAL_CHARS = /[\u202a-\u202c\u2068\u2060-\u2064]/;
