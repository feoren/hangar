// Snippet variables: `{{file}}`, `{{branch}}`, `{{selection}}`, `{{diff}}` and `{{folder}}` in a
// snippet (or a text sent to several sessions) are filled per session right before sending. A
// known variable without a value stops the whole text: half-filled prompts are never sent. Other
// `{{names}}` stay as written, so Jinja or Mustache examples in a prompt keep working. Pure;
// reading the values from git, the terminal and the file tree is the caller's job.

export const SNIPPET_VARIABLES = ["file", "branch", "selection", "diff", "folder"] as const;

export type SnippetVariable = (typeof SNIPPET_VARIABLES)[number];

/** Values found for one session; a missing or blank entry counts as unavailable. */
export type VariableValues = Partial<Record<SnippetVariable, string>>;

/** `{{ name }}` with optional inner spaces; names are word characters and dashes. */
const TOKEN = /\{\{\s*([A-Za-z_][\w-]*)\s*\}\}/g;

/** Characters of `{{diff}}`; a larger diff is cut at a line break and says so. */
export const DIFF_MAX_CHARS = 12_000;

export function isSnippetVariable(name: string): name is SnippetVariable {
  return SNIPPET_VARIABLES.some((known) => known === name);
}

export interface UsedVariables {
  /** Known variables in order of first use, each once. */
  known: SnippetVariable[];
  /** Unknown names in order of first use, each once. */
  unknown: string[];
}

/** The variables a text uses. Matching ignores case, so `{{Branch}}` is `branch`. */
export function usedVariables(text: string): UsedVariables {
  const known: SnippetVariable[] = [];
  const unknown: string[] = [];
  for (const match of text.matchAll(TOKEN)) {
    const raw = match[1] ?? "";
    const name = raw.toLowerCase();
    if (isSnippetVariable(name)) {
      if (!known.includes(name)) known.push(name);
    } else if (!unknown.includes(raw)) {
      unknown.push(raw);
    }
  }
  return { known, unknown };
}

export function hasVariables(text: string): boolean {
  const used = usedVariables(text);
  return used.known.length > 0 || used.unknown.length > 0;
}

export type Expansion = { ok: true; text: string } | { ok: false; missing: SnippetVariable[] };

/**
 * Fills every known variable in one pass: a value that itself contains `{{…}}` stays as it is,
 * and unknown names stay as written. Fails with all missing names when a known variable has no
 * value.
 */
export function expandVariables(text: string, values: VariableValues): Expansion {
  const used = usedVariables(text);
  const missing = used.known.filter((name) => (values[name] ?? "").trim() === "");
  if (missing.length > 0) return { ok: false, missing };
  const filled = text.replace(TOKEN, (token, raw: string) => {
    const name = raw.toLowerCase();
    return isSnippetVariable(name) ? (values[name] ?? token) : token;
  });
  return { ok: true, text: filled };
}

/**
 * The diff as `{{diff}}` uses it: whole when it fits, otherwise cut after the last full line
 * within `max` characters, followed by `note(count)` naming the lines left out.
 */
export function capDiff(
  diff: string,
  note: (omittedLines: number) => string,
  max = DIFF_MAX_CHARS,
): string {
  const text = diff.replace(/\s+$/, "");
  if (text.length <= max) return text;
  const head = text.slice(0, max);
  const lastBreak = head.lastIndexOf("\n");
  const kept = lastBreak > 0 ? head.slice(0, lastBreak) : head;
  const total = text.split("\n").length;
  const omitted = Math.max(1, total - kept.split("\n").length);
  return `${kept}\n${note(omitted)}`;
}

/**
 * What stopped a text, for the notice: each missing variable with why it has no value, e.g.
 * "{{file}}: no file chosen, {{diff}}: no changes".
 */
export function missingSummary(
  missing: readonly SnippetVariable[],
  reason: (name: SnippetVariable) => string,
): string {
  return missing.map((name) => `{{${name}}}: ${reason(name)}`).join(", ");
}
