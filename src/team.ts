// The team connection in the window (team service step F, parts 2a and 2b): checks the backend's
// team state (Rust: serde camelCase, tagged by `state`) and builds the texts of the Team group in
// the sidebar. The backend owns the keys, the connection and every request; the window only shows
// the state, turns "Share with team" on or off, answers confirmations of new devices, links
// repositories to a project and shows the teammates' sessions and their overlaps with ours.

import type { AttentionState } from "./attention";
import {
  field,
  formatLines,
  isCount,
  list,
  parseRange,
  type LineRange,
  type OverlapLevel,
} from "./claims";
import type { MessageKey, Translate } from "./i18n";
import { revealInvisible } from "./invisible";

/**
 * `storage`: the team's local state on this device could not be read. `otherInstance`: another
 * Hangar window on this computer runs this team's connection; this one waits and takes over.
 */
export type AttentionReason =
  | "historyMismatch"
  | "signedOut"
  | "keychain"
  | "needsUpdate"
  | "refused"
  | "storage"
  | "otherInstance"
  | "otherAccount"
  | "unknown";

/** State of one project's team connection (Rust: `TeamStatus`). */
export type TeamStatus =
  | { state: "off" }
  /** `retrying`: a previous attempt failed, the backend waits to reconnect. */
  | { state: "connecting"; retrying: boolean }
  | { state: "connected"; epoch: number; members: number; devices: number }
  /** This device waits until another device of the person, or an owner or admin, confirms it. */
  | { state: "waitingForConfirmation"; safetyNumber: string[] }
  /** `unknown`: a reason (or a state) this Hangar does not know yet. */
  | { state: "needsAttention"; reason: AttentionReason }
  | { state: "removed" }
  | { state: "projectDeleted" };

export type ConfirmationKind = "ownDevice" | "newMember" | "unknown";

/** A new device waiting for this device's confirmation (Rust: `PendingConfirmation`). */
export interface PendingConfirmation {
  id: string;
  projectId: string;
  /** Opaque ids, shown muted and shortened. */
  member: string;
  device: string;
  kind: ConfirmationKind;
  safetyNumber: string[];
  /** The member was removed from the team before; confirming admits them again. */
  readmitted: boolean;
}

/** What a session is doing, as the window's attention logic sees it (Rust: `SessionState`). */
export type SessionState = "working" | "waiting" | "idle";

/** A teammate's session as its device's presence describes it (Rust: `PeerSession`). */
export interface PeerSession {
  /** The session's key in the team: unique per teammate device, never shown (a row's React key). */
  key: string;
  agent: string;
  title: string;
  state: SessionState;
  /** The repository's root commit: files are marked only beside a session of the same one. */
  repo: string;
  /** Paths relative to the repository, as the teammate's git spells them. */
  files: string[];
  /** Files left out of the presence to keep it small. */
  moreFiles: number;
}

/** A teammate's device that is connected to the team (Rust: `PeerView`). */
export interface PeerView {
  member: string;
  device: string;
  /** The display name the device sends; empty when it sent none. */
  name: string;
  /** Another device of the signed-in person. */
  own: boolean;
  presence: "active" | "idle" | "away";
  sessions: PeerSession[];
}

/** One of our sessions colliding with a teammate's session (Rust: `TeammateOverlap`). */
export interface TeammateOverlap {
  sessionId: number;
  member: string;
  device: string;
  /** The teammate's session key: a device may collide with ours in several sessions (a row's React key, never shown). */
  otherSession: string;
  name: string;
  agent: string;
  title: string;
  level: OverlapLevel;
  /** Our spelling of the shared file; `null` for the same area, or a file this device cannot name. */
  path: string | null;
  lines: LineRange[];
  /** Shared files in all. */
  files: number;
}

/** A repository folder linked to a project, or offered for linking (Rust: `LinkedRepo`, `RepoChoice`). */
export interface RepoRef {
  /** The repository's root commit: its identity in the team. */
  root: string;
  /**
   * The clone on this device (its git common folder, normalized). Each clone or fork is linked on
   * its own; `null` only for a link from before folders were bound, which shares nothing.
   */
  folder: string | null;
  name: string;
}

/**
 * A teammate's device that reports a different team history (Rust: `DivergentDevice`): it signed
 * a record naming another membership log. Its sessions stay hidden until the history changes.
 */
export interface DivergentDevice {
  member: string;
  device: string;
  /** The display name it sent last; empty when none arrived. */
  name: string;
}

/** Someone who asked the holder to hand a task over (Rust: `RequestView`). */
export interface TaskRequest {
  member: string;
  /** From the asking person's signed note; empty without one. */
  name: string;
  /** This person asked. */
  mine: boolean;
  /** One of the requester's listed devices signed it: only then the holder gets a card. */
  confirmed: boolean;
}

/** A reservation in the team (Rust: `TaskView`). */
export interface TaskView {
  /** The task's keyed hash: what the commands name; never shown. */
  task: string;
  /** Empty while the holder's note is not readable. */
  title: string;
  area: string | null;
  member: string;
  device: string;
  name: string;
  /** Held by this device: only it releases it or answers a request. */
  mine: boolean;
  /** Held by another device of this person. */
  own: boolean;
  /** Unix ms, the relay's clock. */
  since: number;
  /** When the hold runs out without activity; `null` while it cannot. */
  expiresAt: number | null;
  expiring: boolean;
  request: TaskRequest | null;
}

/** A reservation of this device that did not go through (yet) (Rust: `AttemptView`). */
export interface AttemptView {
  task: string;
  title: string;
  /** `blocked`: someone else was first. */
  state: "reserving" | "blocked";
  member: string | null;
  device: string | null;
  name: string;
  /** This person's release request is open. */
  asked: boolean;
  sessionId: number | null;
}

export type TaskNoticeKind =
  | "releaseAccepted"
  | "releaseDeclined"
  | "releaseTimedOut"
  | "expiring"
  | "expired"
  | "forceReleased"
  | "limit"
  | "askRefused"
  | "reserveFailed";

/** Something about this person's reservations, announced once (Rust: `TaskNotice`). */
export interface TaskNotice {
  id: number;
  kind: TaskNoticeKind;
  title: string;
  member: string;
  name: string;
  /** `expiring`: when the hold ends (Unix ms). */
  at: number | null;
  sessionId: number | null;
}

/** What the Team view shows of one shared project (Rust: `TeamActivity`). */
export interface TeamActivity {
  linked: RepoRef[];
  peers: PeerView[];
  overlaps: TeammateOverlap[];
  divergent: DivergentDevice[];
  tasks: TaskView[];
  attempts: AttemptView[];
  notices: TaskNotice[];
  /** This device's sessions shared with the project: the ones a reservation can be linked to. */
  sessions: number[];
  /** The repository of each of `sessions` (Rust: `SessionRepo`). */
  sessionRepos: SessionRepo[];
}

/** One of this device's shared sessions and the root commit of its repository. */
export interface SessionRepo {
  sessionId: number;
  repo: string;
}

export const EMPTY_ACTIVITY: TeamActivity = {
  linked: [],
  peers: [],
  overlaps: [],
  divergent: [],
  tasks: [],
  attempts: [],
  notices: [],
  sessions: [],
  sessionRepos: [],
};

export interface TeamEntry {
  projectId: string;
  status: TeamStatus;
  activity: TeamActivity;
}

/** Payload of `team-changed`. A project missing from `teams` is off. */
export interface TeamSnapshot {
  teams: TeamEntry[];
  pending: PendingConfirmation[];
  /** Repositories of this device's running sessions, offered for linking. */
  repos: RepoRef[];
}

export type TeamErrorKind =
  | "notSignedIn"
  | "unknownProject"
  | "unknownRequest"
  | "unknownRepository"
  | "invalidTask"
  | "unknownTask"
  | "taskLimit"
  | "keychain"
  | "config"
  | "busy"
  | "unknown";

/** A rejected team command (Rust: `{ kind, detail }`). */
export interface TeamProblem {
  kind: TeamErrorKind;
  detail: string | null;
}

const REASONS: readonly AttentionReason[] = [
  "historyMismatch",
  "signedOut",
  "keychain",
  "needsUpdate",
  "refused",
  "storage",
  "otherInstance",
  "otherAccount",
];
const KINDS: readonly ConfirmationKind[] = ["ownDevice", "newMember"];
const ERROR_KINDS: readonly TeamErrorKind[] = [
  "notSignedIn",
  "unknownProject",
  "unknownRequest",
  "unknownRepository",
  "invalidTask",
  "unknownTask",
  "taskLimit",
  "keychain",
  "config",
  "busy",
];

const SAFETY_GROUPS = 6;
const SAFETY_GROUP = /^\d{5}$/;

function text(value: unknown): string | null {
  return typeof value === "string" && value !== "" ? value : null;
}

/** Exactly 6 groups of 5 digits; anything else is malformed, since the person compares them. */
function parseSafetyNumber(value: unknown): string[] | null {
  const groups = list(value, (group) =>
    typeof group === "string" && SAFETY_GROUP.test(group) ? group : null,
  );
  return groups !== null && groups.length === SAFETY_GROUPS ? groups : null;
}

/**
 * Checks one team status; `null` if a known state is malformed. A state this Hangar does not know
 * becomes "needs attention" with an unknown reason, so it never reads as off or connected.
 */
export function parseTeamStatus(value: unknown): TeamStatus | null {
  const state = field(value, "state");
  switch (state) {
    case "off":
    case "removed":
    case "projectDeleted":
      return { state };
    case "connecting": {
      const retrying = field(value, "retrying");
      return typeof retrying === "boolean" ? { state, retrying } : null;
    }
    case "connected": {
      const epoch = field(value, "epoch");
      const members = field(value, "members");
      const devices = field(value, "devices");
      if (!isCount(epoch) || !isCount(members) || !isCount(devices)) return null;
      return { state, epoch, members, devices };
    }
    case "waitingForConfirmation": {
      const safetyNumber = parseSafetyNumber(field(value, "safetyNumber"));
      return safetyNumber === null ? null : { state, safetyNumber };
    }
    case "needsAttention": {
      const reason = field(value, "reason");
      if (typeof reason !== "string") return null;
      return { state, reason: REASONS.find((entry) => entry === reason) ?? "unknown" };
    }
    default:
      return typeof state === "string" && state !== ""
        ? { state: "needsAttention", reason: "unknown" }
        : null;
  }
}

function parsePending(value: unknown): PendingConfirmation | null {
  const id = text(field(value, "id"));
  const projectId = text(field(value, "projectId"));
  const member = text(field(value, "member"));
  const device = text(field(value, "device"));
  const kindValue = field(value, "kind");
  const safetyNumber = parseSafetyNumber(field(value, "safetyNumber"));
  const readmitted = field(value, "readmitted");
  if (id === null || projectId === null || member === null || device === null) return null;
  if (typeof kindValue !== "string" || safetyNumber === null) return null;
  // Missing counts as malformed, never as "not readmitted": the warning must not get lost.
  if (typeof readmitted !== "boolean") return null;
  const kind = KINDS.find((entry) => entry === kindValue) ?? "unknown";
  return { id, projectId, member, device, kind, safetyNumber, readmitted };
}

/** Checks the list of pending confirmations; `null` if an entry is malformed or an id repeats. */
export function parsePendingConfirmations(value: unknown): PendingConfirmation[] | null {
  const pending = list(value, parsePending);
  if (pending === null) return null;
  return new Set(pending.map((entry) => entry.id)).size === pending.length ? pending : null;
}

const SESSION_STATES: readonly SessionState[] = ["working", "waiting", "idle"];
const PRESENCES: readonly PeerView["presence"][] = ["active", "idle", "away"];
const OVERLAP_LEVELS: readonly OverlapLevel[] = [
  "none",
  "sameArea",
  "sameFile",
  "overlappingLines",
];

function isText(value: unknown): value is string {
  return typeof value === "string";
}

function texts(value: unknown): string[] | null {
  return list(value, (item) => (isText(item) ? item : null));
}

function parsePeerSession(value: unknown): PeerSession | null {
  const key = text(field(value, "key"));
  const agent = field(value, "agent");
  const title = field(value, "title");
  const state = SESSION_STATES.find((entry) => entry === field(value, "state"));
  const repo = text(field(value, "repo"));
  const files = texts(field(value, "files"));
  const moreFiles = field(value, "moreFiles");
  if (key === null || !isText(agent) || !isText(title) || repo === null) return null;
  if (files === null || !isCount(moreFiles)) return null;
  // A state this Hangar does not know reads as idle rather than dropping the whole team view.
  return { key, agent, title, state: state ?? "idle", repo, files, moreFiles };
}

function parsePeer(value: unknown): PeerView | null {
  const member = text(field(value, "member"));
  const device = text(field(value, "device"));
  const name = field(value, "name");
  const own = field(value, "own");
  const presence = PRESENCES.find((entry) => entry === field(value, "presence")) ?? "idle";
  const sessions = list(field(value, "sessions"), parsePeerSession);
  if (member === null || device === null || !isText(name) || typeof own !== "boolean") return null;
  if (sessions === null) return null;
  // The rows are keyed by session key: a repeat would be malformed (the backend drops them).
  if (new Set(sessions.map((session) => session.key)).size !== sessions.length) return null;
  return { member, device, name, own, presence, sessions };
}

function parseTeammateOverlap(value: unknown): TeammateOverlap | null {
  const sessionId = field(value, "sessionId");
  const member = text(field(value, "member"));
  const device = text(field(value, "device"));
  const otherSession = text(field(value, "otherSession"));
  const name = field(value, "name");
  const agent = field(value, "agent");
  const title = field(value, "title");
  const level = OVERLAP_LEVELS.find((entry) => entry === field(value, "level"));
  const pathValue = field(value, "path");
  const lines = list(field(value, "lines"), parseRange);
  const files = field(value, "files");
  if (!isCount(sessionId) || member === null || device === null || otherSession === null) {
    return null;
  }
  if (!isText(name) || !isText(agent) || !isText(title) || level === undefined) return null;
  if (pathValue !== null && !isText(pathValue)) return null;
  if (lines === null || !isCount(files)) return null;
  return {
    sessionId,
    member,
    device,
    otherSession,
    name,
    agent,
    title,
    level,
    path: pathValue,
    lines,
    files,
  };
}

/** A linked repository; its folder may be `null` (a link from before folders were bound). */
function parseLinked(value: unknown): RepoRef | null {
  const root = text(field(value, "root"));
  const folderValue = field(value, "folder");
  const folder = folderValue === undefined || folderValue === null ? null : text(folderValue);
  const name = field(value, "name");
  if (root === null || !isText(name)) return null;
  if (folderValue !== undefined && folderValue !== null && folder === null) return null;
  return { root, folder, name };
}

/** A repository folder offered for linking: always with its folder. */
function parseRepo(value: unknown): RepoRef | null {
  const repo = parseLinked(value);
  return repo === null || repo.folder === null ? null : repo;
}

function parseDivergent(value: unknown): DivergentDevice | null {
  const member = text(field(value, "member"));
  const device = text(field(value, "device"));
  const name = field(value, "name");
  if (member === null || device === null || !isText(name)) return null;
  return { member, device, name };
}

const TASK_HASH = /^[0-9a-f]{64}$/;

function isTime(value: unknown): value is number {
  return typeof value === "number" && Number.isFinite(value) && value >= 0;
}

function nullableText(value: unknown): string | null | undefined {
  if (value === null) return null;
  return isText(value) ? value : undefined;
}

function parseRequest(value: unknown): TaskRequest | null | undefined {
  if (value === null) return null;
  const member = text(field(value, "member"));
  const name = field(value, "name");
  const mine = field(value, "mine");
  const confirmed = field(value, "confirmed");
  if (member === null || !isText(name) || typeof mine !== "boolean") return undefined;
  // Missing counts as unconfirmed: without the requester's signature no card is shown.
  return { member, name, mine, confirmed: confirmed === true };
}

function parseTask(value: unknown): TaskView | null {
  const task = field(value, "task");
  const title = field(value, "title");
  const area = nullableText(field(value, "area"));
  const member = text(field(value, "member"));
  const device = text(field(value, "device"));
  const name = field(value, "name");
  const mine = field(value, "mine");
  const own = field(value, "own");
  const since = field(value, "since");
  const expiresAtValue = field(value, "expiresAt");
  const expiring = field(value, "expiring");
  const request = parseRequest(field(value, "request"));
  if (typeof task !== "string" || !TASK_HASH.test(task) || !isText(title)) return null;
  if (area === undefined || member === null || device === null || !isText(name)) return null;
  if (typeof mine !== "boolean" || typeof own !== "boolean" || typeof expiring !== "boolean") {
    return null;
  }
  if (!isTime(since) || (expiresAtValue !== null && !isTime(expiresAtValue))) return null;
  if (request === undefined) return null;
  return {
    task,
    title,
    area,
    member,
    device,
    name,
    mine,
    own,
    since,
    expiresAt: expiresAtValue,
    expiring,
    request,
  };
}

function parseAttempt(value: unknown): AttemptView | null {
  const task = field(value, "task");
  const title = text(field(value, "title"));
  const state = field(value, "state");
  const member = nullableText(field(value, "member"));
  const device = nullableText(field(value, "device"));
  const name = field(value, "name");
  const asked = field(value, "asked");
  const sessionId = field(value, "sessionId");
  // An attempt not sent yet has no hash; the window then only shows it.
  if (typeof task !== "string" || (task !== "" && !TASK_HASH.test(task)) || title === null) {
    return null;
  }
  if (state !== "reserving" && state !== "blocked") return null;
  if (member === undefined || device === undefined || !isText(name)) return null;
  if (typeof asked !== "boolean" || (sessionId !== null && !isCount(sessionId))) return null;
  return { task, title, state, member, device, name, asked, sessionId };
}

const NOTICE_KINDS: readonly TaskNoticeKind[] = [
  "releaseAccepted",
  "releaseDeclined",
  "releaseTimedOut",
  "expiring",
  "expired",
  "forceReleased",
  "limit",
  "askRefused",
  "reserveFailed",
];

/** A notice of a kind this Hangar does not know is dropped (`null` from `list` would drop all). */
function parseNotice(value: unknown): TaskNotice | null | "skip" {
  const id = field(value, "id");
  const kindValue = field(value, "kind");
  const title = field(value, "title");
  const member = field(value, "member");
  const name = field(value, "name");
  const at = field(value, "at");
  const sessionId = field(value, "sessionId");
  if (!isCount(id) || typeof kindValue !== "string" || !isText(title)) return null;
  if (!isText(member) || !isText(name)) return null;
  if ((at !== null && !isTime(at)) || (sessionId !== null && !isCount(sessionId))) return null;
  const kind = NOTICE_KINDS.find((entry) => entry === kindValue);
  if (kind === undefined) return "skip";
  return { id, kind, title, member, name, at, sessionId };
}

function parseNotices(value: unknown): TaskNotice[] | null {
  const parsed = list(value, (item) => {
    const notice = parseNotice(item);
    return notice === null ? null : { notice };
  });
  if (parsed === null) return null;
  return parsed.flatMap(({ notice }) => (notice === "skip" ? [] : [notice]));
}

/** An optional list: absent (an older backend) is empty, malformed is `null`. */
function optionalList<T>(value: unknown, parse: (value: unknown) => T[] | null): T[] | null {
  return value === undefined ? [] : parse(value);
}

function parseSessionRepo(value: unknown): SessionRepo | null {
  const sessionId = field(value, "sessionId");
  const repo = text(field(value, "repo"));
  return isCount(sessionId) && repo !== null ? { sessionId, repo } : null;
}

/** The activity of a team; absent counts as empty (an older backend), malformed as `null`. */
function parseActivity(value: unknown): TeamActivity | null {
  if (value === undefined) return EMPTY_ACTIVITY;
  const linked = list(field(value, "linked"), parseLinked);
  const peers = list(field(value, "peers"), parsePeer);
  const overlaps = list(field(value, "overlaps"), parseTeammateOverlap);
  const divergent = optionalList(field(value, "divergent"), (v) => list(v, parseDivergent));
  const tasks = optionalList(field(value, "tasks"), (v) => list(v, parseTask));
  const attempts = optionalList(field(value, "attempts"), (v) => list(v, parseAttempt));
  const notices = optionalList(field(value, "notices"), parseNotices);
  const sessions = optionalList(field(value, "sessions"), (v) =>
    list(v, (item) => (isCount(item) ? item : null)),
  );
  const sessionRepos = optionalList(field(value, "sessionRepos"), (v) => list(v, parseSessionRepo));
  if (linked === null || peers === null || overlaps === null || divergent === null) return null;
  if (tasks === null || attempts === null || notices === null || sessions === null) return null;
  if (sessionRepos === null) return null;
  return { linked, peers, overlaps, divergent, tasks, attempts, notices, sessions, sessionRepos };
}

function parseTeamEntry(value: unknown): TeamEntry | null {
  const projectId = text(field(value, "projectId"));
  const status = parseTeamStatus(field(value, "status"));
  const activity = parseActivity(field(value, "activity"));
  if (projectId === null || status === null || activity === null) return null;
  return { projectId, status, activity };
}

/** Checks the payload of `team-changed`; `null` if anything is malformed or a project repeats. */
export function parseTeamSnapshot(value: unknown): TeamSnapshot | null {
  const teams = list(field(value, "teams"), parseTeamEntry);
  const pending = parsePendingConfirmations(field(value, "pending"));
  const reposValue = field(value, "repos");
  const repos = reposValue === undefined ? [] : list(reposValue, parseRepo);
  if (teams === null || pending === null || repos === null) return null;
  if (new Set(teams.map((entry) => entry.projectId)).size !== teams.length) return null;
  return { teams, pending, repos };
}

/** The activities of a snapshot by project. */
export function activitiesOf(snapshot: TeamSnapshot): ReadonlyMap<string, TeamActivity> {
  return new Map(snapshot.teams.map((entry) => [entry.projectId, entry.activity]));
}

/** A rejected command as `{ kind, detail }`; anything else becomes `unknown`. */
export function parseTeamError(error: unknown): TeamProblem {
  const kind = ERROR_KINDS.find((entry) => entry === field(error, "kind")) ?? "unknown";
  const detailValue = field(error, "detail");
  const detail = typeof detailValue === "string" ? detailValue : null;
  if (kind === "unknown" && typeof error === "string") return { kind, detail: error };
  return { kind, detail };
}

/** The statuses of a snapshot by project; projects missing from it are off. */
export function statusesOf(snapshot: TeamSnapshot): ReadonlyMap<string, TeamStatus> {
  return new Map(snapshot.teams.map((entry) => [entry.projectId, entry.status]));
}

const OFF: TeamStatus = { state: "off" };

export function statusFor(
  statuses: ReadonlyMap<string, TeamStatus>,
  projectId: string,
): TeamStatus {
  return statuses.get(projectId) ?? OFF;
}

/** Whether "Share with team" is on: every state except off and the two where sharing stopped. */
export function isSharing(status: TeamStatus): boolean {
  return status.state !== "off" && status.state !== "removed" && status.state !== "projectDeleted";
}

/** A deleted project cannot be shared again; every other state can be toggled. */
export function canToggle(status: TeamStatus): boolean {
  return status.state !== "projectDeleted";
}

/** How a status looks next to its text: never by color alone, the text always says it. */
export type StatusTone = "ok" | "busy" | "problem" | "stopped";

export interface StatusLine {
  tone: StatusTone;
  text: string;
}

const REASON_TEXTS: Record<AttentionReason, MessageKey> = {
  historyMismatch: "team.status.historyMismatch",
  signedOut: "team.status.signedOut",
  keychain: "team.status.keychain",
  needsUpdate: "team.status.needsUpdate",
  refused: "team.status.refused",
  storage: "team.status.storage",
  otherInstance: "team.status.otherInstance",
  otherAccount: "team.status.otherAccount",
  unknown: "team.status.attention",
};

/** The text key of a status; `null` for off, which shows nothing below the switch. */
export function statusKey(status: TeamStatus): MessageKey | null {
  switch (status.state) {
    case "off":
      return null;
    case "connecting":
      return status.retrying ? "team.status.retrying" : "team.status.connecting";
    case "connected":
      return "team.status.connected";
    case "waitingForConfirmation":
      return "team.status.waiting";
    case "needsAttention":
      return REASON_TEXTS[status.reason];
    case "removed":
      return "team.status.removed";
    case "projectDeleted":
      return "team.status.projectDeleted";
  }
}

export function statusTone(status: TeamStatus): StatusTone {
  switch (status.state) {
    case "connected":
      return "ok";
    case "connecting":
      return status.retrying ? "problem" : "busy";
    case "waitingForConfirmation":
      return "busy";
    case "needsAttention":
      // Another window holds the connection: this one waits, nothing is broken.
      return status.reason === "otherInstance" ? "busy" : "problem";
    case "off":
    case "removed":
    case "projectDeleted":
      return "stopped";
  }
}

/** The line below the switch; `null` while off. Connected adds members and devices. */
export function statusLine(status: TeamStatus, t: Translate): StatusLine | null {
  const key = statusKey(status);
  if (key === null) return null;
  const parts = [t(key)];
  if (status.state === "connected") {
    parts.push(t("team.status.members", { count: status.members }));
    parts.push(t("team.status.devices", { count: status.devices }));
  }
  return { tone: statusTone(status), text: parts.join(" · ") };
}

const KIND_TEXTS: Record<ConfirmationKind, MessageKey> = {
  ownDevice: "team.confirm.ownDevice",
  newMember: "team.confirm.newMember",
  unknown: "team.confirm.unknown",
};

export function confirmationKindText(kind: ConfirmationKind, t: Translate): string {
  return t(KIND_TEXTS[kind]);
}

const ERROR_TEXTS: Record<TeamErrorKind, MessageKey> = {
  notSignedIn: "account.error.notSignedIn",
  unknownProject: "team.error.unknownProject",
  unknownRequest: "team.error.unknownRequest",
  unknownRepository: "team.error.unknownRepository",
  invalidTask: "team.error.invalidTask",
  unknownTask: "team.error.unknownTask",
  taskLimit: "team.error.taskLimit",
  keychain: "team.status.keychain",
  config: "account.error.config",
  busy: "team.error.busy",
  unknown: "error.unknown",
};

export function teamErrorText(problem: TeamProblem, t: Translate): string {
  return t(ERROR_TEXTS[problem.kind]);
}

const SHORT_ID = 8;

/** An opaque id cut to its first characters when long; the full id goes into the tooltip. */
export function shortId(id: string): string {
  return id.length > SHORT_ID + 2 ? `${id.slice(0, SHORT_ID)}…` : id;
}

/** A pending list without the answered request, so it disappears at once. */
export function withoutRequest(
  pending: readonly PendingConfirmation[],
  id: string,
): PendingConfirmation[] {
  return pending.filter((entry) => entry.id !== id);
}

/**
 * How long Confirm and Reject stay disabled after a request appears, so a stray Enter or a
 * double click meant for the previous request cannot answer one nobody has looked at yet.
 */
export const ARM_DELAY_MS = 1000;

/**
 * The one request to show: the one shown before while it is still pending, so the panel never
 * swaps under the pointer, otherwise the first one; `null` when nothing waits.
 */
export function shownRequest(
  pending: readonly PendingConfirmation[],
  previousId: string | null,
): PendingConfirmation | null {
  return pending.find((entry) => entry.id === previousId) ?? pending[0] ?? null;
}

/** Where the shown request stands in the queue (1 based); `null` when it is not in it. */
export function requestPosition(
  pending: readonly PendingConfirmation[],
  id: string,
): { index: number; count: number } | null {
  const index = pending.findIndex((entry) => entry.id === id);
  return index === -1 ? null : { index: index + 1, count: pending.length };
}

/** Milliseconds until the answers of a request shown at `shownAt` may be used; 0 when armed. */
export function armDelayLeft(shownAt: number, now: number, delay = ARM_DELAY_MS): number {
  // A clock running backwards counts as just shown rather than as long armed.
  if (now < shownAt) return delay;
  return Math.max(0, delay - (now - shownAt));
}

/** Whether Confirm and Reject of a request shown at `shownAt` may be used at `now`. */
export function isArmed(shownAt: number, now: number, delay = ARM_DELAY_MS): boolean {
  return armDelayLeft(shownAt, now, delay) === 0;
}

// -------------------------------------------------------------------------------------------
// Teammates (part 2b)
// -------------------------------------------------------------------------------------------

/** What the window's attention logic says a session does, as the team's presence names it. */
export function sessionStateOf(state: AttentionState | undefined): SessionState {
  switch (state) {
    case "working":
      return "working";
    case "attention":
      return "waiting";
    case "idle":
    case undefined:
      return "idle";
  }
}

/** The sessions' states for `team_session_states`, in the order of `ids`. */
export function sessionStates(
  ids: readonly number[],
  attention: ReadonlyMap<number, AttentionState>,
): { sessionId: number; state: SessionState }[] {
  return ids.map((sessionId) => ({ sessionId, state: sessionStateOf(attention.get(sessionId)) }));
}

function sameFolder(a: RepoRef, b: RepoRef): boolean {
  return a.root === b.root && a.folder !== null && a.folder === b.folder;
}

/**
 * The repository folders to offer: the linked ones first, then the running sessions' others. A
 * clone or fork of a linked repository in another folder is a choice of its own, unticked. Links
 * from before folders were bound (`folder: null`) are not offered: they share nothing.
 */
export function repoChoices(
  linked: readonly RepoRef[],
  running: readonly RepoRef[],
): { repo: RepoRef; linked: boolean }[] {
  const shown = linked
    .filter((repo) => repo.folder !== null)
    .map((repo) => ({ repo, linked: true }));
  for (const repo of running) {
    if (!linked.some((entry) => sameFolder(entry, repo))) shown.push({ repo, linked: false });
  }
  return shown;
}

/**
 * Whether a link from before folders were bound waits for the person: a running folder of that
 * repository is offered but not ticked, and nothing of it is shared until it is.
 */
export function needsRelink(linked: readonly RepoRef[], running: readonly RepoRef[]): boolean {
  return linked.some(
    (old) =>
      old.folder === null &&
      running.some(
        (repo) => repo.root === old.root && !linked.some((entry) => sameFolder(entry, repo)),
      ),
  );
}

/** Links from before folders were bound: they share nothing, and the person may remove them. */
export function legacyLinks(linked: readonly RepoRef[]): RepoRef[] {
  return linked.filter((repo) => repo.folder === null);
}

/** A clone's folder as the person knows it: the git folder's parent (`c:/src/shop/.git`). */
export function folderLabel(folder: string): string {
  return folder.replace(/\/\.git$/i, "");
}

/** "Anna", or the member's shortened id when the device sent no name; own devices say so. */
export function personName(
  person: { name: string; member: string; own?: boolean },
  t: Translate,
): string {
  if (person.own === true) return t("team.activity.you");
  // Names come from teammates' devices: invisible and reordering characters show as marks.
  const name = revealInvisible(person.name.trim());
  return name === "" ? t("team.confirm.member", { id: shortId(person.member) }) : name;
}

/**
 * "Anna #3f9a2c1b…": the self-chosen name with the short member id the verified log vouches for,
 * so two teammates who call themselves the same stay apart in texts. Own devices and nameless
 * teammates read as `personName` says.
 */
export function personTag(
  person: { name: string; member: string; own?: boolean },
  t: Translate,
): string {
  const name = personName(person, t);
  if (person.own === true || person.name.trim() === "") return name;
  return `${name} #${shortId(person.member)}`;
}

/** "Anna #3f9a2c1b… (Claude Code)": who changes something, for overlaps and notices. */
export function personWithAgent(
  person: { name: string; member: string; agent: string },
  t: Translate,
): string {
  const name = personTag(person, t);
  const agent = revealInvisible(person.agent.trim());
  return agent === "" ? name : `${name} (${agent})`;
}

/** "Anna #3f9a2c1b…, device 9c41d0e2…, reports a different team history. …" */
export function divergentText(device: DivergentDevice, t: Translate): string {
  return t("team.activity.divergent", {
    person: personTag(device, t),
    device: shortId(device.device),
  });
}

const STATE_TEXTS: Record<SessionState, MessageKey> = {
  working: "team.state.working",
  waiting: "team.state.waiting",
  idle: "team.state.idle",
};

export function sessionStateText(state: SessionState, t: Translate): string {
  return t(STATE_TEXTS[state]);
}

/** "Anna (Codex) is changing the same lines: src/x.ts, lines 3 to 9", or the file or area form. */
export function teammateOverlapText(overlap: TeammateOverlap, t: Translate): string {
  const person = personWithAgent(overlap, t);
  const path = revealInvisible(overlap.path ?? "");
  if (overlap.level === "overlappingLines" && overlap.path !== null) {
    return t("claims.teammateLines", { person, path, lines: formatLines(overlap.lines, t) });
  }
  if (overlap.level === "sameFile" && overlap.path !== null) {
    return t("claims.teammateFile", { person, path });
  }
  return t("claims.teammateArea", { person });
}

/** Same file or overlapping lines, with a file this device can name: worth a marker and notice. */
export function isTeammateFileOverlap(overlap: TeammateOverlap): boolean {
  return (
    overlap.path !== null && (overlap.level === "sameFile" || overlap.level === "overlappingLines")
  );
}

/** Every teammate overlap of every shared project, by our session, worst first per session. */
export function teammateOverlapsBySession(
  activities: ReadonlyMap<string, TeamActivity>,
): Map<number, TeammateOverlap[]> {
  const grouped = new Map<number, TeammateOverlap[]>();
  const rank = (level: OverlapLevel) => OVERLAP_LEVELS.indexOf(level);
  for (const activity of activities.values()) {
    for (const overlap of activity.overlaps) {
      const entries = grouped.get(overlap.sessionId);
      if (entries === undefined) grouped.set(overlap.sessionId, [overlap]);
      else entries.push(overlap);
    }
  }
  for (const entries of grouped.values()) entries.sort((a, b) => rank(b.level) - rank(a.level));
  return grouped;
}

/** The sidebar marker's text for a session's worst teammate overlap on a file; `null` if none. */
export function teammateMarkerText(
  overlaps: readonly TeammateOverlap[],
  t: Translate,
): string | null {
  const files = overlaps.filter(isTeammateFileOverlap);
  const first = files[0];
  if (first === undefined) return null;
  const text = teammateOverlapText(first, t);
  return files.length > 1 ? `${text} ${t("claims.moreCount", { count: files.length - 1 })}` : text;
}

/** Same for every notice of one collision: our session, the teammate's device and the file. */
export function teammateOverlapKey(overlap: TeammateOverlap): string {
  return [String(overlap.sessionId), overlap.member, overlap.device, overlap.path ?? ""].join(
    "\u0000",
  );
}

/** File-level teammate overlaps not announced yet; the caller adds their keys to `seen`. */
export function freshTeammateOverlaps(
  seen: ReadonlySet<string>,
  activities: ReadonlyMap<string, TeamActivity>,
): TeammateOverlap[] {
  const fresh = new Map<string, TeammateOverlap>();
  for (const activity of activities.values()) {
    for (const overlap of activity.overlaps) {
      if (!isTeammateFileOverlap(overlap)) continue;
      const key = teammateOverlapKey(overlap);
      if (!seen.has(key) && !fresh.has(key)) fresh.set(key, overlap);
    }
  }
  return [...fresh.values()];
}

/** Collision keys remembered per app run; the oldest go first beyond this. */
export const MAX_SEEN_TEAMMATE_OVERLAPS = 2000;

/** Desktop notifications about teammates come at most this often. */
export const TEAMMATE_NOTICE_GAP_MS = 60_000;

/** Adds `keys` to `seen` and forgets the oldest beyond `max`, so the set stays bounded. */
export function rememberSeen(
  seen: Set<string>,
  keys: readonly string[],
  max = MAX_SEEN_TEAMMATE_OVERLAPS,
): void {
  for (const key of keys) {
    seen.delete(key);
    seen.add(key);
  }
  for (const key of seen) {
    if (seen.size <= max) break;
    seen.delete(key);
  }
}

/**
 * Whether a desktop notification about a teammate may go out at `now`, after the last one at
 * `lastAt`: however many collisions teammates' devices report, the desktop hears of them at most
 * once per `TEAMMATE_NOTICE_GAP_MS` (the sidebar markers and the in-app notice still show all).
 */
export function teammateNoticeDue(
  lastAt: number | null,
  now: number,
  gap = TEAMMATE_NOTICE_GAP_MS,
): boolean {
  // A clock running backwards counts as just notified.
  return lastAt === null || (now >= lastAt && now - lastAt >= gap);
}

/** "Anna (Codex) is changing the same file as Claude 1" with the file and lines as body. */
export function teammateNotice(
  overlap: TeammateOverlap,
  sessionName: string,
  t: Translate,
): { title: string; body: string } {
  const lines = overlap.level === "overlappingLines" ? formatLines(overlap.lines, t) : "";
  const path = revealInvisible(overlap.path ?? "");
  return {
    title: t("claims.teammateNotice", {
      person: personWithAgent(overlap, t),
      session: sessionName,
    }),
    body: lines === "" ? path : `${path}, ${lines}`,
  };
}

// -------------------------------------------------------------------------------------------
// Reservations ("I'm working on ...")
// -------------------------------------------------------------------------------------------

/** The holder of a reservation as the Team view names it: "You" on this device. */
export function holderName(task: TaskView, t: Translate): string {
  if (task.mine) return t("team.tasks.you");
  return personTag(task, t);
}

/** A reservation's title; one whose note is not readable yet says so. */
export function taskTitle(title: string, t: Translate): string {
  const shown = revealInvisible(title.trim());
  return shown === "" ? t("team.tasks.untitled") : shown;
}

/**
 * What a waiting attempt says: being reserved, or who was first; "your agent waits" only when the
 * attempt is linked to a session (the gate pauses that session, nothing else).
 */
export function attemptText(attempt: AttemptView, t: Translate): string {
  if (attempt.state !== "blocked" || attempt.member === null) return t("team.tasks.reserving");
  const person = personTag({ name: attempt.name, member: attempt.member }, t);
  return attempt.sessionId === null
    ? t("team.tasks.blockedNoSession", { person })
    : t("team.tasks.blocked", { person });
}

/** The running sessions a reservation can be linked to: those the project shares. */
export function linkableSessions<T extends { id: number }>(
  sessions: readonly T[],
  activity: TeamActivity,
): T[] {
  return sessions.filter((session) => activity.sessions.includes(session.id));
}

/** The release requests waiting for this device's answer, oldest reservation first. */
export function releaseRequests(
  activities: ReadonlyMap<string, TeamActivity>,
): { projectId: string; task: TaskView }[] {
  const requests: { projectId: string; task: TaskView }[] = [];
  for (const [projectId, activity] of activities) {
    for (const task of activity.tasks) {
      // Only a request one of the requester's listed devices signed gets a card.
      if (task.mine && task.request?.confirmed === true) requests.push({ projectId, task });
    }
  }
  return requests;
}

/** Same for every announcement of one release request. */
export function releaseRequestKey(projectId: string, task: TaskView): string {
  return [projectId, task.task, task.request?.member ?? ""].join("\u0000");
}

/** Whether "Ask to release" fits: someone else's readable reservation without an open request. */
export function canAskRelease(task: TaskView): boolean {
  return !task.mine && !task.own && task.request === null && task.title.trim() !== "";
}

const NOTICE_TEXTS: Record<TaskNoticeKind, MessageKey> = {
  releaseAccepted: "team.notice.releaseAccepted",
  releaseDeclined: "team.notice.releaseDeclined",
  releaseTimedOut: "team.notice.releaseTimedOut",
  expiring: "team.notice.expiring",
  expired: "team.notice.expired",
  forceReleased: "team.notice.forceReleased",
  limit: "team.notice.limit",
  askRefused: "team.notice.askRefused",
  reserveFailed: "team.notice.reserveFailed",
};

/** The text of a notice; `time` formats the deadline of a warning. */
export function taskNoticeText(
  notice: TaskNotice,
  t: Translate,
  time: (ms: number) => string,
): string {
  return t(NOTICE_TEXTS[notice.kind], {
    title: taskTitle(notice.title, t),
    person: personTag(notice, t),
    time: notice.at === null ? "" : time(notice.at),
  });
}

/** Notices of every shared project not announced yet; the caller adds their keys to `seen`. */
export function freshTaskNotices(
  seen: ReadonlySet<string>,
  activities: ReadonlyMap<string, TeamActivity>,
): { key: string; notice: TaskNotice }[] {
  const fresh: { key: string; notice: TaskNotice }[] = [];
  for (const [projectId, activity] of activities) {
    for (const notice of activity.notices) {
      const key = `${projectId}\u0000${String(notice.id)}`;
      if (!seen.has(key)) fresh.push({ key, notice });
    }
  }
  return fresh;
}

/** A time of day in the window's locale ("14:05"), for reservation deadlines. */
export function clockTime(ms: number): string {
  return new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" }).format(
    new Date(ms),
  );
}
