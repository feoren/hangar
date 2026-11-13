// The Hangar account in the window (team service step F, part 1): checks the backend's account
// state and project list (Rust: account.rs, serde camelCase) and builds the texts of the Account
// section in Settings and the Team group in the sidebar. The backend owns the sign-in, the
// tokens and every request; the window never sees a token or passes a URL.

import { field, isCount, list } from "./claims";
import type { MessageKey, Translate } from "./i18n";

export type FailKind =
  | "denied"
  | "expired"
  | "refused"
  | "network"
  | "server"
  | "foreignPage"
  | "keychain"
  | "badResponse";

export interface FailReason {
  kind: FailKind;
  /** The server's own text, already cleaned by the backend; may be `null`. */
  detail: string | null;
}

/**
 * `signedOut`: signed out and removed from the account on the server; `signedOutLocally`: signed
 * out here, but the server did not confirm the sign-out; `sessionEnded`: the server refused the
 * sign-in.
 */
export type IdleNotice = "signedOut" | "signedOutLocally" | "sessionEnded";

/** The signed-in person (Rust: `Me`). Seats are those of the person's own account. */
export interface Me {
  id: string;
  email: string;
  name: string | null;
  seatsAccountTotal: number | null;
  seatsAccountUsed: number | null;
}

/** The sign-in state machine (Rust: `AccountState`). */
export type AccountState =
  | { kind: "idle"; notice: IdleNotice | null }
  | {
      kind: "waitingForUser";
      userCode: string;
      verificationUri: string;
      verificationUriComplete: string;
      /** Unix ms. */
      expiresAt: number;
      interval: number;
    }
  | { kind: "signedIn"; user: Me | null }
  | { kind: "failed"; reason: FailReason };

/** Payload of `account-changed` (Rust: `AccountView`). */
export interface AccountView {
  baseUrl: string;
  official: boolean;
  /** The account page listing connected devices. */
  devicesUrl: string;
  state: AccountState;
  /** Other servers whose sign-in stays stored on this device (base addresses). */
  otherSignIns: string[];
}

export type Role = "owner" | "admin" | "member" | "other";

/** One project, i.e. one team (Rust: `Project`). Seats count per account of the owner. */
export interface Project {
  id: string;
  name: string;
  role: Role;
  plan: string | null;
  seatsAccountTotal: number | null;
  seatsAccountUsed: number | null;
  /** Only owners and admins get it. */
  pendingInvitations: number | null;
  /** The backend checked that the invite page lies on the server's own origin. */
  canInvite: boolean;
}

/** Pages the backend opens (Rust: `AccountPage`). */
export type AccountPage =
  { kind: "verification" } | { kind: "devices" } | { kind: "invite"; projectId: string };

export type AccountErrorKind =
  | "notSignedIn"
  | "sessionEnded"
  | "network"
  | "server"
  | "badResponse"
  | "baseUrl"
  | "foreignPage"
  | "keychain"
  | "open"
  | "unknownProject"
  | "nothingToOpen"
  | "config"
  | "unknown";

/** A rejected account command (Rust: `AccountError` as `{ kind, detail }`). */
export interface AccountProblem {
  kind: AccountErrorKind;
  detail: string | null;
}

const FAIL_KINDS: readonly FailKind[] = [
  "denied",
  "expired",
  "refused",
  "network",
  "server",
  "foreignPage",
  "keychain",
  "badResponse",
];
const ROLES: readonly Role[] = ["owner", "admin", "member", "other"];
const ERROR_KINDS: readonly AccountErrorKind[] = [
  "notSignedIn",
  "sessionEnded",
  "network",
  "server",
  "badResponse",
  "baseUrl",
  "foreignPage",
  "keychain",
  "open",
  "unknownProject",
  "nothingToOpen",
  "config",
];

function text(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

/** A string, `null` or missing; `undefined` for anything else (malformed). */
function optionalText(value: unknown): string | null | undefined {
  if (value === null || value === undefined) return null;
  return typeof value === "string" ? value : undefined;
}

function optionalCount(value: unknown): number | null | undefined {
  if (value === null || value === undefined) return null;
  return isCount(value) ? value : undefined;
}

function parseMe(value: unknown): Me | null {
  const id = text(field(value, "id"));
  const email = text(field(value, "email"));
  const name = optionalText(field(value, "name"));
  const total = optionalCount(field(value, "seatsAccountTotal"));
  const used = optionalCount(field(value, "seatsAccountUsed"));
  if (id === null || email === null || name === undefined) return null;
  if (total === undefined || used === undefined) return null;
  return { id, email, name, seatsAccountTotal: total, seatsAccountUsed: used };
}

function parseState(value: unknown): AccountState | null {
  switch (field(value, "kind")) {
    case "idle": {
      const notice = field(value, "notice");
      if (notice === null || notice === undefined) return { kind: "idle", notice: null };
      return notice === "signedOut" || notice === "signedOutLocally" || notice === "sessionEnded"
        ? { kind: "idle", notice }
        : null;
    }
    case "waitingForUser": {
      const userCode = text(field(value, "userCode"));
      const verificationUri = text(field(value, "verificationUri"));
      const verificationUriComplete = text(field(value, "verificationUriComplete"));
      const expiresAt = field(value, "expiresAt");
      const interval = field(value, "interval");
      if (userCode === null || verificationUri === null || verificationUriComplete === null) {
        return null;
      }
      if (!isCount(expiresAt) || !isCount(interval)) return null;
      return {
        kind: "waitingForUser",
        userCode,
        verificationUri,
        verificationUriComplete,
        expiresAt,
        interval,
      };
    }
    case "signedIn": {
      const user = field(value, "user");
      if (user === null || user === undefined) return { kind: "signedIn", user: null };
      const me = parseMe(user);
      return me === null ? null : { kind: "signedIn", user: me };
    }
    case "failed": {
      const reason = field(value, "reason");
      const kind = FAIL_KINDS.find((entry) => entry === field(reason, "kind"));
      const detail = optionalText(field(reason, "detail"));
      if (kind === undefined || detail === undefined) return null;
      return { kind: "failed", reason: { kind, detail } };
    }
    default:
      return null;
  }
}

/** Checks the backend's account view; `null` if anything is malformed. */
export function parseAccountView(value: unknown): AccountView | null {
  const baseUrl = text(field(value, "baseUrl"));
  const official = field(value, "official");
  const devicesUrl = text(field(value, "devicesUrl"));
  const state = parseState(field(value, "state"));
  const otherSignIns = list(field(value, "otherSignIns"), text);
  if (baseUrl === null || devicesUrl === null || typeof official !== "boolean" || state === null) {
    return null;
  }
  if (otherSignIns === null) return null;
  return { baseUrl, official, devicesUrl, state, otherSignIns };
}

function parseProject(value: unknown): Project | null {
  const id = text(field(value, "id"));
  const name = text(field(value, "name"));
  const role = ROLES.find((entry) => entry === field(value, "role"));
  const plan = optionalText(field(value, "plan"));
  const total = optionalCount(field(value, "seatsAccountTotal"));
  const used = optionalCount(field(value, "seatsAccountUsed"));
  const pending = optionalCount(field(value, "pendingInvitations"));
  const canInvite = field(value, "canInvite");
  if (id === null || id === "" || name === null || role === undefined) return null;
  if (plan === undefined || total === undefined || used === undefined || pending === undefined) {
    return null;
  }
  if (typeof canInvite !== "boolean") return null;
  return {
    id,
    name,
    role,
    plan,
    seatsAccountTotal: total,
    seatsAccountUsed: used,
    pendingInvitations: pending,
    canInvite,
  };
}

/** Checks the project list; `null` if an entry is malformed or an id repeats. */
export function parseProjects(value: unknown): Project[] | null {
  const projects = list(value, parseProject);
  if (projects === null) return null;
  return new Set(projects.map((project) => project.id)).size === projects.length ? projects : null;
}

/** A rejected command as `{ kind, detail }`; anything else becomes `unknown`. */
export function parseAccountError(error: unknown): AccountProblem {
  const kind = ERROR_KINDS.find((entry) => entry === field(error, "kind")) ?? "unknown";
  const detail = text(field(error, "detail"));
  if (kind === "unknown" && typeof error === "string") return { kind, detail: error };
  return { kind, detail };
}

export function isSignedIn(view: AccountView | null): boolean {
  return view?.state.kind === "signedIn";
}

/**
 * What a new view means for the project list: load it when the account just signed in, clear it
 * when it no longer is, keep it otherwise.
 */
export function projectsAction(
  previous: AccountView | null,
  next: AccountView,
): "load" | "clear" | "keep" {
  const was = isSignedIn(previous);
  const is = isSignedIn(next);
  if (is && (!was || previous?.baseUrl !== next.baseUrl)) return "load";
  if (!is && was) return "clear";
  if (!is && previous !== null && previous.baseUrl !== next.baseUrl) return "clear";
  return "keep";
}

/** The main button of each state's block, which takes the focus when the state changes. */
export type FocusTarget = "signIn" | "copyCode" | "signOut" | "tryAgain";

const FOCUS_TARGETS: Record<AccountState["kind"], FocusTarget> = {
  idle: "signIn",
  waitingForUser: "copyCode",
  signedIn: "signOut",
  failed: "tryAgain",
};

/**
 * Where the focus goes after the state changed from `previous` to `next`: the main button of the
 * new block, so a keyboard user is not left on a button that disappeared. Nothing on the first
 * render or when the kind stayed the same.
 */
export function focusAfterChange(
  previous: AccountState["kind"] | null,
  next: AccountState["kind"],
): FocusTarget | null {
  if (previous === null || previous === next) return null;
  return FOCUS_TARGETS[next];
}

/** After a sign-out: which other servers keep a stored sign-in, or `null` when none do. */
export function otherSignInsText(view: AccountView, t: Translate): string | null {
  const { state } = view;
  const signedOut =
    state.kind === "idle" && (state.notice === "signedOut" || state.notice === "signedOutLocally");
  if (!signedOut || view.otherSignIns.length === 0) return null;
  return t("account.otherSignIns", { hosts: view.otherSignIns.map(serverHost).join(", ") });
}

/** Whole minutes left for the code, at least 1 while it still works. */
export function minutesLeft(expiresAt: number, now: number): number {
  return Math.max(1, Math.ceil((expiresAt - now) / 60_000));
}

/** Name to show for the signed-in person: the name, else the e-mail address. */
export function displayName(user: Me): string {
  const name = user.name?.trim() ?? "";
  return name === "" ? user.email : name;
}

/** What the account button in the sidebar footer shows: visible text, accessible name, tooltip. */
export interface AccountButtonText {
  text: string;
  label: string;
  title: string;
}

/**
 * Signed out: "Sign in" with a hint. Signed in: the person's name, or, while their details have
 * not arrived yet, a neutral "Signed in", never "Account: Account".
 */
export function accountButtonText(view: AccountView | null, t: Translate): AccountButtonText {
  if (view?.state.kind !== "signedIn") {
    const signIn = t("account.signIn");
    return { text: signIn, label: signIn, title: t("sidebar.signInHint") };
  }
  const user = view.state.user;
  if (user === null) {
    const signedIn = t("sidebar.signedIn");
    return { text: signedIn, label: signedIn, title: signedIn };
  }
  const name = displayName(user);
  const label = t("sidebar.account", { name });
  return { text: name, label, title: label };
}

/** Host (and port) of the server address, for short mentions. */
export function serverHost(baseUrl: string): string {
  try {
    return new URL(baseUrl).host;
  } catch {
    return baseUrl;
  }
}

const FAIL_TEXTS: Record<FailKind, MessageKey> = {
  denied: "account.fail.denied",
  expired: "account.fail.expired",
  refused: "account.fail.refused",
  network: "account.fail.network",
  server: "account.fail.server",
  foreignPage: "account.fail.foreignPage",
  keychain: "account.fail.keychain",
  badResponse: "account.fail.badResponse",
};

export function failText(reason: FailReason, t: Translate): string {
  return t(FAIL_TEXTS[reason.kind]);
}

const ERROR_TEXTS: Record<AccountErrorKind, MessageKey> = {
  notSignedIn: "account.error.notSignedIn",
  sessionEnded: "account.sessionEnded",
  network: "account.fail.network",
  server: "account.fail.server",
  badResponse: "account.fail.badResponse",
  baseUrl: "account.error.baseUrl",
  foreignPage: "account.fail.foreignPage",
  keychain: "account.fail.keychain",
  open: "account.error.open",
  unknownProject: "team.loadFailed",
  nothingToOpen: "account.error.notSignedIn",
  config: "account.error.config",
  unknown: "error.unknown",
};

export function accountErrorText(problem: AccountProblem, t: Translate): string {
  return t(ERROR_TEXTS[problem.kind]);
}

const ROLE_TEXTS: Record<Exclude<Role, "other">, MessageKey> = {
  owner: "team.role.owner",
  admin: "team.role.admin",
  member: "team.role.member",
};

/** The role's name; `null` for a role this Hangar does not know. */
export function roleText(role: Role, t: Translate): string | null {
  return role === "other" ? null : t(ROLE_TEXTS[role]);
}

/** "2 of 3 seats" for the owner's account, `null` when the server sent no numbers. */
export function seatsText(
  seats: { seatsAccountTotal: number | null; seatsAccountUsed: number | null },
  t: Translate,
): string | null {
  const { seatsAccountTotal: total, seatsAccountUsed: used } = seats;
  if (total === null || used === null) return null;
  return t("team.seats", { used, total });
}

/** The meta line of a project: plan, seats and pending invitations, those that are known. */
export function projectMeta(project: Project, t: Translate): string[] {
  const parts: string[] = [];
  if (project.plan !== null && project.plan.trim() !== "") parts.push(project.plan.trim());
  const seats = seatsText(project, t);
  if (seats !== null) parts.push(seats);
  if (project.pendingInvitations !== null && project.pendingInvitations > 0) {
    parts.push(t("team.pending", { count: project.pendingInvitations }));
  }
  return parts;
}
