// The team connection in the window (team service step F, part 2a): checks the backend's team
// state (Rust: serde camelCase, tagged by `state`) and builds the texts of the Team group in the
// sidebar. The backend owns the keys, the connection and every request; the window only shows the
// state, turns "Share with team" on or off and answers confirmations of new devices.

import { field, isCount, list } from "./claims";
import type { MessageKey, Translate } from "./i18n";

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

/** Payload of `team-changed`. A project missing from `teams` is off. */
export interface TeamSnapshot {
  teams: { projectId: string; status: TeamStatus }[];
  pending: PendingConfirmation[];
}

export type TeamErrorKind =
  "notSignedIn" | "unknownProject" | "unknownRequest" | "keychain" | "config" | "busy" | "unknown";

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
];
const KINDS: readonly ConfirmationKind[] = ["ownDevice", "newMember"];
const ERROR_KINDS: readonly TeamErrorKind[] = [
  "notSignedIn",
  "unknownProject",
  "unknownRequest",
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

function parseTeamEntry(value: unknown): { projectId: string; status: TeamStatus } | null {
  const projectId = text(field(value, "projectId"));
  const status = parseTeamStatus(field(value, "status"));
  return projectId === null || status === null ? null : { projectId, status };
}

/** Checks the payload of `team-changed`; `null` if anything is malformed or a project repeats. */
export function parseTeamSnapshot(value: unknown): TeamSnapshot | null {
  const teams = list(field(value, "teams"), parseTeamEntry);
  const pending = parsePendingConfirmations(field(value, "pending"));
  if (teams === null || pending === null) return null;
  if (new Set(teams.map((entry) => entry.projectId)).size !== teams.length) return null;
  return { teams, pending };
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
