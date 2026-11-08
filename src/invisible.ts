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
