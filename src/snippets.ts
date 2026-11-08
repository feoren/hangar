// Snippets: short stored texts that go into the active session as if pasted (bracketed paste,
// no Enter). Stored with the settings; the defaults come in the UI language of the first start.

import { translate, type Language, type MessageKey } from "./i18n";

export interface Snippet {
  /** Stable within the list, for React keys and reordering. */
  id: string;
  text: string;
}

/** Protects storage and the menus against a broken or hand-edited value. */
export const SNIPPET_LIMIT = 50;
export const SNIPPET_MAX_LENGTH = 8000;
/** Label length in menus and the launcher. */
const LABEL_LENGTH = 60;

const DEFAULT_KEYS: readonly MessageKey[] = [
  "snippets.defaultReview",
  "snippets.defaultTests",
  "snippets.defaultExplain",
];

export function defaultSnippets(language: Language): Snippet[] {
  return DEFAULT_KEYS.map((key, index) => ({
    id: `default-${String(index + 1)}`,
    text: translate(language, key),
  }));
}

const ID = /^[a-z0-9-]{1,40}$/;

/**
 * Reads the stored list. `undefined` (never stored) gives the defaults; an empty list stays empty,
 * because the user deleted them. Invalid entries are dropped, missing or duplicate IDs replaced.
 */
export function parseSnippets(value: unknown, language: Language): Snippet[] {
  if (value === undefined) return defaultSnippets(language);
  if (!Array.isArray(value)) return defaultSnippets(language);
  const result: Snippet[] = [];
  const ids = new Set<string>();
  for (const entry of value) {
    if (result.length >= SNIPPET_LIMIT) break;
    if (typeof entry !== "object" || entry === null) continue;
    const text: unknown = Reflect.get(entry, "text");
    const id: unknown = Reflect.get(entry, "id");
    if (typeof text !== "string" || text.trim() === "") continue;
    let key = typeof id === "string" && ID.test(id) && !ids.has(id) ? id : "";
    for (let number = result.length + 1; key === ""; number += 1) {
      const candidate = `snippet-${String(number)}`;
      if (!ids.has(candidate)) key = candidate;
    }
    ids.add(key);
    result.push({ id: key, text: text.slice(0, SNIPPET_MAX_LENGTH) });
  }
  return result;
}

/** New, unused ID for a snippet added in Settings. */
export function newSnippetId(existing: readonly Snippet[]): string {
  const ids = new Set(existing.map((snippet) => snippet.id));
  for (let number = existing.length + 1; ; number += 1) {
    const candidate = `snippet-${String(number)}`;
    if (!ids.has(candidate)) return candidate;
  }
}

/** First non-empty line, shortened: the name of the snippet in menus and the launcher. */
export function snippetLabel(snippet: Pick<Snippet, "text">): string {
  const line =
    snippet.text
      .split(/\r?\n/)
      .map((part) => part.trim())
      .find((part) => part !== "") ?? "";
  const chars = Array.from(line);
  return chars.length > LABEL_LENGTH ? `${chars.slice(0, LABEL_LENGTH - 1).join("")}…` : line;
}

/** Snippets that can be inserted (not empty). */
export function usableSnippets(snippets: readonly Snippet[]): Snippet[] {
  return snippets.filter((snippet) => snippet.text.trim() !== "");
}

/** Moves a snippet one place up (-1) or down (1); unchanged at the edges. */
export function moveSnippet(snippets: readonly Snippet[], id: string, step: 1 | -1): Snippet[] {
  const index = snippets.findIndex((snippet) => snippet.id === id);
  const target = index + step;
  if (index < 0 || target < 0 || target >= snippets.length) return [...snippets];
  const next = [...snippets];
  const [moved] = next.splice(index, 1);
  if (moved) next.splice(target, 0, moved);
  return next;
}
