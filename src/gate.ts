// The gate (team service step B): before Claude Code edits a file that another of the user's
// running sessions is changing, the backend holds the agent's hook request open and the person
// decides in Hangar. This module checks the backend's snapshot of pending decisions, picks the one
// to show, builds the card's texts and tells which decisions are new (chime, notification).

import type { GateStatus } from "./api";
import type { AttentionReason, AttentionState } from "./attention";
import { field, formatLines, isCount, list, parseRange, type LineRange } from "./claims";
import type { Translate } from "./i18n";
import { revealInvisible } from "./invisible";

/** `sameArea`: only a teammate's reserved folder covers the file. */
export type GateLevel = "sameArea" | "sameFile" | "overlappingLines";

/**
 * `alwaysFile`, `alwaysFolder`, `neverFile`, `neverFolder` answer this edit and set a gate
 * rule for the project, which the backend applies to later asks without a card.
 */
export type GateAnswer =
  "allowOnce" | "allowFile" | "deny" | "alwaysFile" | "alwaysFolder" | "neverFile" | "neverFolder";

export type RuleEffect = "allow" | "deny";
export type RuleScope = "file" | "folder";

/** One edit a gate rule answered without asking (Rust: `RuleAnswer` in gate_rules.rs). */
export interface RuleAnswer {
  /** Increasing per app run. */
  id: number;
  /** Unix ms. */
  at: number;
  sessionId: number;
  /** The session's title when the rule answered. */
  session: string;
  projectName: string;
  /** The edited file, relative to the worktree. */
  path: string;
  ruleId: number;
  rulePath: string;
  scope: RuleScope;
  effect: RuleEffect;
}

/** A gate rule of one project (Rust: `GateRule` in gate_rules.rs). */
export interface GateRule {
  id: number;
  projectName: string;
  /** Relative to the worktree, `/` separators. */
  path: string;
  scope: RuleScope;
  effect: RuleEffect;
  /** Unix ms. */
  createdAt: number;
}

/** One edit waiting for the person's answer (Rust: `PendingDecision`). */
export interface PendingDecision {
  /** Unique per app run. */
  id: number;
  /** The waiting session. */
  sessionId: number;
  /** "Edit", "Write", "MultiEdit", "NotebookEdit" or any other tool name; not shown. */
  tool: string;
  /** Relative to the session's worktree with `/` separators; shown via `revealInvisible`. */
  path: string;
  /** The user's own sessions changing the same file; empty when only teammates do. */
  otherSessions: number[];
  /** Teammates changing the file ("Anna (Claude Code)"), from a shared project's presence. */
  teammates: string[];
  /** Teammates' reservations whose folder covers the file ("Anna #3f9a2c1b…", "Fix login"). */
  reserved: { who: string; title: string }[];
  level: GateLevel;
  /** Lines the other sessions changed in that file; empty: whole file or unknown. */
  lines: LineRange[];
  /** Unix ms. */
  createdAt: number;
}

/** Payload of `gate-changed` and answer of `gate_snapshot`. */
export interface GateSnapshot {
  enabled: boolean;
  /** Oldest first (by id). */
  pending: PendingDecision[];
  /** Edits gate rules answered, newest first; older backends send none. */
  log: RuleAnswer[];
}

export const EMPTY_GATE: GateSnapshot = { enabled: false, pending: [], log: [] };

const EFFECTS: readonly RuleEffect[] = ["allow", "deny"];
const SCOPES: readonly RuleScope[] = ["file", "folder"];

function text(value: unknown): string | null {
  return typeof value === "string" && value !== "" ? value : null;
}

function parseRuleAnswer(value: unknown): RuleAnswer | null {
  const id = field(value, "id");
  const at = field(value, "at");
  const sessionId = field(value, "sessionId");
  const ruleId = field(value, "ruleId");
  const session = field(value, "session");
  const projectName = field(value, "projectName");
  const path = text(field(value, "path"));
  const rulePath = text(field(value, "rulePath"));
  const scope = SCOPES.find((entry) => entry === field(value, "scope"));
  const effect = EFFECTS.find((entry) => entry === field(value, "effect"));
  if (!isCount(id) || !isCount(at) || !isCount(sessionId) || !isCount(ruleId)) return null;
  if (typeof session !== "string" || typeof projectName !== "string") return null;
  if (path === null || rulePath === null || scope === undefined || effect === undefined) {
    return null;
  }
  return { id, at, sessionId, session, projectName, path, ruleId, rulePath, scope, effect };
}

function parseRule(value: unknown): GateRule | null {
  const id = field(value, "id");
  const createdAt = field(value, "createdAt");
  const projectName = field(value, "projectName");
  const path = text(field(value, "path"));
  const scope = SCOPES.find((entry) => entry === field(value, "scope"));
  const effect = EFFECTS.find((entry) => entry === field(value, "effect"));
  if (!isCount(id) || !isCount(createdAt) || typeof projectName !== "string") return null;
  if (path === null || scope === undefined || effect === undefined) return null;
  return { id, projectName, path, scope, effect, createdAt };
}

/**
 * The answer of `gate_rules`, sorted by project, then path. Malformed entries are left out, so
 * one broken rule never hides the others from Settings, where they can be deleted.
 */
export function parseGateRules(value: unknown): GateRule[] {
  if (!Array.isArray(value)) return [];
  return value
    .map(parseRule)
    .filter((rule): rule is GateRule => rule !== null)
    .sort(
      (a, b) =>
        a.projectName.localeCompare(b.projectName) || a.path.localeCompare(b.path) || a.id - b.id,
    );
}

/** The folder a "for this folder" answer covers; `null` for a file at the top of the project. */
export function ruleFolder(path: string): string | null {
  const slash = path.lastIndexOf("/");
  return slash > 0 ? path.slice(0, slash) : null;
}

/** How long the short note of a rule's answer stays on its session. */
export const RULE_NOTE_MS = 6000;

/**
 * Log entries newer than `lastSeen` (an entry id) and younger than `RULE_NOTE_MS` at `now`,
 * oldest first, for the short note. The age keeps a reloaded window from replaying old answers.
 */
export function freshRuleAnswers(
  lastSeen: number,
  log: readonly RuleAnswer[],
  now: number,
): RuleAnswer[] {
  return log
    .filter((entry) => entry.id > lastSeen && now - entry.at < RULE_NOTE_MS)
    .sort((a, b) => a.id - b.id);
}

/** "src/auth/" for a folder rule, the path itself for a file rule. */
export function rulePathLabel(path: string, scope: RuleScope): string {
  return scope === "folder" ? `${revealInvisible(path)}/` : revealInvisible(path);
}

/** The short note a rule's answer leaves on the session: "Rule allowed src/x.ts (src/)". */
export function ruleAnswerText(entry: RuleAnswer, t: Translate): string {
  return t(entry.effect === "allow" ? "gate.ruleAllowed" : "gate.ruleDenied", {
    path: revealInvisible(entry.path),
    rule: rulePathLabel(entry.rulePath, entry.scope),
  });
}

const LEVELS: readonly GateLevel[] = ["sameArea", "sameFile", "overlappingLines"];

function parseReserved(item: unknown): { who: string; title: string } | null {
  const who = field(item, "who");
  const title = field(item, "title");
  if (typeof who !== "string" || typeof title !== "string" || title === "") return null;
  return { who, title };
}

function parseDecision(value: unknown): PendingDecision | null {
  const id = field(value, "id");
  const sessionId = field(value, "sessionId");
  const tool = field(value, "tool");
  const path = field(value, "path");
  const otherSessions = list(field(value, "otherSessions"), (item) =>
    isCount(item) ? item : null,
  );
  const teammatesValue = field(value, "teammates");
  const teammates =
    teammatesValue === undefined
      ? []
      : list(teammatesValue, (item) => (typeof item === "string" && item !== "" ? item : null));
  const reservedValue = field(value, "reserved");
  const reserved = reservedValue === undefined ? [] : list(reservedValue, parseReserved);
  const level = LEVELS.find((entry) => entry === field(value, "level"));
  const lines = list(field(value, "lines"), parseRange);
  const createdAt = field(value, "createdAt");
  if (!isCount(id) || !isCount(sessionId) || typeof tool !== "string") return null;
  if (typeof path !== "string" || path === "" || level === undefined || lines === null) {
    return null;
  }
  // Someone else changes the file or reserved its folder: one of the user's sessions or a
  // teammate, at least one.
  if (otherSessions === null || teammates === null || reserved === null) return null;
  if (otherSessions.length === 0 && teammates.length === 0 && reserved.length === 0) return null;
  if (typeof createdAt !== "number" || !Number.isFinite(createdAt) || createdAt < 0) return null;
  return {
    id,
    sessionId,
    tool,
    path,
    otherSessions,
    teammates,
    reserved,
    level,
    lines,
    createdAt,
  };
}

/** Checks the backend's snapshot; `null` if anything is malformed or an id repeats. */
export function parseGateSnapshot(value: unknown): GateSnapshot | null {
  const enabled = field(value, "enabled");
  const pending = list(field(value, "pending"), parseDecision);
  if (typeof enabled !== "boolean" || pending === null) return null;
  const ids = new Set(pending.map((decision) => decision.id));
  if (ids.size !== pending.length) return null;
  // The log only informs: a malformed entry is dropped, never the decisions with it.
  const rawLog = field(value, "log");
  const log = (Array.isArray(rawLog) ? rawLog : [])
    .map(parseRuleAnswer)
    .filter((entry): entry is RuleAnswer => entry !== null)
    .sort((a, b) => b.id - a.id);
  // The backend sends them oldest first; sorting again costs nothing and keeps the queue honest.
  return { enabled, pending: [...pending].sort((a, b) => a.id - b.id), log };
}

/** Sessions with at least one pending decision. */
export function waitingSessions(snapshot: Pick<GateSnapshot, "enabled" | "pending">): Set<number> {
  return new Set(snapshot.pending.map((decision) => decision.sessionId));
}

/**
 * The decision shown on the card: the active session's oldest, else the oldest of a session shown
 * in a pane, else the oldest overall. `null` while nothing waits.
 */
export function decisionToShow(
  pending: readonly PendingDecision[],
  activeId: number | null,
  shown: ReadonlySet<number> = new Set(),
): PendingDecision | null {
  return (
    pending.find((decision) => decision.sessionId === activeId) ??
    pending.find((decision) => shown.has(decision.sessionId)) ??
    pending[0] ??
    null
  );
}

/**
 * Decisions not reported yet, oldest first. The caller adds their ids to `seen`, so each decision
 * plays the chime and sends a notification at most once.
 */
export function freshDecisions(
  seen: ReadonlySet<number>,
  snapshot: Pick<GateSnapshot, "enabled" | "pending">,
): PendingDecision[] {
  return snapshot.pending.filter((decision) => !seen.has(decision.id));
}

export interface DecisionText {
  /** "Claude Code wants to change src/x.ts" */
  title: string;
  /** "Also being changed by Codex · feature: lines 10 to 14" */
  detail: string;
}

/**
 * The card's two lines, also the desktop notification's title and body. Session labels come from
 * `nameOf` (the sidebar title, "Codex · feature" for a worktree session); the path shows invisible
 * characters as placeholders.
 */
export function decisionText(
  decision: PendingDecision,
  nameOf: (id: number) => string,
  t: Translate,
): DecisionText {
  const others = [...decision.otherSessions.map(nameOf), ...decision.teammates].join(", ");
  const tasks = decision.reserved
    .map((entry) =>
      t("gate.reservedTask", {
        title: revealInvisible(entry.title),
        person: revealInvisible(entry.who),
      }),
    )
    .join(", ");
  const lines =
    decision.level === "overlappingLines" && decision.lines.length > 0
      ? formatLines(decision.lines, t)
      : "";
  return {
    title: t("gate.title", {
      session: nameOf(decision.sessionId),
      path: revealInvisible(decision.path),
    }),
    detail: [
      others === ""
        ? ""
        : lines === ""
          ? t("gate.also", { others })
          : t("gate.alsoLines", { others, lines }),
      tasks === "" ? "" : t("gate.reserved", { tasks }),
    ]
      .filter((part) => part !== "")
      .join(" · "),
  };
}

export interface AttentionView {
  attention: ReadonlyMap<number, AttentionState>;
  reasons: ReadonlyMap<number, AttentionReason>;
}

/**
 * The tracker's attention with every session that waits for a decision on top: it shows as
 * waiting with the reason "gate", seen or not, until the decision is answered or gone. Without
 * waiting sessions the tracker's maps come back unchanged.
 */
export function withGate(view: AttentionView, waiting: ReadonlySet<number>): AttentionView {
  if (waiting.size === 0) return view;
  const attention = new Map(view.attention);
  const reasons = new Map(view.reasons);
  for (const id of waiting) {
    attention.set(id, "attention");
    reasons.set(id, "gate");
  }
  return { attention, reasons };
}

/**
 * Why a session's edits are not paused although its program could be: a calm line for the
 * session's tooltip, or `null` when there is nothing to say (gate active, not for this program,
 * gate off).
 */
export function gateHint(status: GateStatus | undefined, t: Translate): string | null {
  switch (status) {
    case "noEndpoint":
      return t("gate.noEndpoint");
    case "launcher":
      return t("gate.launcher");
    case "profileSettings":
      return t("gate.profileSettings");
    case "installPath":
      return t("gate.installPath");
    case "hookFiles":
      return t("gate.hookFiles");
    case "active":
    case "none":
    case undefined:
      return null;
  }
}

/** Upper bound for the wait in `ready`: a backend that never answers must not stop the restore. */
export const GATE_READY_TIMEOUT_MS = 3000;

export interface GateSetup {
  /** Tells the backend the setting; every call after the first just passes it on. */
  configure: (enabled: boolean) => Promise<void>;
  /**
   * Settles once the first `configure` settled (or after `GATE_READY_TIMEOUT_MS`). Restored
   * sessions wait for it: started earlier, a Claude Code session would miss the gate for its
   * whole run, because the backend only adds the hooks while the gate is on.
   */
  ready: Promise<void>;
}

export function gateSetup(
  send: (enabled: boolean) => Promise<unknown>,
  wait: (ms: number) => Promise<void> = (ms) =>
    new Promise((resolve) => {
      setTimeout(resolve, ms);
    }),
): GateSetup {
  let markReady: () => void = () => undefined;
  const configured = new Promise<void>((resolve) => {
    markReady = resolve;
  });
  const configure = async (enabled: boolean) => {
    try {
      await send(enabled);
    } finally {
      markReady();
    }
  };
  return { configure, ready: Promise.race([configured, wait(GATE_READY_TIMEOUT_MS)]) };
}
