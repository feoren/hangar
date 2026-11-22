// Recently opened sessions: the programs the person started in a folder, offered again in the
// sidebar ("Recently opened", Ctrl+Shift+O) so the next start skips choosing a folder and then a
// program. A start records its entry, closing updates it with the session's final title and
// conversation, which a reopen resumes where the agent CLI has one. Kept by the backend for the
// main window alone (mainStore.ts, `main_store.rs` `ClosedSession`, which holds the same shape):
// an entry starts a program in a folder, and a viewer that planted one could start an agent in a
// folder of its choice. Invalid content yields an empty list.

import { folderName, samePath } from "./folders";
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
/** `closedAt` of an entry taken over from the recent folders: when it was used is not known. */
export const UNKNOWN_TIME = 0;

export interface RecentSession {
  profileId: string;
  cwd: string;
  title: string;
  /** The session's marker; `#rrggbb` in lower case. */
  color: string | null;
  emoji: string | null;
  /** Conversation id the agent CLI resumes (lowercase UUID); `null` for programs without one. */
  cliSessionId: string | null;
  /**
   * When the session was last used, in milliseconds since the Unix epoch: its start, or its
   * close once it closed. The stored field keeps the name it had when only closed sessions were
   * listed. UNKNOWN_TIME for entries taken over from the recent folders (`seedRecent`).
   */
  closedAt: number;
}

/** What a started or closed session tells about itself (`SessionView`). */
export interface SessionSource {
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
 * Whether the session goes on the list. Not an update session (it would update again), not one
 * started as a sub-agent definition, and not one with a worktree of its own or a team session
 * (which always has one): the worktree goes on close, and agent CLIs resume a conversation by
 * its folder, so it would not come back.
 */
export function isRecorded(session: SessionSource): boolean {
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

function validEntry(entry: RecentSession): boolean {
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

/**
 * The entry for a session started or closed at `at`; `null` when it is not recorded or would not
 * keep.
 */
export function recentEntry(session: SessionSource, at: number): RecentSession | null {
  if (!isRecorded(session)) return null;
  const entry: RecentSession = {
    profileId: session.profileId,
    cwd: session.cwd,
    title: session.title,
    color: normalizeMarkerColor(session.marker?.color ?? null),
    emoji: session.marker?.emoji ?? null,
    cliSessionId: normalizeCliSessionId(session.cliSessionId),
    closedAt: at,
  };
  return validEntry(entry) ? entry : null;
}

/**
 * Two entries for the same thing: the same program in the same folder, or one conversation (the
 * backend refuses a list that names one conversation twice). Only the newest stays.
 */
function sameEntry(a: RecentSession, b: RecentSession): boolean {
  if (a.cliSessionId !== null && a.cliSessionId === b.cliSessionId) return true;
  return a.profileId === b.profileId && samePath(a.cwd, b.cwd);
}

/**
 * `entry` goes first and replaces older entries for the same program and folder (a start, then
 * its close, leave one entry); at most KEEP_LIMIT stay.
 */
export function pushRecent(list: readonly RecentSession[], entry: RecentSession): RecentSession[] {
  if (!validEntry(entry)) return [...list];
  return [entry, ...list.filter((other) => !sameEntry(other, entry))].slice(0, KEEP_LIMIT);
}

/** The list without `entry` (removed by the person, or no longer startable). */
export function withoutRecent(
  list: readonly RecentSession[],
  entry: RecentSession,
): RecentSession[] {
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
 * The entries the sidebar offers, most recent first, at most SHOW_LIMIT: only what is not open.
 * An entry is hidden while a session in the list runs its program in its folder (or waits there
 * for a restart), or holds its conversation.
 */
export function shownRecent(
  list: readonly RecentSession[],
  sessions: readonly {
    profileId: string;
    cwd: string;
    cliSessionId?: string | null | undefined;
  }[],
): RecentSession[] {
  const held = new Set(
    sessions.flatMap((session) => {
      const id = normalizeCliSessionId(session.cliSessionId);
      return id === null ? [] : [id];
    }),
  );
  return list
    .filter(
      (entry) =>
        !(entry.cliSessionId !== null && held.has(entry.cliSessionId)) &&
        !sessions.some(
          (session) => session.profileId === entry.profileId && samePath(session.cwd, entry.cwd),
        ),
    )
    .slice(0, SHOW_LIMIT);
}

/**
 * The first list, before anything was recorded: the recent folders (newest first) with the
 * program started last, up to SHOW_LIMIT, so the section offers something from the start.
 * Nothing without a program; no conversation, the folder's name as the title.
 */
export function seedRecent(
  recentFolders: readonly string[],
  profileId: string | undefined,
): RecentSession[] {
  if (profileId === undefined) return [];
  let list: RecentSession[] = [];
  for (const cwd of recentFolders) {
    if (list.length >= SHOW_LIMIT) break;
    const entry: RecentSession = {
      profileId,
      cwd,
      title: folderName(cwd),
      color: null,
      emoji: null,
      cliSessionId: null,
      closedAt: UNKNOWN_TIME,
    };
    // Appended: the newest folder stays first.
    if (validEntry(entry) && !list.some((other) => sameEntry(other, entry))) {
      list = [...list, entry];
    }
  }
  return list;
}

/** localStorage key: the first list was made once; a list emptied by hand stays empty. */
export const SEEDED_STORAGE_KEY = "hangar.recentlyOpened.seeded";

/**
 * Whether the first list is still to be made. Without storage (blocked, private mode) it counts
 * as made: entries that keep coming back after being removed are worse than none.
 */
export function needsSeed(storage: Pick<Storage, "getItem"> | null): boolean {
  try {
    return storage !== null && storage.getItem(SEEDED_STORAGE_KEY) === null;
  } catch {
    return false;
  }
}

/** The window's localStorage; `null` where reading it throws (blocked site data). */
export function browserStorage(): Storage | null {
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function markSeeded(storage: Pick<Storage, "setItem"> | null): void {
  try {
    storage?.setItem(SEEDED_STORAGE_KEY, "1");
  } catch {
    // Blocked storage: `needsSeed` reads that as made already.
  }
}

function parseEntry(value: unknown): RecentSession | null {
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
  const entry: RecentSession = { profileId, cwd, title, color, emoji, cliSessionId, closedAt };
  return validEntry(entry) ? entry : null;
}

/**
 * The list the backend answered. Anything malformed, one bad entry, too many entries or one
 * conversation twice gives an empty list, as the backend reads a file like that.
 */
export function parseRecentSessions(value: unknown): RecentSession[] {
  if (!Array.isArray(value) || value.length > KEEP_LIMIT) return [];
  const entries: RecentSession[] = [];
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
