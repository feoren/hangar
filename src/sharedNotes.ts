// Shared project notes (ADR 0006): the Shared group's state from the backend and what the panel
// shows for it. Pure logic; the panel calls the backend.

import type { NoteFolder } from "./api";
import type { MessageKey } from "./i18n";

/**
 * Why the Shared group is locked. `team`: a team vault of a team this person is not in; `link`:
 * of a team of this person, but this clone is not linked; `waiting`: linked, the key has not
 * reached this computer yet; `oldServer`: the team server is too old for team notes.
 */
export type LockReason = "needsCode" | "team" | "link" | "waiting" | "oldServer" | "damaged";

const LOCK_REASONS: readonly LockReason[] = [
  "needsCode",
  "team",
  "link",
  "waiting",
  "oldServer",
  "damaged",
];

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
      /** A team vault whose key is being renewed: readable, but every save is refused. */
      renewing: boolean;
      /**
       * Whether the vault is a team vault; `null` when the backend does not say. Only a vault
       * known to be solo offers "Share with team".
       */
      team: boolean | null;
    };

/** The team a folder's shared notes belong to (`shared_notes_team`). */
export interface TeamNotes {
  /** The website's project id of the team. */
  project: string;
  /** This person is an owner or admin of the team. */
  manager: boolean;
  /** The team server speaks protocol version 2; `null` while not known. */
  speaks: boolean | null;
  /** The team links this clone of the repository. */
  linked: boolean;
  /** The team has a key ring on the relay; `null` while not known. */
  exists: boolean | null;
}

function record(value: unknown): object | null {
  return typeof value === "object" && value !== null && !Array.isArray(value) ? value : null;
}

function field(value: object, name: string): unknown {
  return name in value ? Reflect.get(value, name) : undefined;
}

function count(value: unknown): number {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 ? value : 0;
}

/** `true`, `false` or `null`; `undefined` for anything else. */
function optionalFlag(value: unknown): boolean | null | undefined {
  if (value === null || typeof value === "boolean") return value;
  return undefined;
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
      const reason = LOCK_REASONS.find((known) => known === field(entry, "reason"));
      if (reason === undefined) return null;
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
      const renewing = field(entry, "renewing") === true;
      const team = field(entry, "team");
      return {
        state: "unlocked",
        uncommitted: count(field(entry, "uncommitted")),
        ignored: field(entry, "ignored") === true,
        unreadable: count(field(entry, "unreadable")),
        resolved,
        renewing,
        // Only team vaults renew their key, so a renewing vault is a team vault.
        team: typeof team === "boolean" ? team : renewing ? true : null,
      };
    }
    default:
      return null;
  }
}

/** `shared_notes_team`'s answer; `null` for no team and for anything malformed. */
export function parseTeamNotes(value: unknown): TeamNotes | null {
  const entry = record(value);
  if (!entry) return null;
  const project = field(entry, "project");
  const manager = field(entry, "manager");
  const linked = field(entry, "linked");
  const speaks = optionalFlag(field(entry, "speaks"));
  const exists = optionalFlag(field(entry, "exists"));
  if (typeof project !== "string" || project === "") return null;
  if (typeof manager !== "boolean" || typeof linked !== "boolean") return null;
  if (speaks === undefined || exists === undefined) return null;
  return { project, manager, speaks, linked, exists };
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

/** Whether the project has shared notes as a feature at all (any state but "unavailable"). */
export function showsGroups(status: SharedStatus | null): boolean {
  return status !== null && status.state !== "unavailable";
}

/**
 * The one action a state offers; never more than one. `turnOn` and `unlock` open the dialog of
 * the notes recovery code, `teamOn` the dialog that turns team notes on, `link` links this folder.
 */
export type SharedAction = "turnOn" | "unlock" | "teamOn" | "link";

/** A line of the Shared group; its text may name the team's project as `{project}`. */
export interface SharedLine<Action> {
  text: MessageKey;
  action: Action | null;
}

/** The team, when it links this clone; only then can it share notes in this folder. */
function linkedTeam(team: TeamNotes | null): TeamNotes | null {
  return team?.linked === true ? team : null;
}

/** The setup line of a folder without shared notes (ADR 0006, section 8). */
function offLine(team: TeamNotes | null): SharedLine<SharedAction> {
  const linked = linkedTeam(team);
  // Members wait for an owner or admin; an old team server cannot hold the team's key, so there
  // (and while the server is not known) owners and admins get the solo setup with a code.
  if (linked !== null && linked.speaks !== false) {
    if (!linked.manager) return { text: "sharedNotes.askManager", action: null };
    if (linked.speaks === true) return { text: "sharedNotes.teamOffLine", action: "teamOn" };
  }
  return { text: "sharedNotes.offLine", action: "turnOn" };
}

/** The line the Shared group shows instead of notes, or `null` when it lists notes. */
export function sharedLine(
  status: SharedStatus,
  team: TeamNotes | null = null,
): SharedLine<SharedAction> | null {
  switch (status.state) {
    case "unavailable":
    case "unlocked":
      return null;
    case "off":
      return offLine(team);
    case "newerFormat":
      return { text: "sharedNotes.newerFormat", action: null };
    case "locked":
      switch (status.reason) {
        case "needsCode":
          return { text: "sharedNotes.lockedLine", action: "unlock" };
        case "team":
          return { text: "sharedNotes.teamLocked", action: null };
        case "link":
          return { text: "sharedNotes.linkLine", action: "link" };
        case "waiting":
          return { text: "sharedNotes.waiting", action: null };
        case "oldServer":
          return { text: "sharedNotes.oldServer", action: null };
        case "damaged":
          return { text: "sharedNotes.damaged", action: null };
      }
  }
}

/**
 * The quiet team line under the Shared group, or `null`: the key is being renewed; a solo vault
 * can be shared with the linked team (owners and admins, team server of version 2); or that team
 * server needs an update first (owners and admins only, members cannot act on it).
 */
export function sharedTeamLine(
  status: SharedStatus,
  team: TeamNotes | null,
): SharedLine<"shareTeam"> | null {
  if (status.state === "unlocked" && status.renewing) {
    return { text: "sharedNotes.renewing", action: null };
  }
  const linked = linkedTeam(team);
  if (linked === null || !linked.manager) return null;
  const solo = status.state === "unlocked" && status.team === false;
  if (solo && linked.speaks === true) return { text: "sharedNotes.shareLine", action: "shareTeam" };
  if ((solo || status.state === "off") && linked.speaks === false) {
    return { text: "sharedNotes.updateServer", action: null };
  }
  return null;
}

/** Whether shared notes can be saved, created and restored: unlocked, no key renewal pending. */
export function sharedWritable(status: SharedStatus | null): boolean {
  return status?.state === "unlocked" && !status.renewing;
}

/** Whether to ask `shared_notes_team`: the folder is in a Git repository of a known format. */
export function asksTeam(status: SharedStatus | null): boolean {
  return status !== null && status.state !== "unavailable" && status.state !== "newerFormat";
}

/** Project names by id from the account's projects; empty while they are not loaded. */
export function projectNamesOf(
  projects: readonly { id: string; name: string }[] | null,
): ReadonlyMap<string, string> {
  return new Map((projects ?? []).map((project) => [project.id, project.name]));
}

/**
 * The team's project as the texts name it: its name on the website, else (signed out, not loaded
 * yet) the repository folder's name, so a line never shows an id.
 */
export function teamProjectName(
  team: TeamNotes | null,
  names: ReadonlyMap<string, string>,
  fallback: string,
): string {
  const name = team === null ? undefined : names.get(team.project)?.trim();
  return name === undefined || name === "" ? fallback : name;
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
