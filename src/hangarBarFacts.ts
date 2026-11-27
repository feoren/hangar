// Hangar's bar above an agent session's terminal (HangarBar.tsx): which sessions get it, which
// facts it shows and in which order they give way when the pane gets narrow. Pure, so it is
// tested without React. Programs that draw Hangar's status line themselves (`SessionInfo`
// `statusLine`) never get the bar: the same facts would show twice.

import type { ClaimsSnapshot } from "./claims";
import type { SessionContext, OutputRate } from "./contextUsage";

/** One fact of the bar. */
export type BarSegment =
  "model" | "folder" | "worktree" | "branch" | "changes" | "context" | "speed" | "cost" | "runtime";

/** "Where": the left group, in this order. */
export const WHERE_SEGMENTS: readonly BarSegment[] = [
  "model",
  "folder",
  "worktree",
  "branch",
  "changes",
];

/** "How": the right group, in this order. */
export const HOW_SEGMENTS: readonly BarSegment[] = ["context", "speed", "cost", "runtime"];

/**
 * The order in which the facts give way as the pane gets narrower (styles.css "Hangar bar"):
 * the first goes first, the context stays to the end.
 */
export const DROP_ORDER: readonly BarSegment[] = [
  "speed",
  "runtime",
  "worktree",
  "changes",
  "cost",
  "branch",
  "folder",
  "model",
  "context",
];

/** 1 for the fact that gives way first; the context, the last one, never does (0). */
export function dropRank(segment: BarSegment): number {
  const rank = DROP_ORDER.indexOf(segment) + 1;
  return rank === DROP_ORDER.length ? 0 : rank;
}

/**
 * Whether a session's pane shows the bar: only while the setting is on, only for agents (no
 * shells, no update runs) and only when the program does not draw Hangar's status line itself.
 */
export function showsHangarBar(options: {
  enabled: boolean;
  agent: boolean;
  statusLine: boolean | undefined;
}): boolean {
  return options.enabled && options.agent && options.statusLine !== true;
}

/** What the bar knows about a session; `null` or `undefined`: not known, the fact is left out. */
export interface BarFacts {
  model: { text: string; title: string } | null;
  folder: string | null;
  /** The session runs in its own git worktree. */
  worktree: boolean;
  branch: string | null;
  /** Files the session's agent changed so far ("12", "200+"); `null`: none or not known. */
  changes: string | null;
  context: SessionContext | undefined;
  speed: OutputRate | undefined;
  cost: string | null;
  runtime: string | null;
}

function present(segment: BarSegment, facts: BarFacts): boolean {
  switch (segment) {
    case "model":
      return facts.model !== null;
    case "folder":
      return facts.folder !== null && facts.folder !== "";
    case "worktree":
      return facts.worktree;
    case "branch":
      return facts.branch !== null && facts.branch !== "";
    case "changes":
      return facts.changes !== null;
    case "context":
      return facts.context !== undefined;
    case "speed":
      return facts.speed !== undefined;
    case "cost":
      return facts.cost !== null;
    case "runtime":
      return facts.runtime !== null;
  }
}

/** The facts the bar shows, per group and in order; facts without data are left out. */
export function barSegments(facts: BarFacts): { where: BarSegment[]; how: BarSegment[] } {
  return {
    where: WHERE_SEGMENTS.filter((segment) => present(segment, facts)),
    how: HOW_SEGMENTS.filter((segment) => present(segment, facts)),
  };
}

/**
 * How many files a session's agent changed, from the local claims (claims.ts): "12", "200+" when
 * the backend cut the list. `null` while the claims are off, not ready, outside a repository or
 * without a change.
 */
export function changedFiles(claims: ClaimsSnapshot, sessionId: number): string | null {
  if (!claims.enabled) return null;
  const own = claims.sessions.find((entry) => entry.sessionId === sessionId);
  if (own === undefined || own.state.kind !== "ready" || own.claims.length === 0) return null;
  const count = String(own.claims.length);
  return own.truncated ? `${count}+` : count;
}
