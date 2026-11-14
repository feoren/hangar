// Project overview in the home view: one card per project folder of the running sessions, with
// their state, open changes, newest commits, team and dev servers. Pure: grouping the folders,
// assembling a card's data and limiting the git calls are tested without React or the backend.

import { folderKey, folderName } from "./folders";
import { parseChangedCount } from "./doneNotice";
import { parseGitLog, type CommitSummary } from "./gitHistory";
import { hasInvisible } from "./invisible";
import { repoTeammates, peersOf, type RepoTeammate } from "./repoPeers";
import type { TaskView, TeamActivity } from "./team";

/** The backend answers at most this many folders per call (`git_repo_places`). */
export const MAX_PLACE_PATHS = 64;
/** Commits a card lists. */
export const COMMITS_SHOWN = 3;
/** Reservations a card lists; more are counted. */
export const TASKS_SHOWN = 3;
/** Git calls of all cards together at one time, so a long list never starts dozens of processes. */
export const GIT_CALLS_AT_ONCE = 3;

/** Answer of `git_repo_places` for one folder (Rust: `RepoPlace`). */
export interface RepoPlace {
  /** The repository's main folder; a linked worktree names the folder it belongs to. */
  root: string;
  /** The checkout that holds the folder. */
  workTree: string;
}

function placeOf(value: unknown): RepoPlace | null {
  if (typeof value !== "object" || value === null) return null;
  const root: unknown = Reflect.get(value, "root");
  const workTree: unknown = Reflect.get(value, "workTree");
  if (typeof root !== "string" || typeof workTree !== "string") return null;
  if (root.trim() === "" || workTree.trim() === "") return null;
  if (hasInvisible(root) || hasInvisible(workTree)) return null;
  return { root, workTree };
}

/** Answer of `git_repo_places`: folder to place, `null` outside a repository; malformed left out. */
export function parseRepoPlaces(value: unknown): Map<string, RepoPlace | null> {
  const places = new Map<string, RepoPlace | null>();
  if (typeof value !== "object" || value === null || Array.isArray(value)) return places;
  for (const [path, entry] of Object.entries(value)) {
    if (entry === null) {
      places.set(path, null);
      continue;
    }
    const place = placeOf(entry);
    if (place !== null) places.set(path, place);
  }
  return places;
}

/** What the overview needs to know of a session. */
export interface OverviewSession {
  id: number;
  cwd: string;
  kind?: "agent" | "update" | undefined;
  worktree?: { path: string; repoRoot: string } | undefined;
}

function counts(session: OverviewSession): boolean {
  return session.kind !== "update" && session.cwd.trim() !== "";
}

/**
 * Folders to ask the backend about, each once, at most MAX_PLACE_PATHS: a session in its own
 * worktree already knows its repository, update sessions run in the home folder on purpose.
 */
export function placePaths(sessions: readonly OverviewSession[]): string[] {
  const paths: string[] = [];
  for (const session of sessions) {
    if (!counts(session) || session.worktree !== undefined || paths.includes(session.cwd)) {
      continue;
    }
    paths.push(session.cwd);
    if (paths.length === MAX_PLACE_PATHS) break;
  }
  return paths;
}

/** One checkout of a project and the sessions working in it. */
export interface WorkTree {
  path: string;
  sessionIds: number[];
}

/** One card: a repository with its worktrees, or a plain folder. */
export interface Project<T> {
  /** `folderKey` of `root`. */
  key: string;
  /** The repository's main folder, or the plain folder. */
  root: string;
  name: string;
  /** A git repository: changes and commits apply. */
  repo: boolean;
  /** In list order. */
  sessions: T[];
  /** The checkouts with sessions, in order of appearance; empty for a plain folder. */
  workTrees: WorkTree[];
}

/**
 * Cards in the order their first session appears: sessions in one repository (its main folder,
 * a subfolder, a worktree Hangar made or one made by hand) share a card. A folder the backend has
 * not answered for yet counts as a plain folder until it has.
 */
export function groupProjects<T extends OverviewSession>(
  sessions: readonly T[],
  places: ReadonlyMap<string, RepoPlace | null>,
): Project<T>[] {
  const projects = new Map<string, Project<T>>();
  for (const session of sessions) {
    if (!counts(session)) continue;
    const place: RepoPlace | null =
      session.worktree !== undefined
        ? { root: session.worktree.repoRoot, workTree: session.worktree.path }
        : (places.get(session.cwd) ?? null);
    const root = place?.root ?? session.cwd;
    const key = folderKey(root);
    let project = projects.get(key);
    if (project === undefined) {
      project = { key, root, name: folderName(root), repo: false, sessions: [], workTrees: [] };
      projects.set(key, project);
    }
    project.sessions.push(session);
    if (place === null) continue;
    project.repo = true;
    const treeKey = folderKey(place.workTree);
    const tree = project.workTrees.find((entry) => folderKey(entry.path) === treeKey);
    if (tree === undefined)
      project.workTrees.push({ path: place.workTree, sessionIds: [session.id] });
    else tree.sessionIds.push(session.id);
  }
  return [...projects.values()];
}

/** The team block of a card; `null` hides it. */
export interface ProjectTeam {
  teammates: RepoTeammate[];
  tasks: TaskView[];
}

/**
 * Teammates working in the card's repository and the reservations of the team projects its
 * sessions are shared with, each once. `null` without a team or without anything to show.
 */
export function projectTeam(
  activities: ReadonlyMap<string, TeamActivity>,
  sessionIds: readonly number[],
): ProjectTeam | null {
  const teammates = repoTeammates(sessionIds.flatMap((id) => peersOf(activities, id)));
  const tasks: TaskView[] = [];
  for (const activity of activities.values()) {
    if (!sessionIds.some((id) => activity.sessions.includes(id))) continue;
    for (const entry of activity.tasks) {
      if (!tasks.some((known) => known.task === entry.task)) tasks.push(entry);
    }
  }
  return teammates.length === 0 && tasks.length === 0 ? null : { teammates, tasks };
}

export interface ProjectDevServer {
  url: string;
  /** The first of the card's sessions that printed it: the globe opens through this one. */
  sessionId: number;
}

/** Dev server addresses of the card's sessions, each once, in session order. */
export function projectDevServers(
  sessionIds: readonly number[],
  devServers: ReadonlyMap<number, string>,
): ProjectDevServer[] {
  const servers: ProjectDevServer[] = [];
  for (const sessionId of sessionIds) {
    const url = devServers.get(sessionId);
    if (url !== undefined && !servers.some((server) => server.url === url)) {
      servers.push({ url, sessionId });
    }
  }
  return servers;
}

/**
 * Changes whenever one of the card's sessions finished a stretch of work (`finished`: times per
 * session), so the card asks git again then and only then.
 */
export function projectToken(
  sessionIds: readonly number[],
  finished: ReadonlyMap<number, number>,
): string {
  return sessionIds.map((id) => `${String(id)}:${String(finished.get(id) ?? 0)}`).join(",");
}

/** Changed files in one checkout. */
export interface TreeChanges {
  path: string;
  count: number;
}

export interface ProjectGit {
  /** The checkouts git answered for. */
  changes: TreeChanges[];
  /** Newest first, at most COMMITS_SHOWN. */
  commits: CommitSummary[];
}

export interface GitCalls {
  changedCount: (cwd: string) => Promise<unknown>;
  log: (cwd: string, skip: number, limit: number) => Promise<unknown>;
}

/**
 * What git says about a card: changed files per checkout (one `git status` each) and the newest
 * commits of the main folder. Whatever fails is left out; a plain folder asks nothing.
 */
export async function loadProjectGit(
  project: Pick<Project<unknown>, "repo" | "root" | "workTrees">,
  git: GitCalls,
): Promise<ProjectGit> {
  if (!project.repo) return { changes: [], commits: [] };
  const [counted, log] = await Promise.all([
    Promise.allSettled(project.workTrees.map((tree) => git.changedCount(tree.path))),
    git.log(project.root, 0, COMMITS_SHOWN).then(parseGitLog, () => null),
  ]);
  const changes: TreeChanges[] = [];
  counted.forEach((result, index) => {
    const tree = project.workTrees[index];
    if (result.status === "fulfilled" && tree !== undefined) {
      changes.push({ path: tree.path, count: parseChangedCount(result.value) });
    }
  });
  const commits = log !== null && log.isRepo ? log.commits.slice(0, COMMITS_SHOWN) : [];
  return { changes, commits };
}

/** Changed files of all checkouts; `null` when git answered for none. */
export function totalChanges(changes: readonly TreeChanges[]): number | null {
  if (changes.length === 0) return null;
  return changes.reduce((sum, entry) => sum + entry.count, 0);
}

/** The session whose Changes tab shows the card's changes: the first checkout with some. */
export function changesTarget(
  project: Pick<Project<unknown>, "workTrees">,
  changes: readonly TreeChanges[],
): number | null {
  const changed = changes.find((entry) => entry.count > 0);
  if (changed === undefined) return null;
  const key = folderKey(changed.path);
  const tree = project.workTrees.find((entry) => folderKey(entry.path) === key);
  return tree?.sessionIds[0] ?? null;
}

/**
 * Runs at most `max` tasks at a time, the others in the order they came. A failed task frees its
 * place like a finished one.
 */
export function createLimiter(max: number): <T>(task: () => Promise<T>) => Promise<T> {
  let running = 0;
  const waiting: (() => void)[] = [];
  const next = () => {
    running -= 1;
    waiting.shift()?.();
  };
  return <T>(task: () => Promise<T>) =>
    new Promise<T>((resolve, reject) => {
      const start = () => {
        running += 1;
        // Through a promise, so a task that throws at once still frees its place.
        Promise.resolve().then(task).then(resolve, reject).finally(next);
      };
      if (running < Math.max(1, max)) start();
      else waiting.push(start);
    });
}
