// Workspace for restoring after a restart: which sessions were open in which order and which
// one was active, plus trimming the saved scrollback.
// Pure logic without backend; contract: docs/wiki/pages/syntheses/session-restore.md.

import type { SessionKind, Worktree } from "./api";
import { normalizeMarkerColor, storedEmoji } from "./marker";
import { parseWorktree } from "./worktree";

export const WORKSPACE_VERSION = 1;
/** Backend limit for one scrollback (UTF-8). Leaves some room for the separator line on load. */
export const MAX_SCROLLBACK_BYTES = 2 * 1024 * 1024 - 4096;
/** Backend limit for the number of sessions. */
const MAX_SESSIONS = 512;

export interface SavedSession {
  /** UUID, lowercase; stays the same across restarts (unlike the session ID). */
  key: string;
  profileId: string;
  cwd: string;
  title: string;
  /** Position in the sidebar, starting at 0. */
  order: number;
  /** Session marker (marker.ts): `#rrggbb` lowercase, or `null`. */
  color: string | null;
  /** Session marker: 1 to 16 bytes UTF-8, or `null` (never an empty string). */
  emoji: string | null;
  /** The session's own git worktree, or `null`. */
  worktree: Worktree | null;
}

export interface Workspace {
  version: typeof WORKSPACE_VERSION;
  sessions: SavedSession[];
  activeKey: string | null;
}

export const EMPTY_WORKSPACE: Workspace = {
  version: WORKSPACE_VERSION,
  sessions: [],
  activeKey: null,
};

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export function isSessionKey(value: unknown): value is string {
  return typeof value === "string" && UUID.test(value);
}

export function newSessionKey(): string {
  return crypto.randomUUID();
}

export interface SessionEntry {
  key: string;
  profileId: string;
  cwd: string;
  title: string;
  /** Update sessions (`kind: "update"`) never go into the workspace. */
  kind?: SessionKind | undefined;
  color?: string | null | undefined;
  emoji?: string | null | undefined;
  worktree?: Worktree | null | undefined;
}

/** Started again on restore. An update after a restart would come as a surprise. */
export function restorable(entry: { kind?: SessionKind | undefined }): boolean {
  return entry.kind !== "update";
}

/**
 * Builds the workspace from the sessions in sidebar order. Entries the backend would reject
 * (invalid or duplicate key, empty profile ID) are dropped, so a single broken session does
 * not prevent saving all the others. Update sessions are dropped as well; if one of them was
 * active, the workspace records no active session.
 */
export function buildWorkspace(
  entries: readonly SessionEntry[],
  activeKey: string | null,
): Workspace {
  const seen = new Set<string>();
  const sessions: SavedSession[] = [];
  for (const entry of entries) {
    if (sessions.length >= MAX_SESSIONS) break;
    if (!restorable(entry)) continue;
    if (!isSessionKey(entry.key) || entry.profileId.trim() === "") continue;
    const key = entry.key.toLowerCase();
    if (seen.has(key)) continue;
    seen.add(key);
    sessions.push({
      key,
      profileId: entry.profileId,
      cwd: entry.cwd,
      title: entry.title,
      order: sessions.length,
      color: normalizeMarkerColor(entry.color),
      emoji: storedEmoji(entry.emoji),
      worktree: parseWorktree(entry.worktree ?? null),
    });
  }
  const active = activeKey?.toLowerCase() ?? null;
  return {
    version: WORKSPACE_VERSION,
    sessions,
    activeKey: active !== null && seen.has(active) ? active : null,
  };
}

function savedSession(value: unknown): SavedSession | null {
  if (typeof value !== "object" || value === null) return null;
  const field = (name: string): unknown => (name in value ? Reflect.get(value, name) : undefined);
  const key = field("key");
  const profileId = field("profileId");
  const cwd = field("cwd");
  const title = field("title");
  const order = field("order");
  if (!isSessionKey(key)) return null;
  if (typeof profileId !== "string" || profileId.trim() === "") return null;
  if (typeof cwd !== "string" || typeof title !== "string") return null;
  if (typeof order !== "number" || !Number.isInteger(order) || order < 0) return null;
  // Older files have no marker; invalid values are dropped instead of losing the session.
  const color = normalizeMarkerColor(field("color"));
  const emoji = storedEmoji(field("emoji"));
  const worktree = parseWorktree(field("worktree"));
  return { key: key.toLowerCase(), profileId, cwd, title, order, color, emoji, worktree };
}

/**
 * Validates the response of `load_workspace`. Returns the sessions sorted by `order` and
 * renumbered; invalid entries and duplicate keys are dropped. `null` means there is nothing
 * to restore.
 */
export function parseWorkspace(value: unknown): Workspace | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const version: unknown = "version" in value ? value.version : undefined;
  const list: unknown = "sessions" in value ? value.sessions : undefined;
  const active: unknown = "activeKey" in value ? value.activeKey : null;
  if (version !== WORKSPACE_VERSION || !Array.isArray(list)) return null;
  const sessions = list
    .map(savedSession)
    .filter((session): session is SavedSession => session !== null)
    .sort((a, b) => a.order - b.order);
  return buildWorkspace(sessions, isSessionKey(active) ? active : null);
}

/**
 * Workspace to save when the UI was reloaded and the backend still has sessions from before
 * that the UI cannot show: the saved state stays at the front, new sessions go after it.
 * That way a save after the reload overwrites nothing.
 */
export function mergeWorkspace(current: Workspace, remembered: Workspace | null): Workspace {
  if (remembered === null || remembered.sessions.length === 0) return current;
  return buildWorkspace(
    [...remembered.sessions, ...current.sessions],
    current.activeKey ?? remembered.activeKey,
  );
}

/** Enough to decide whether a session's scrollback gets saved now. */
export interface ScrollbackCandidate {
  /** New output since the last save. */
  dirty: boolean;
  /** Time of the last output (ms, same clock as `now`). */
  lastOutputAt: number;
}

/**
 * Periodic saving only takes sessions that have been quiet for `quietMs`: a session printing a
 * lot will have new output again right away, and serializing costs several milliseconds each.
 * `force` (quit, update) takes every changed session.
 */
export function scrollbackDue(
  candidate: ScrollbackCandidate,
  now: number,
  quietMs: number,
  force: boolean,
): boolean {
  return candidate.dirty && (force || now - candidate.lastOutputAt >= quietMs);
}

export function sameWorkspace(a: Workspace, b: Workspace): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

const encoder = new TextEncoder();

export function utf8Length(text: string): number {
  return encoder.encode(text).length;
}

/**
 * Trims a serialized scrollback from the front to at most `maxBytes` (UTF-8). It cuts only at
 * line ends so no escape sequence or character breaks in the middle; the rest starts with a
 * color reset, because the color setting that was cut off would otherwise be missing.
 */
export function trimScrollback(text: string, maxBytes: number = MAX_SCROLLBACK_BYTES): string {
  if (utf8Length(text) <= maxBytes) return text;
  const reset = "\x1b[0m";
  const lines = text.split("\n");
  let total = utf8Length(reset);
  let start = lines.length;
  // Take as many whole lines from the end as fit (+1 for the line break).
  while (start > 0) {
    const line = lines[start - 1] ?? "";
    const size = utf8Length(line) + (start < lines.length ? 1 : 0);
    if (total + size > maxBytes) break;
    total += size;
    start -= 1;
  }
  if (start === lines.length) return "";
  return reset + lines.slice(start).join("\n");
}
