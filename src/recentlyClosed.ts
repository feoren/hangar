// Recently closed sessions: after closing a session, the same program starts again in the same
// folder with one click (sidebar, Ctrl+Shift+O), resuming its conversation where the agent CLI
// has one. Kept by the backend for the main window alone (mainStore.ts, `main_store.rs`
// `ClosedSession`): an entry starts a program in a folder, and a viewer that planted one could
// start an agent in a folder of its choice. Invalid content yields an empty list.

import { samePath } from "./folders";
import { normalizeMarkerColor, parseEmoji, type Marker } from "./marker";
import { normalizeCliSessionId } from "./workspace";

/** Kept at most, as the backend bounds them (`MAX_RECENTLY_CLOSED`). */
export const KEEP_LIMIT = 10;
/** Shown at most in the sidebar. */
export const SHOW_LIMIT = 5;
/** As the backend bounds them (`main_store.rs`, `manager.rs` `MAX_TITLE_CHARS`). */
const MAX_PROFILE_ID_UNITS = 200;
const MAX_FOLDER_UNITS = 4096;
const MAX_TITLE_CHARS = 256;

export interface ClosedSession {
  profileId: string;
  cwd: string;
  title: string;
  /** The session's marker; `#rrggbb` in lower case. */
  color: string | null;
  emoji: string | null;
  /** Conversation id the agent CLI resumes (lowercase UUID); `null` for programs without one. */
  cliSessionId: string | null;
  /** Milliseconds since the Unix epoch. */
  closedAt: number;
}

/** What a closed session tells about itself (`SessionView`). */
export interface ClosingSession {
  profileId: string;
  cwd: string;
  title: string;
  kind?: "agent" | "update" | undefined;
  mode?: "private" | "team" | undefined;
  subAgent?: string | null | undefined;
  worktree?: unknown;
  cliSessionId?: string | null | undefined;
  marker?: Marker | undefined;
}

/**
 * Whether closing the session puts it on the list. Not an update session (it would update
 * again), not one started as a sub-agent definition, and not one with a worktree of its own or a
 * team session (which always has one): the worktree goes on close, and agent CLIs resume a
 * conversation by its folder, so it would not come back.
 */
export function recordsOnClose(session: ClosingSession): boolean {
  return (
    session.kind !== "update" &&
    session.mode !== "team" &&
    (session.subAgent ?? null) === null &&
    session.worktree === undefined
  );
}

// eslint-disable-next-line no-control-regex
const CONTROL = /[\u0000-\u001f\u007f]/;
// eslint-disable-next-line no-control-regex
const FOLDER_CONTROL = /[\u0000-\u001f]/;

function validEntry(entry: ClosedSession): boolean {
  const { profileId, cwd, title, color, emoji, cliSessionId, closedAt } = entry;
  return (
    profileId.trim() !== "" &&
    !CONTROL.test(profileId) &&
    profileId.length <= MAX_PROFILE_ID_UNITS &&
    cwd.trim() !== "" &&
    !FOLDER_CONTROL.test(cwd) &&
    cwd.length <= MAX_FOLDER_UNITS &&
    title.trim() !== "" &&
    !CONTROL.test(title) &&
    // Code points, as the backend counts `chars()`.
    Array.from(title).length <= MAX_TITLE_CHARS &&
    (color === null || normalizeMarkerColor(color) === color) &&
    (emoji === null || parseEmoji(emoji) === emoji) &&
    (cliSessionId === null || normalizeCliSessionId(cliSessionId) === cliSessionId) &&
    Number.isSafeInteger(closedAt) &&
    closedAt >= 0
  );
}

/** The entry for a session closed at `closedAt`; `null` when it is not recorded or would not keep. */
export function closedEntry(session: ClosingSession, closedAt: number): ClosedSession | null {
  if (!recordsOnClose(session)) return null;
  const entry: ClosedSession = {
    profileId: session.profileId,
    cwd: session.cwd,
    title: session.title,
    color: normalizeMarkerColor(session.marker?.color ?? null),
    emoji: session.marker?.emoji ?? null,
    cliSessionId: normalizeCliSessionId(session.cliSessionId),
    closedAt,
  };
  return validEntry(entry) ? entry : null;
}

/**
 * Two entries for the same thing: one conversation, or (with no conversation to tell them apart)
 * the same program in the same folder. Only the newest stays.
 */
function sameSession(a: ClosedSession, b: ClosedSession): boolean {
  if (a.cliSessionId !== null && a.cliSessionId === b.cliSessionId) return true;
  return a.profileId === b.profileId && samePath(a.cwd, b.cwd);
}

/** `entry` goes first; older entries for the same session leave, at most KEEP_LIMIT stay. */
export function pushClosed(list: readonly ClosedSession[], entry: ClosedSession): ClosedSession[] {
  if (!validEntry(entry)) return [...list];
  return [entry, ...list.filter((other) => !sameSession(other, entry))].slice(0, KEEP_LIMIT);
}

/** The list without `entry` (reopened or no longer startable). */
export function withoutClosed(
  list: readonly ClosedSession[],
  entry: ClosedSession,
): ClosedSession[] {
  return list.filter(
    (other) =>
      !(
        other.closedAt === entry.closedAt &&
        other.profileId === entry.profileId &&
        other.cwd === entry.cwd &&
        other.cliSessionId === entry.cliSessionId
      ),
  );
}

/**
 * The entries the sidebar offers, most recent first, at most SHOW_LIMIT: none whose conversation
 * a session in the list holds (it is open, or its placeholder waits for a restart). An entry
 * without a conversation id resumes the program's last conversation in its folder, which a
 * session of the same program there may hold: it is hidden while one is in the list.
 */
export function shownClosed(
  list: readonly ClosedSession[],
  sessions: readonly {
    profileId: string;
    cwd: string;
    cliSessionId?: string | null | undefined;
  }[],
): ClosedSession[] {
  const held = new Set(
    sessions.flatMap((session) => {
      const id = normalizeCliSessionId(session.cliSessionId);
      return id === null ? [] : [id];
    }),
  );
  return list
    .filter((entry) =>
      entry.cliSessionId === null
        ? !sessions.some(
            (session) => session.profileId === entry.profileId && samePath(session.cwd, entry.cwd),
          )
        : !held.has(entry.cliSessionId),
    )
    .slice(0, SHOW_LIMIT);
}

function parseEntry(value: unknown): ClosedSession | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const text = (key: string): string | null => {
    const field: unknown = Reflect.get(value, key);
    return typeof field === "string" ? field : null;
  };
  const optional = (key: string): string | null | undefined => {
    const field: unknown = Reflect.get(value, key) ?? null;
    return field === null ? null : typeof field === "string" ? field : undefined;
  };
  const profileId = text("profileId");
  const cwd = text("cwd");
  const title = text("title");
  const color = optional("color");
  const emoji = optional("emoji");
  const cliSessionId = optional("cliSessionId");
  const closedAt: unknown = Reflect.get(value, "closedAt");
  if (profileId === null || cwd === null || title === null || typeof closedAt !== "number") {
    return null;
  }
  if (color === undefined || emoji === undefined || cliSessionId === undefined) return null;
  const entry: ClosedSession = { profileId, cwd, title, color, emoji, cliSessionId, closedAt };
  return validEntry(entry) ? entry : null;
}

/**
 * The list the backend answered. Anything malformed, one bad entry, too many entries or one
 * conversation twice gives an empty list, as the backend reads a file like that.
 */
export function parseRecentlyClosed(value: unknown): ClosedSession[] {
  if (!Array.isArray(value) || value.length > KEEP_LIMIT) return [];
  const entries: ClosedSession[] = [];
  const ids = new Set<string>();
  for (const item of value) {
    const entry = parseEntry(item);
    if (entry === null) return [];
    if (entry.cliSessionId !== null) {
      if (ids.has(entry.cliSessionId)) return [];
      ids.add(entry.cliSessionId);
    }
    entries.push(entry);
  }
  return entries;
}
