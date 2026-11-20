// Quick access entries that come from a search or a lookup of their own: notes found by their
// text (the backend's full-text search, asked only while there is a query) and the projects of
// the running sessions (the cards of the project overview). Pure; App.tsx asks the backend.

import type { ReactNode } from "react";

import { folderName } from "./folders";
import { shortPath } from "./format";
import type { Translate } from "./i18n";
import type { LauncherItem } from "./LauncherDialog";
import { parseSearchHits, type SearchHit } from "./notes";
import {
  groupProjects,
  parseRepoPlaces,
  placePaths,
  type OverviewSession,
  type Project,
  type RepoPlace,
} from "./projectCards";

/** At most this many note hits: quick access shows a few per section anyway. */
export const NOTE_HITS = 10;

/** One whitespace-collapsed line of the excerpt, short enough for the dimmed detail. */
const EXCERPT_CHARS = 80;

function excerptLine(text: string): string {
  const line = text.replace(/\s+/g, " ").trim();
  return line.length <= EXCERPT_CHARS ? line : `${line.slice(0, EXCERPT_CHARS - 1)}…`;
}

/**
 * Notes whose text matches the query, in the backend's order: the title, then where it lives and
 * the passage that matched. They match the query even where their title does not (`found`).
 */
export function noteItems(
  hits: readonly SearchHit[],
  open: (hit: SearchHit) => void,
  t: Translate,
  icon: ReactNode,
): LauncherItem[] {
  return hits.slice(0, NOTE_HITS).map((hit) => {
    const place = hit.folder.path === null ? t("notes.general") : folderName(hit.folder.path);
    const excerpt = excerptLine(hit.excerpt);
    return {
      key: `note:${hit.folderKey}:${hit.id}`,
      section: "notes",
      label: hit.title.trim() === "" ? t("notes.untitled") : hit.title,
      detail: excerpt === "" ? place : `${place} · ${excerpt}`,
      found: true,
      icon,
      run: () => {
        open(hit);
      },
    };
  });
}

/**
 * The projects of the running sessions as the overview groups them, found by name, folder or the
 * titles of their sessions; choosing one shows its card.
 */
export function projectItems<T extends OverviewSession & { title: string }>(
  projects: readonly Project<T>[],
  open: (key: string) => void,
  icon: ReactNode,
): LauncherItem[] {
  return projects.map((project) => ({
    key: `project:${project.key}`,
    section: "projects",
    label: project.name,
    detail: shortPath(project.root),
    keywords: [project.root, ...project.sessions.map((session) => session.title)].join(" "),
    icon,
    run: () => {
      open(project.key);
    },
  }));
}

/** The searcher for quick access: the backend's note search (`api.searchNotes`), checked. */
export async function searchNoteItems(
  query: string,
  search: (query: string) => Promise<unknown>,
  open: (hit: SearchHit) => void,
  t: Translate,
  icon: ReactNode,
): Promise<LauncherItem[]> {
  const trimmed = query.trim();
  if (trimmed === "") return [];
  return noteItems(parseSearchHits(await search(trimmed)), open, t, icon);
}

/**
 * The loader for quick access: the projects as the overview's cards group them, from the
 * repository places of the sessions' folders (`api.gitRepoPlaces`). Without an answer every
 * folder counts as a plain one, as on the cards.
 */
export async function loadProjectItems(
  sessions: readonly (OverviewSession & { title: string })[],
  places: (paths: string[]) => Promise<unknown>,
  open: (key: string) => void,
  icon: ReactNode,
): Promise<LauncherItem[]> {
  const paths = placePaths(sessions);
  const none = new Map<string, RepoPlace | null>();
  const known = paths.length === 0 ? none : await places(paths).then(parseRepoPlaces, () => none);
  return projectItems(groupProjects(sessions, known), open, icon);
}
