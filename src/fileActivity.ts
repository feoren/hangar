// Activity map of the Files tab: which files agents changed, and when. The app already observes
// every session's claims (claims.ts) and the teammates' presence (team.ts); comparing one snapshot
// with the one before gives change events, kept per project in a small, capped log
// (activityStore.ts persists it). From that log the tree gets a heat per file and folder for a
// time window, and the markings learn since when someone holds a file. Pure, tested in
// fileActivity.test.ts.

import type { Claim, ClaimsSnapshot } from "./claims";
import {
  ancestors,
  folderKeyOf,
  repoToTreeKey,
  rootOffset,
  type HolderRef,
  type OtherWorktree,
} from "./fileTree";
import { peersOf } from "./repoPeers";
import type { TeamActivity } from "./team";

const MINUTE_MS = 60_000;
const HOUR_MS = 60 * MINUTE_MS;
const DAY_MS = 24 * HOUR_MS;

/** The longest window: older events are dropped. */
export const KEEP_MS = 7 * DAY_MS;
/** Events kept per project; the oldest go first. */
export const MAX_EVENTS = 3000;
/** Projects kept; the one with the oldest last event goes first. */
export const MAX_PROJECTS = 12;
/** Holds remembered (for "since when"); the oldest go first. */
export const MAX_SEEN = 3000;

// ---------------------------------------------------------------------------------------------
// Projects and holders

/** A session of this device as the activity map needs it. */
export interface LocalSession {
  id: number;
  title: string;
  /** Name of its program (profile), empty when unknown. */
  program: string;
  /** The repository a Hangar worktree was made from (`Worktree.repoRoot`), else `null`. */
  repo: string | null;
}

/** The window's sessions as the activity map needs them, programs named by their profiles. */
export function localSessionsOf(
  sessions: readonly {
    id: number;
    title: string;
    profileId: string;
    worktree?: { repoRoot: string };
  }[],
  profiles: readonly { id: string; name: string }[],
): LocalSession[] {
  const names = new Map(profiles.map((profile) => [profile.id, profile.name]));
  return sessions.map((session) => ({
    id: session.id,
    title: session.title,
    program: names.get(session.profileId) ?? "",
    repo: session.worktree?.repoRoot ?? null,
  }));
}

/**
 * A project's key: the repository's main folder for a session in a worktree Hangar made, else
 * the session's own worktree root. Sessions in one repository meet under the same key; a worktree
 * made by hand outside Hangar counts as a project of its own. `null` outside a repository.
 */
export function projectKeyOf(repo: string | null, root: string | null): string | null {
  const folder = repo ?? root;
  return folder === null ? null : folderKeyOf(folder).key;
}

/** A holder as a string: one per teammate session, one per worktree of this device. */
export function holderKey(holder: HolderRef): string {
  return holder.kind === "team"
    ? `t:${holder.member}\n${holder.device}\n${holder.session}`
    : `w:${folderKeyOf(holder.root).key}`;
}

function seenKey(project: string, holder: string, path: string): string {
  return `${project}\u0000${holder}\u0000${path}`;
}

/**
 * The other worktrees of the active session's project with their sessions and claims: what the
 * tree marks as "another session of yours changes this". Sessions sharing the active session's
 * worktree see the same changes and stay out, as do sessions whose claims are not ready.
 */
export function otherWorktrees(
  snapshot: ClaimsSnapshot,
  sessions: readonly LocalSession[],
  activeId: number,
): OtherWorktree[] {
  const repoOf = new Map(sessions.map((session) => [session.id, session.repo]));
  const active = snapshot.sessions.find((entry) => entry.sessionId === activeId);
  const project = projectKeyOf(repoOf.get(activeId) ?? null, active?.root ?? null);
  if (project === null || active === undefined || active.root === null) return [];
  const own = folderKeyOf(active.root).key;
  const byRoot = new Map<string, OtherWorktree & { sessions: number[] }>();
  for (const entry of snapshot.sessions) {
    if (entry.sessionId === activeId || entry.root === null) continue;
    if (entry.state.kind !== "ready" || entry.claims.length === 0) continue;
    const root = folderKeyOf(entry.root).key;
    if (root === own) continue;
    if (projectKeyOf(repoOf.get(entry.sessionId) ?? null, entry.root) !== project) continue;
    const known = byRoot.get(root);
    if (known === undefined) {
      byRoot.set(root, { root: entry.root, sessions: [entry.sessionId], claims: entry.claims });
    } else {
      known.sessions.push(entry.sessionId);
    }
  }
  return [...byRoot.values()];
}

// ---------------------------------------------------------------------------------------------
// Observing

/** One file a holder holds right now. */
export interface Observed {
  project: string;
  holder: string;
  /** Repository relative, as git spells it. */
  path: string;
  /** Changes whenever the holder changes the file again (kind and lines); empty for teammates. */
  signature: string;
}

export interface SeenEntry {
  project: string;
  holder: string;
  path: string;
  signature: string;
  /** When the hold began (ms). */
  since: number;
}

export interface ActivityEvent {
  /** Repository relative, as git spells it. */
  path: string;
  /** When it was seen (ms). */
  at: number;
  /** A teammate's session, not one of this device. */
  team: boolean;
}

function claimSignature(claim: Claim): string {
  return [
    claim.kind,
    ...claim.ranges.map((range) => `${String(range.start)}-${String(range.end)}`),
  ].join(" ");
}

/**
 * What this device's sessions hold: every claim of a ready session, under its worktree. A session
 * still reading its changes (or failing to) keeps what it held before: its worktree comes back in
 * `keep`, so a slow scan does not count as a new change.
 */
export function localObservations(
  snapshot: ClaimsSnapshot,
  sessions: readonly LocalSession[],
): { observed: Observed[]; keep: Set<string> } {
  const repoOf = new Map(sessions.map((session) => [session.id, session.repo]));
  const observed: Observed[] = [];
  const keep = new Set<string>();
  const done = new Set<string>();
  for (const entry of snapshot.sessions) {
    if (entry.root === null) continue;
    const project = projectKeyOf(repoOf.get(entry.sessionId) ?? null, entry.root);
    if (project === null) continue;
    const holder = holderKey({ kind: "worktree", root: entry.root });
    if (entry.state.kind !== "ready") {
      keep.add(holder);
      continue;
    }
    // Sessions sharing a worktree report the same claims: once is enough.
    if (done.has(holder)) continue;
    done.add(holder);
    for (const claim of entry.claims) {
      observed.push({ project, holder, path: claim.path, signature: claimSignature(claim) });
    }
  }
  return { observed, keep };
}

/**
 * What teammates hold: every file of their sessions in the repository of one of this device's
 * sessions (`peersOf`), under that session's project.
 */
export function teamObservations(
  activities: ReadonlyMap<string, TeamActivity>,
  snapshot: ClaimsSnapshot,
  sessions: readonly LocalSession[],
): Observed[] {
  const observed: Observed[] = [];
  const rootOf = new Map(snapshot.sessions.map((entry) => [entry.sessionId, entry.root]));
  for (const session of sessions) {
    const project = projectKeyOf(session.repo, rootOf.get(session.id) ?? null);
    if (project === null) continue;
    for (const peer of peersOf(activities, session.id)) {
      for (const peerSession of peer.sessions) {
        const holder = holderKey({
          kind: "team",
          member: peer.member,
          device: peer.device,
          session: peerSession.key,
        });
        for (const path of peerSession.files) {
          observed.push({ project, holder, path, signature: "" });
        }
      }
    }
  }
  return observed;
}

/**
 * Compares what is held now with what was held before, for one kind of holder (`"w:"` this
 * device's worktrees, `"t:"` teammates): a file newly held, or held with other lines, is an event.
 * Holds of that kind that are gone are forgotten, unless their holder is in `keep`.
 */
export function observe(
  seen: ReadonlyMap<string, SeenEntry>,
  kind: "w:" | "t:",
  observed: readonly Observed[],
  keep: ReadonlySet<string>,
  now: number,
): { seen: Map<string, SeenEntry>; events: { project: string; event: ActivityEvent }[] } {
  const next = new Map<string, SeenEntry>();
  for (const [key, entry] of seen) {
    if (!entry.holder.startsWith(kind) || keep.has(entry.holder)) next.set(key, entry);
  }
  const events: { project: string; event: ActivityEvent }[] = [];
  const handled = new Set<string>();
  for (const item of observed) {
    if (!item.holder.startsWith(kind)) continue;
    const key = seenKey(item.project, item.holder, item.path);
    // The same hold twice in one snapshot (two sessions in one worktree) is one.
    if (handled.has(key)) continue;
    handled.add(key);
    const before = seen.get(key);
    next.set(key, {
      project: item.project,
      holder: item.holder,
      path: item.path,
      signature: item.signature,
      since: before?.since ?? now,
    });
    // Two holders on one file are two agents working: each counts.
    if (before?.signature === item.signature) continue;
    events.push({
      project: item.project,
      event: { path: item.path, at: now, team: kind === "t:" },
    });
  }
  return { seen: next, events };
}

/** Since when `holder` holds `path` in `project`, `null` when this device never saw it. */
export function sinceOf(
  seen: ReadonlyMap<string, SeenEntry>,
  project: string,
  holder: HolderRef,
  path: string,
): number | null {
  return seen.get(seenKey(project, holderKey(holder), path))?.since ?? null;
}

// ---------------------------------------------------------------------------------------------
// The event log

/** The activity map's change events and holds; not the sub-agent view's activity log. */
export interface EventLog {
  /** Per project key, oldest first. */
  projects: ReadonlyMap<string, readonly ActivityEvent[]>;
  seen: ReadonlyMap<string, SeenEntry>;
}

export const EMPTY_LOG: EventLog = { projects: new Map(), seen: new Map() };

/**
 * Adds events and keeps the log small: events older than KEEP_MS go, at most MAX_EVENTS per
 * project (the newest stay), at most MAX_PROJECTS projects (the most recently active stay) and
 * MAX_SEEN holds (the newest stay).
 */
export function appendEvents(
  log: EventLog,
  added: readonly { project: string; event: ActivityEvent }[],
  seen: ReadonlyMap<string, SeenEntry>,
  now: number,
): EventLog {
  // Nothing new: the same log, so nobody computes the heat again.
  if (added.length === 0) return sameSeen(log.seen, seen) ? log : { ...log, seen };
  const projects = new Map<string, ActivityEvent[]>();
  for (const [project, events] of log.projects) projects.set(project, [...events]);
  for (const { project, event } of added) {
    const list = projects.get(project);
    if (list === undefined) projects.set(project, [event]);
    else list.push(event);
  }
  return prune({ projects, seen }, now);
}

/** The caps of `appendEvents` alone, for a log just read. */
export function prune(log: EventLog, now: number): EventLog {
  const oldest = now - KEEP_MS;
  const kept: [string, ActivityEvent[]][] = [];
  for (const [project, events] of log.projects) {
    const sorted = [...events].filter((event) => event.at >= oldest);
    // Appended in time order as a rule; a clock set back must not break the binary search.
    sorted.sort((a, b) => a.at - b.at);
    if (sorted.length === 0) continue;
    kept.push([project, sorted.slice(Math.max(0, sorted.length - MAX_EVENTS))]);
  }
  kept.sort((a, b) => lastAt(b[1]) - lastAt(a[1]));
  const projects = new Map(kept.slice(0, MAX_PROJECTS));
  let seen = log.seen;
  if (seen.size > MAX_SEEN) {
    const newest = [...seen].sort((a, b) => b[1].since - a[1].since).slice(0, MAX_SEEN);
    seen = new Map(newest);
  }
  return { projects, seen };
}

function sameSeen(a: ReadonlyMap<string, SeenEntry>, b: ReadonlyMap<string, SeenEntry>): boolean {
  if (a.size !== b.size) return false;
  for (const [key, entry] of a) {
    const other = b.get(key);
    if (other?.signature !== entry.signature || other.since !== entry.since) return false;
  }
  return true;
}

function lastAt(events: readonly ActivityEvent[]): number {
  return events[events.length - 1]?.at ?? 0;
}

/** Version of the stored form; another one is dropped. */
const LOG_VERSION = 2;

/**
 * Budget of the stored log in characters, well inside the usual 5 MB of localStorage (UTF-16,
 * so about 2 MB of it) and shared with the app's other entries.
 */
export const MAX_STORED_CHARS = 1_000_000;

/** `[path index, seconds, team]`: a path changed a hundred times is stored once. */
function projectJson(events: readonly ActivityEvent[]): string {
  const paths: string[] = [];
  const index = new Map<string, number>();
  const rows: [number, number, number][] = [];
  for (const event of events) {
    let at = index.get(event.path);
    if (at === undefined) {
      at = paths.length;
      paths.push(event.path);
      index.set(event.path, at);
    }
    rows.push([at, Math.round(event.at / 1000), event.team ? 1 : 0]);
  }
  return JSON.stringify({ paths, events: rows });
}

/** A project's holds as `[holder, path, signature, seconds]`, the project key stored once. */
function seenJson(entries: readonly SeenEntry[]): string {
  return JSON.stringify(
    entries.map((entry) => [
      entry.holder,
      entry.path,
      entry.signature,
      Math.round(entry.since / 1000),
    ]),
  );
}

/** `{"a":…,"b":…}` from already serialized values. */
function objectJson(parts: readonly (readonly [string, string])[]): string {
  return `{${parts.map(([key, value]) => `${JSON.stringify(key)}:${value}`).join(",")}}`;
}

/** The newest `share` of a list sorted oldest first (at least none, at most all). */
function newest<T>(items: readonly T[], share: number): T[] {
  const keep = Math.max(0, Math.min(items.length, Math.floor(items.length * share)));
  return items.slice(items.length - keep);
}

/**
 * The log as JSON, at most `maxChars` long: per project its events and its holds. When it does
 * not fit, the projects active longest ago go first (with their holds), then the oldest events
 * and holds of the rest, until it fits.
 */
export function serializeLog(log: EventLog, maxChars = MAX_STORED_CHARS): string {
  const seenByProject = new Map<string, SeenEntry[]>();
  for (const entry of log.seen.values()) {
    const list = seenByProject.get(entry.project);
    if (list === undefined) seenByProject.set(entry.project, [entry]);
    else list.push(entry);
  }
  for (const list of seenByProject.values()) list.sort((a, b) => a.since - b.since);
  const lastActive = (project: string) =>
    Math.max(
      lastAt(log.projects.get(project) ?? []),
      ...(seenByProject.get(project) ?? []).map((entry) => entry.since),
    );
  // Newest first, so dropping from the end drops the projects active longest ago.
  const order = [...new Set([...log.projects.keys(), ...seenByProject.keys()])].sort(
    (a, b) => lastActive(b) - lastActive(a),
  );

  const render = (projects: readonly string[], share: number): string => {
    const eventParts: [string, string][] = [];
    const seenParts: [string, string][] = [];
    for (const project of projects) {
      const events = newest(log.projects.get(project) ?? [], share);
      const seen = newest(seenByProject.get(project) ?? [], share);
      if (events.length > 0) eventParts.push([project, projectJson(events)]);
      if (seen.length > 0) seenParts.push([project, seenJson(seen)]);
    }
    return `{"v":${String(LOG_VERSION)},"projects":${objectJson(eventParts)},"seen":${objectJson(seenParts)}}`;
  };

  // Each project's size once, so dropping whole projects needs no rendering per step.
  const overhead = render([], 1).length;
  const sizes = order.map((project) => render([project], 1).length - overhead);
  let kept = order.length;
  let total = overhead + sizes.reduce((sum, size) => sum + size, 0);
  while (kept > 1 && total > maxChars) {
    kept -= 1;
    total -= sizes[kept] ?? 0;
  }
  const projects = order.slice(0, kept);
  let share = 1;
  let text = render(projects, share);
  // One project still too large: keep its newest part, a little less than what fits.
  while (text.length > maxChars && share > 0) {
    share = share * (maxChars / text.length) * 0.95;
    if (share < 0.001) share = 0;
    text = render(projects, share);
  }
  return text;
}

function property(value: unknown, key: string): unknown {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? Reflect.get(value, key)
    : undefined;
}

function isSeconds(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function parseEvents(value: unknown): ActivityEvent[] | null {
  const paths = property(value, "paths");
  const rows = property(value, "events");
  if (!Array.isArray(paths) || !Array.isArray(rows)) return null;
  const names: string[] = [];
  for (const path of paths) {
    if (typeof path !== "string" || path === "") return null;
    names.push(path);
  }
  const events: ActivityEvent[] = [];
  for (const row of rows) {
    const cells = tuple(row, 3);
    if (cells === null) return null;
    const [at, seconds, team] = cells;
    if (!isSeconds(at) || !isSeconds(seconds) || (team !== 0 && team !== 1)) return null;
    const path = names[at];
    if (path === undefined) return null;
    events.push({ path, at: seconds * 1000, team: team === 1 });
  }
  return events;
}

/** The items of a JSON array of exactly `length` items, each still to be checked. */
function tuple(value: unknown, length: number): unknown[] | null {
  if (!Array.isArray(value) || value.length !== length) return null;
  const items: unknown[] = [];
  for (const item of value) items.push(item);
  return items;
}

function parseSeen(project: string, value: unknown): SeenEntry | null {
  const cells = tuple(value, 4);
  if (cells === null) return null;
  const [holder, path, signature, since] = cells;
  if (typeof holder !== "string" || typeof path !== "string") return null;
  if (typeof signature !== "string" || !isSeconds(since)) return null;
  if (!holder.startsWith("w:") && !holder.startsWith("t:")) return null;
  return { project, holder, path, signature, since: since * 1000 };
}

/** The stored log, pruned; `null` when it is malformed or of another version. */
export function parseLog(text: string, now: number): EventLog | null {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return null;
  }
  if (property(value, "v") !== LOG_VERSION) return null;
  const stored = property(value, "projects");
  const storedSeen = property(value, "seen");
  if (typeof stored !== "object" || stored === null || Array.isArray(stored)) return null;
  if (typeof storedSeen !== "object" || storedSeen === null || Array.isArray(storedSeen)) {
    return null;
  }
  const projects = new Map<string, ActivityEvent[]>();
  for (const [project, entry] of Object.entries(stored)) {
    const events = parseEvents(entry);
    if (events === null) return null;
    projects.set(project, events);
  }
  const seen = new Map<string, SeenEntry>();
  for (const [project, items] of Object.entries(storedSeen)) {
    if (!Array.isArray(items)) return null;
    for (const item of items) {
      const entry = parseSeen(project, item);
      if (entry === null) return null;
      seen.set(seenKey(entry.project, entry.holder, entry.path), entry);
    }
  }
  return prune({ projects, seen }, now);
}

// ---------------------------------------------------------------------------------------------
// Heat

export type HeatWindow = "hour" | "today" | "week";

export const HEAT_WINDOWS: readonly HeatWindow[] = ["hour", "today", "week"];

/** Steps of the replay: the window in this many equal parts. */
export const PLAY_STEPS = 24;

/** Where a window begins: an hour ago, at midnight (local time), seven days ago. */
export function windowStart(window: HeatWindow, now: number): number {
  switch (window) {
    case "hour":
      return now - HOUR_MS;
    case "today": {
      const midnight = new Date(now);
      midnight.setHours(0, 0, 0, 0);
      return midnight.getTime();
    }
    case "week":
      return now - KEEP_MS;
  }
}

/** The end of replay step `step` (0 to PLAY_STEPS) of the window `from` to `to`. */
export function stepTime(from: number, to: number, step: number): number {
  const clamped = Math.min(PLAY_STEPS, Math.max(0, step));
  return from + Math.round(((to - from) * clamped) / PLAY_STEPS);
}

/** First index with `at >= time` in events sorted by time. */
function firstAtOrAfter(events: readonly ActivityEvent[], time: number): number {
  let low = 0;
  let high = events.length;
  while (low < high) {
    const middle = (low + high) >> 1;
    if ((events[middle]?.at ?? 0) < time) low = middle + 1;
    else high = middle;
  }
  return low;
}

export interface Heat {
  /** Changes per file, by `treeKey`. */
  files: ReadonlyMap<string, number>;
  /** Changes inside each folder, all levels below added up. */
  folders: ReadonlyMap<string, number>;
  maxFile: number;
  maxFolder: number;
  /** Events in the range, also outside the tree's root. */
  total: number;
}

export const NO_HEAT: Heat = {
  files: new Map(),
  folders: new Map(),
  maxFile: 0,
  maxFolder: 0,
  total: 0,
};

/**
 * Heat of the tree for events from `from` up to and including `until`: once per update, never per
 * row. Paths outside the tree's root count in `total` only.
 */
export function heatOf(
  events: readonly ActivityEvent[],
  from: number,
  until: number,
  root: string | null,
  repoRoot: string | null,
): Heat {
  const offset = rootOffset(root, repoRoot);
  if (offset === null || root === null || until < from) return NO_HEAT;
  const windows = folderKeyOf(root).windows;
  const files = new Map<string, number>();
  const folders = new Map<string, number>();
  let total = 0;
  const start = firstAtOrAfter(events, from);
  const end = firstAtOrAfter(events, until + 1);
  for (let index = start; index < end; index += 1) {
    const event = events[index];
    if (event === undefined) continue;
    total += 1;
    const key = repoToTreeKey(event.path, offset, windows);
    if (key === null) continue;
    files.set(key, (files.get(key) ?? 0) + 1);
    for (const folder of ancestors(key)) folders.set(folder, (folders.get(folder) ?? 0) + 1);
  }
  return {
    files,
    folders,
    maxFile: Math.max(0, ...files.values()),
    maxFolder: Math.max(0, ...folders.values()),
    total,
  };
}

export type HeatLevel = 0 | 1 | 2 | 3 | 4;

/**
 * Level 1 to 4 of a count against the largest of its kind, on a square root scale so a file
 * changed once still shows beside one changed fifty times; 0 for nothing.
 */
export function heatLevel(count: number, max: number): HeatLevel {
  if (count <= 0 || max <= 0) return 0;
  const level = Math.ceil(4 * Math.sqrt(Math.min(1, count / max)));
  if (level >= 4) return 4;
  if (level <= 1) return 1;
  return level === 2 ? 2 : 3;
}
