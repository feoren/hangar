// Shared project notes (ADR 0006): the Shared group's state from the backend and what the panel
// shows for it. Pure logic; the panel calls the backend.

import type { NoteFolder } from "./api";
import type { MessageKey } from "./i18n";

export type LockReason = "needsCode" | "team" | "damaged";

export interface ResolvedConflict {
  title: string;
  /** Title of the copy of the other version; `null` when none was needed. */
  copy: string | null;
}

export type SharedStatus =
  | { state: "unavailable" }
  | { state: "off" }
  | { state: "locked"; reason: LockReason }
  | { state: "newerFormat" }
  | {
      state: "unlocked";
      uncommitted: number;
      ignored: boolean;
      unreadable: number;
      resolved: ResolvedConflict[];
    };

function record(value: unknown): object | null {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? value : null;
}

function field(value: object, name: string): unknown {
  return name in value ? Reflect.get(value, name) : undefined;
}

function count(value: unknown): number {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 ? value : 0;
}

/** The backend's answer, or `null` when it is not one. */
export function parseSharedStatus(value: unknown): SharedStatus | null {
  const entry = record(value);
  if (!entry) return null;
  switch (field(entry, "state")) {
    case "unavailable":
      return { state: "unavailable" };
    case "off":
      return { state: "off" };
    case "newerFormat":
      return { state: "newerFormat" };
    case "locked": {
      const reason = field(entry, "reason");
      if (reason !== "needsCode" && reason !== "team" && reason !== "damaged") return null;
      return { state: "locked", reason };
    }
    case "unlocked": {
      const resolved: ResolvedConflict[] = [];
      const list = field(entry, "resolved");
      for (const item of Array.isArray(list) ? list : []) {
        const conflict = record(item);
        if (!conflict) continue;
        const title = field(conflict, "title");
        const copy = field(conflict, "copy");
        if (typeof title !== "string") continue;
        resolved.push({ title, copy: typeof copy === "string" ? copy : null });
      }
      return {
        state: "unlocked",
        uncommitted: count(field(entry, "uncommitted")),
        ignored: field(entry, "ignored") === true,
        unreadable: count(field(entry, "unreadable")),
        resolved,
      };
    }
    default:
      return null;
  }
}

/** What typing a recovery code did; a wrong code is an answer, not an error. */
export type UnlockOutcome =
  | { outcome: "unlocked"; status: SharedStatus }
  | { outcome: "wrongCode" }
  | { outcome: "typo" }
  | { outcome: "teamCode" };

export function parseUnlock(value: unknown): UnlockOutcome | null {
  const entry = record(value);
  if (!entry) return null;
  const outcome = field(entry, "outcome");
  if (outcome === "wrongCode" || outcome === "typo" || outcome === "teamCode") return { outcome };
  if (outcome !== "unlocked") return null;
  const status = parseSharedStatus(field(entry, "status"));
  return status ? { outcome, status } : null;
}

/** What saving the recovery code as a file did. */
export type SaveCodeOutcome =
  { outcome: "saved"; path: string } | { outcome: "cancelled" } | { outcome: "insideRepository" };

export function parseSaveCode(value: unknown): SaveCodeOutcome | null {
  const entry = record(value);
  if (!entry) return null;
  const outcome = field(entry, "outcome");
  if (outcome === "cancelled" || outcome === "insideRepository") return { outcome };
  const path = field(entry, "path");
  return outcome === "saved" && typeof path === "string" ? { outcome, path } : null;
}

/** Message for a code that did not unlock. */
export function unlockError(outcome: UnlockOutcome): MessageKey | null {
  switch (outcome.outcome) {
    case "unlocked":
      return null;
    case "wrongCode":
      return "sharedNotes.wrongCode";
    case "typo":
      return "sharedNotes.typo";
    case "teamCode":
      return "sharedNotes.teamCode";
  }
}

/** The shared notes of the project that contains `folder`. */
export function sharedFolder(folder: NoteFolder): NoteFolder {
  return { path: folder.path, shared: true };
}

export function isShared(folder: NoteFolder): boolean {
  return folder.shared === true;
}

/** Whether the panel shows the two groups "Shared" and "Only on this computer". */
export function showsGroups(status: SharedStatus | null): boolean {
  return status !== null && status.state !== "unavailable";
}

/** The one action a state offers; never more than one. */
export type SharedAction = "turnOn" | "unlock";

/** The line the Shared group shows instead of notes, or `null` when it lists notes. */
export function sharedLine(
  status: SharedStatus,
): { text: MessageKey; action: SharedAction | null } | null {
  switch (status.state) {
    case "unavailable":
    case "unlocked":
      return null;
    case "off":
      return { text: "sharedNotes.offLine", action: "turnOn" };
    case "newerFormat":
      return { text: "sharedNotes.newerFormat", action: null };
    case "locked":
      switch (status.reason) {
        case "needsCode":
          return { text: "sharedNotes.lockedLine", action: "unlock" };
        case "team":
          return { text: "sharedNotes.teamLocked", action: null };
        case "damaged":
          return { text: "sharedNotes.damaged", action: null };
      }
  }
}

/** Quiet hints under the shared notes: what is not committed, ignored or unreadable. */
export function sharedHints(
  status: SharedStatus,
): { key: MessageKey; params?: Record<string, number> }[] {
  if (status.state !== "unlocked") return [];
  const hints: { key: MessageKey; params?: Record<string, number> }[] = [];
  if (status.ignored) hints.push({ key: "sharedNotes.ignored" });
  else if (status.uncommitted === 1) hints.push({ key: "sharedNotes.uncommittedOne" });
  else if (status.uncommitted > 1)
    hints.push({ key: "sharedNotes.uncommitted", params: { count: status.uncommitted } });
  if (status.unreadable === 1) hints.push({ key: "sharedNotes.unreadableOne" });
  else if (status.unreadable > 1)
    hints.push({ key: "sharedNotes.unreadable", params: { count: status.unreadable } });
  return hints;
}
