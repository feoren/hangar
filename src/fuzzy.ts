// Fuzzy matching for the launcher. Every word of the query must appear in the text as a
// subsequence (letters in order, gaps allowed); word starts and consecutive letters score higher,
// so "hc" finds "Hangar Claude" and "hangar claude" finds "Claude Code in hangar".
// Small and dependency free: texts are short (titles, folder names), so a quadratic search is fine.

export interface FuzzyMatch {
  score: number;
  /** Indices of the matched characters in the text, ascending, for highlighting. */
  positions: number[];
}

const MATCH = 1;
const WORD_START = 7;
const TEXT_START = 4;
const CONSECUTIVE = 8;
const GAP = 2;
/** Separators after which a new word starts. */
const SEPARATOR = /[\s\\/\-_.:,()[\]]/;

function isWordStart(text: string, index: number): boolean {
  if (index === 0) return true;
  const previous = text[index - 1] ?? "";
  if (SEPARATOR.test(previous)) return true;
  const current = text[index] ?? "";
  // camelCase and PascalCase: "openNotes" starts a word at "N".
  return (
    previous.toLowerCase() === previous &&
    previous.toUpperCase() !== previous &&
    current.toUpperCase() === current &&
    current.toLowerCase() !== current
  );
}

/**
 * Lowercase per UTF-16 unit, so indices stay aligned with the text even for characters whose
 * lowercase form is longer (Turkish dotted I).
 */
function fold(text: string): string[] {
  return Array.from({ length: text.length }, (_, index) => {
    const char = text[index] ?? "";
    const lower = char.toLowerCase();
    return lower.length === 1 ? lower : char;
  });
}

/** Best match of one word (no spaces) in the text, `null` if its letters do not appear in order. */
export function matchWord(word: string, text: string): FuzzyMatch | null {
  const query = fold(word);
  const haystack = fold(text);
  const m = query.length;
  const n = haystack.length;
  if (m === 0) return { score: 0, positions: [] };
  if (m > n) return null;
  // Cheap rejection first: most texts (e.g. thousands of file paths) do not contain the letters
  // in order at all, and the scoring below allocates per row.
  let next = 0;
  for (let j = 0; j < n && next < m; j += 1) {
    if (haystack[j] === query[next]) next += 1;
  }
  if (next < m) return null;
  // best[i][j]: best score with query[i] matched at text[j]; from[i][j]: where query[i-1] sat.
  const best: number[][] = [];
  const from: number[][] = [];
  for (let i = 0; i < m; i += 1) {
    const row = new Array<number>(n).fill(Number.NEGATIVE_INFINITY);
    const back = new Array<number>(n).fill(-1);
    // Running maximum of the previous row up to j - 2, for matches after a gap.
    let gapBest = Number.NEGATIVE_INFINITY;
    let gapFrom = -1;
    for (let j = 0; j < n; j += 1) {
      if (i > 0 && j >= 2) {
        const candidate = best[i - 1]?.[j - 2] ?? Number.NEGATIVE_INFINITY;
        if (candidate > gapBest) {
          gapBest = candidate;
          gapFrom = j - 2;
        }
      }
      if (haystack[j] !== query[i]) continue;
      const bonus = MATCH + (isWordStart(text, j) ? WORD_START : 0) + (j === 0 ? TEXT_START : 0);
      if (i === 0) {
        // Small penalty for a late start, so earlier matches win ties.
        row[j] = bonus - j * 0.01;
        continue;
      }
      const adjacent =
        j >= 1 ? (best[i - 1]?.[j - 1] ?? Number.NEGATIVE_INFINITY) : Number.NEGATIVE_INFINITY;
      const viaAdjacent = adjacent + CONSECUTIVE;
      const viaGap = gapBest - GAP;
      if (viaAdjacent >= viaGap && Number.isFinite(viaAdjacent)) {
        row[j] = viaAdjacent + bonus;
        back[j] = j - 1;
      } else if (Number.isFinite(viaGap)) {
        row[j] = viaGap + bonus;
        back[j] = gapFrom;
      }
    }
    best.push(row);
    from.push(back);
  }
  const last = best[m - 1] ?? [];
  let end = -1;
  let score = Number.NEGATIVE_INFINITY;
  for (let j = 0; j < n; j += 1) {
    const value = last[j] ?? Number.NEGATIVE_INFINITY;
    if (value > score) {
      score = value;
      end = j;
    }
  }
  if (end < 0) return null;
  const positions: number[] = [];
  for (let i = m - 1, j = end; i >= 0 && j >= 0; i -= 1) {
    positions.push(j);
    j = from[i]?.[j] ?? -1;
  }
  positions.reverse();
  return { score, positions };
}

/**
 * Matches every word of the query (split at spaces) against the text. All words must match;
 * their scores add up. An empty query matches everything with score 0.
 */
export function fuzzyMatch(query: string, text: string): FuzzyMatch | null {
  const words = query.trim().split(/\s+/).filter(Boolean);
  let score = 0;
  const positions = new Set<number>();
  for (const word of words) {
    const match = matchWord(word, text);
    if (match === null) return null;
    score += match.score;
    for (const position of match.positions) positions.add(position);
  }
  return { score, positions: [...positions].sort((a, b) => a - b) };
}

/** Splits the text into plain and matched runs for rendering with `<mark>`. */
export function highlightRuns(
  text: string,
  positions: readonly number[],
): { text: string; match: boolean }[] {
  const marked = new Set(positions);
  const runs: { text: string; match: boolean }[] = [];
  for (let index = 0; index < text.length; index += 1) {
    const match = marked.has(index);
    const last = runs[runs.length - 1];
    const char = text[index] ?? "";
    if (last?.match === match) last.text += char;
    else runs.push({ text: char, match });
  }
  return runs;
}
