// Snippet variables: `{{file}}`, `{{branch}}`, `{{selection}}`, `{{diff}}` and `{{folder}}` in a
// snippet (or a text sent to several sessions) are filled per session right before sending. A
// known variable without a value stops the whole text: half-filled prompts are never sent. Other
// `{{names}}` stay as written, so Jinja or Mustache examples in a prompt keep working. Pure;
// reading the values from git, the terminal and the file tree is the caller's job.

import { mentionPath, quotePath, wslPath, type ShellKind } from "./pathQuote";

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

export type Expansion =
  | { ok: true; text: string }
  | { ok: false; missing: SnippetVariable[] }
  | { ok: false; unsafe: SnippetVariable[] };

/**
 * The quoting the snippet's own text puts around a variable, as the shell reads it: none (Hangar
 * quotes the value), inside the snippet's single or double quotes (the value goes in as it is),
 * or `unknown` where the reading below does not follow the shell.
 */
type Quoting = "none" | "single" | "double" | "unknown";

/** Where a known variable's token sits in the snippet. */
interface Span {
  start: number;
  end: number;
}

/** PowerShell reads the typographic quotes as quotes too. */
const PS_SINGLE = "'\u2018\u2019\u201a\u201b";
const PS_DOUBLE = '"\u201c\u201d\u201e';

/** A `#` that starts a word starts a comment (bash, PowerShell; zsh only with an option). */
function startsWord(text: string, at: number): boolean {
  return at === 0 || /[\s;&|()<>]/.test(text[at - 1] ?? "");
}

/**
 * The quoting at each span, read from the snippet with the rules of `rules`. What the reading
 * does not follow makes the rest of the text `unknown`: an escape or `$` right before a variable,
 * backticks and `${` in POSIX shells, `$(` inside double quotes (its inside is quoted anew),
 * here-documents and here-strings, ANSI-C quotes, PowerShell's `--%`, and comments, whose quotes
 * mean nothing to one shell and something to another.
 */
function quotingAt(
  text: string,
  spans: readonly Span[],
  rules: "posix" | "powershell" | "cmd",
): Quoting[] {
  const result: Quoting[] = [];
  let state: Quoting = "none";
  let next = 0;
  const spanAt = (at: number): Span | undefined => {
    const span = spans[next];
    return span?.start === at ? span : undefined;
  };
  let i = 0;
  while (i < text.length) {
    const span = spanAt(i);
    if (span !== undefined) {
      result.push(state);
      next += 1;
      i = span.end;
      continue;
    }
    const c = text[i] ?? "";
    const after = text[i + 1] ?? "";
    i += 1;
    if (state === "unknown") continue;
    if (rules === "cmd") {
      if (state === "none" && c === "^") {
        if (spanAt(i) !== undefined) state = "unknown";
        else i += 1;
      } else if (c === '"') {
        state = state === "none" ? "double" : "none";
      }
      continue;
    }
    if (state === "single") {
      if (rules === "posix" ? c === "'" : PS_SINGLE.includes(c)) state = "none";
      continue;
    }
    const escape = rules === "posix" ? "\\" : "`";
    if (c === escape) {
      if (spanAt(i) !== undefined) state = "unknown";
      else i += 1;
      continue;
    }
    if (c === "$") {
      const nested = after === "(" && state === "double";
      if (
        spanAt(i) !== undefined ||
        nested ||
        after === "{" ||
        (rules === "posix" && after === "'")
      ) {
        state = "unknown";
      }
      continue;
    }
    if (state === "double") {
      if (rules === "posix" ? c === '"' : PS_DOUBLE.includes(c)) state = "none";
      else if (rules === "posix" && c === "`") state = "unknown";
      continue;
    }
    // Outside quotes.
    if (c === "#" && startsWord(text, i - 1)) state = "unknown";
    else if (rules === "posix") {
      if (c === "'") state = "single";
      else if (c === '"') state = "double";
      else if (c === "`" || (c === "<" && after === "<")) state = "unknown";
    } else if (PS_SINGLE.includes(c)) state = "single";
    else if (PS_DOUBLE.includes(c)) state = "double";
    else if (c === "@" && (PS_SINGLE.includes(after) || PS_DOUBLE.includes(after))) {
      state = "unknown";
    } else if (c === "-" && text.startsWith("-%", i)) state = "unknown";
  }
  return result;
}

/**
 * A cmd profile often starts PowerShell, which reads the same line with its own rules. The value
 * goes in quoted only where both shells agree it is quoted (`unknown` otherwise); `"` around it
 * reads as a string to both.
 */
function cmdQuoting(cmd: Quoting, powershell: Quoting): Quoting {
  if (cmd === "unknown" || powershell === "unknown") return "unknown";
  if (cmd === "none" && powershell === "double") return "unknown";
  if (cmd === "double" && powershell === "none") return "unknown";
  return cmd;
}

/** The quoting at each span for a shell of `kind`. */
function shellQuoting(
  text: string,
  spans: readonly Span[],
  kind: Exclude<ShellKind, "agent">,
): Quoting[] {
  if (kind !== "cmd") return quotingAt(text, spans, kind === "powershell" ? "powershell" : "posix");
  const powershell = quotingAt(text, spans, "powershell");
  return quotingAt(text, spans, "cmd").map((q, index) =>
    cmdQuoting(q, powershell[index] ?? "unknown"),
  );
}

/**
 * Letters, marks and digits of any script, spaces and `_ - . , : / \ @ + = ~`: nothing a shell
 * reads as code, wherever it stands. The rule where the snippet's quoting is `unknown`.
 */
const PLAIN_VALUE = /^[\p{L}\p{M}\p{N} _\-.,:/\\@+=~]*$/u;
/** The same without `\`, which escapes in POSIX shells. */
const PLAIN_POSIX_VALUE = /^[\p{L}\p{M}\p{N} _\-.,:/@+=~]*$/u;

/**
 * What ends or breaks open the snippet's own quotes. POSIX: `\` because fish escapes `\'` inside
 * single quotes, `!` because bash expands history inside double quotes (csh even inside single
 * ones). PowerShell: its typographic quotes count.
 */
const BREAKS_QUOTES = {
  posix: { single: /['\\!]/, double: /["$`\\!]/ },
  powershell: { single: /['\u2018-\u201b]/, double: /["\u201c-\u201e$`]/ },
} as const;

/**
 * What cmd cannot hold in double quotes (`"`, `%`, `!` with delayed expansion, line breaks) and
 * what a PowerShell behind it would expand or end its quotes with.
 */
const CMD_BREAKS = /["%!$`'\u2018-\u201e\r\n]/;

/** Characters a POSIX shell never needs quotes for. */
const POSIX_PLAIN_WORD = /^[\p{L}\p{N}_\-.:/]+$/u;

/**
 * One word for bash, zsh, fish and csh alike: single quotes, with `'`, `\` and `!` each escaped
 * outside them (fish escapes inside single quotes, csh expands `!` there).
 */
function posixWord(value: string): string {
  if (POSIX_PLAIN_WORD.test(value)) return value;
  return `'${value.replace(/['\\!]/g, (c) => `'\\${c}'`)}'`;
}

/** The value as it goes into the snippet at a place with `quoting`, or `null` when it cannot. */
function shellWord(
  value: string,
  kind: Exclude<ShellKind, "agent">,
  quoting: Quoting,
): string | null {
  const posix = kind === "posix" || kind === "wsl";
  if (quoting === "unknown") {
    return (posix ? PLAIN_POSIX_VALUE : PLAIN_VALUE).test(value) ? value : null;
  }
  if (kind === "cmd") {
    if (CMD_BREAKS.test(value)) return null;
    return quoting === "none" ? quotePath(value, "cmd") : value;
  }
  if (quoting === "none") return posix ? posixWord(value) : quotePath(value, "powershell");
  return BREAKS_QUOTES[posix ? "posix" : "powershell"][quoting].test(value) ? null : value;
}

/**
 * The value as the shell names it: a folder in WSL as `/mnt/<drive>/...`, a Windows folder in a
 * POSIX shell (Git Bash) with `/`, which needs no escaping there.
 */
function nativeValue(name: SnippetVariable, value: string, kind: ShellKind): string {
  if (name !== "folder") return value;
  if (kind === "wsl") return wslPath(value);
  if (kind === "posix" && /^([a-z]:[\\/]|\\\\)/i.test(value)) {
    return value.replace(/\\/g, "/");
  }
  return value;
}

/**
 * Fills every known variable in one pass: a value that itself contains `{{…}}` stays as it is,
 * and unknown names stay as written. Fails with all missing names when a known variable has no
 * value.
 *
 * For a shell (`kind` other than `agent`) a value is one word, never code. Where the snippet
 * leaves the variable unquoted, Hangar quotes the value with the shell's rules, so any character
 * may stand in it (cmd: not `"`, `%`, `!`, line breaks and what a PowerShell behind it expands).
 * Inside the snippet's own quotes the value goes in as it is, and fails when it holds what would
 * end or break open those quotes; where the snippet's quoting cannot be read safely, only plain
 * values pass (`PLAIN_VALUE`). A failing value fails with the names of all such variables, and
 * the text is not sent.
 */
export function expandVariables(
  text: string,
  values: VariableValues,
  kind: ShellKind = "agent",
): Expansion {
  const used = usedVariables(text);
  const missing = used.known.filter((name) => (values[name] ?? "").trim() === "");
  if (missing.length > 0) return { ok: false, missing };
  if (kind === "agent") {
    const filled = text.replace(TOKEN, (token, raw: string) => {
      const name = raw.toLowerCase();
      return isSnippetVariable(name) ? (values[name] ?? token) : token;
    });
    return { ok: true, text: filled };
  }
  const tokens = [...text.matchAll(TOKEN)].flatMap((match) => {
    const name = (match[1] ?? "").toLowerCase();
    if (!isSnippetVariable(name)) return [];
    return [{ name, start: match.index, end: match.index + match[0].length }];
  });
  const quoting = shellQuoting(text, tokens, kind);
  const unsafe = new Set<SnippetVariable>();
  const words = tokens.map(({ name }, index) => {
    const value = nativeValue(name, values[name] ?? "", kind);
    const word = shellWord(value, kind, quoting[index] ?? "unknown");
    if (word === null) unsafe.add(name);
    return word ?? "";
  });
  if (unsafe.size > 0) return { ok: false, unsafe: used.known.filter((name) => unsafe.has(name)) };
  let filled = "";
  let from = 0;
  tokens.forEach((span, index) => {
    filled += text.slice(from, span.start) + (words[index] ?? "");
    from = span.end;
  });
  return { ok: true, text: filled + text.slice(from) };
}

/**
 * `{{file}}` for a session: an @ mention for an agent (as the Files tab inserts it), the plain
 * path for a shell with its own separators, which `expandVariables` then quotes. `path` is
 * relative to the session's folder with `/` between names; one starting with `-` gets `./` in
 * front, so a command does not read it as an option.
 */
export function fileValue(path: string, kind: ShellKind): string {
  if (kind === "agent") return `@${mentionPath(path)}`;
  const relative = path.startsWith("-") ? `./${path}` : path;
  return kind === "cmd" || kind === "powershell" ? relative.replace(/\//g, "\\") : relative;
}

/** The variables a shell would read as code, for the notice: "{{branch}}, {{folder}}". */
export function unsafeSummary(unsafe: readonly SnippetVariable[]): string {
  return unsafe.map((name) => `{{${name}}}`).join(", ");
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
