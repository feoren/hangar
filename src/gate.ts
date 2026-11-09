// The gate (team service step B): before Claude Code edits a file that another of the user's
// running sessions is changing, the backend holds the agent's hook request open and the person
// decides in Hangar. This module checks the backend's snapshot of pending decisions, picks the one
// to show, builds the card's texts and tells which decisions are new (chime, notification).

import type { GateStatus } from "./api";
import type { AttentionReason, AttentionState } from "./attention";
import { field, formatLines, isCount, list, parseRange, type LineRange } from "./claims";
import type { Translate } from "./i18n";
import { revealInvisible } from "./invisible";

export type GateLevel = "sameFile" | "overlappingLines";

export type GateAnswer = "allowOnce" | "allowFile" | "deny";

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
  /** Sessions changing the same file, at least one. */
  otherSessions: number[];
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
}

export const EMPTY_GATE: GateSnapshot = { enabled: false, pending: [] };

const LEVELS: readonly GateLevel[] = ["sameFile", "overlappingLines"];

function parseDecision(value: unknown): PendingDecision | null {
  const id = field(value, "id");
  const sessionId = field(value, "sessionId");
  const tool = field(value, "tool");
  const path = field(value, "path");
  const otherSessions = list(field(value, "otherSessions"), (item) =>
    isCount(item) ? item : null,
  );
  const level = LEVELS.find((entry) => entry === field(value, "level"));
  const lines = list(field(value, "lines"), parseRange);
  const createdAt = field(value, "createdAt");
  if (!isCount(id) || !isCount(sessionId) || typeof tool !== "string") return null;
  if (typeof path !== "string" || path === "" || level === undefined || lines === null) {
    return null;
  }
  if (otherSessions === null || otherSessions.length === 0) return null;
  if (typeof createdAt !== "number" || !Number.isFinite(createdAt) || createdAt < 0) return null;
  return { id, sessionId, tool, path, otherSessions, level, lines, createdAt };
}

/** Checks the backend's snapshot; `null` if anything is malformed or an id repeats. */
export function parseGateSnapshot(value: unknown): GateSnapshot | null {
  const enabled = field(value, "enabled");
  const pending = list(field(value, "pending"), parseDecision);
  if (typeof enabled !== "boolean" || pending === null) return null;
  const ids = new Set(pending.map((decision) => decision.id));
  if (ids.size !== pending.length) return null;
  // The backend sends them oldest first; sorting again costs nothing and keeps the queue honest.
  return { enabled, pending: [...pending].sort((a, b) => a.id - b.id) };
}

/** Sessions with at least one pending decision. */
export function waitingSessions(snapshot: GateSnapshot): Set<number> {
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
  snapshot: GateSnapshot,
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
  const others = decision.otherSessions.map(nameOf).join(", ");
  const lines =
    decision.level === "overlappingLines" && decision.lines.length > 0
      ? formatLines(decision.lines, t)
      : "";
  return {
    title: t("gate.title", {
      session: nameOf(decision.sessionId),
      path: revealInvisible(decision.path),
    }),
    detail: lines === "" ? t("gate.also", { others }) : t("gate.alsoLines", { others, lines }),
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
