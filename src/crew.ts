// The crew column (docs/wiki/pages/syntheses/crew-column.md): which sub-agents of the session stand
// beside its terminal, in which state, when the column shows, which form it takes in a narrow pane
// and how often a figure's speech bubble may change. Pure, so it is tested without React.

import type { AttentionReason } from "./attention";
import type { SubAgent } from "./subAgents";

export type CrewState = "working" | "waiting" | "done" | "quiet";

/** A finished sub-agent stays this long, then leaves the column. */
export const DONE_LINGER_MS = 8000;
/** A quiet sub-agent older than this is history, not crew. */
export const QUIET_SHOWN_MS = 10 * 60 * 1000;
/** A bubble changes at most this often per figure. */
export const BUBBLE_MIN_MS = 1000;
/** Figures the column holds; the rest is a "+N more" line. */
export const MAX_CREW = 6;
/** Panes narrower than this get the strip above the terminal instead of the column. */
export const CREW_COLUMN_MIN_PANE = 560;

export type CrewForm = "column" | "strip";

/** The column in a wide pane, the strip in a narrow one; an unmeasured pane counts as wide. */
export function crewForm(paneWidth: number | null): CrewForm {
  return paneWidth !== null && paneWidth < CREW_COLUMN_MIN_PANE ? "strip" : "column";
}

/**
 * Whether the session waits for a decision of the user: only a pending gate decision counts. A
 * bell does not: Claude Code also rings it for the main agent's idle notice.
 */
export function waitsForDecision(
  attention: string | undefined,
  reason: AttentionReason | null | undefined,
): boolean {
  return attention === "attention" && reason === "gate";
}

/**
 * The sub-agents the session's pending gate decisions name (Claude Code's `agent_id`). A decision
 * that names nobody is the main agent's: the column exists only for Claude Code, which names a
 * sub-agent on every tool call since 2.1.69, so nobody is guessed.
 */
export function waitingSubAgents(
  pending: readonly { sessionId: number; agents: readonly { id: string }[] }[],
  sessionId: number,
): ReadonlySet<string> {
  const named = new Set<string>();
  for (const decision of pending) {
    if (decision.sessionId !== sessionId) continue;
    for (const agent of decision.agents) named.add(agent.id);
  }
  return named;
}

/** Sub-agents are watched for the column while the session supports them and the switch is on. */
export function crewWatchWanted(supportsSubAgents: boolean, enabled: boolean): boolean {
  return supportsSubAgents && enabled;
}

export interface CrewEntry {
  agent: SubAgent;
  state: CrewState;
}

/**
 * The state of one sub-agent, `null` when it does not belong in the column any more (a finished
 * one after its linger, a quiet one that is history). `awaited`: a pending gate decision of the
 * session names this sub-agent.
 */
export function crewState(agent: SubAgent, now: number, awaited: boolean): CrewState | null {
  const age = agent.lastActivity === null ? 0 : Math.max(0, now - agent.lastActivity);
  switch (agent.status) {
    case "finished":
      return age > DONE_LINGER_MS ? null : "done";
    case "stale":
      return awaited ? "waiting" : age > QUIET_SHOWN_MS ? null : "quiet";
    case "running":
      return awaited ? "waiting" : "working";
  }
}

const RANK: Record<CrewState, number> = { waiting: 0, working: 1, quiet: 2, done: 3 };

export interface Crew {
  /** In start order. */
  entries: CrewEntry[];
  /** Sub-agents left out for lack of room. */
  more: number;
}

/**
 * The crew of a session: every sub-agent that still belongs in the column, at most `max` of them
 * (waiting first, then working, quiet, done, newer before older), shown in start order.
 * `waiting`: the sub-agents a pending gate decision names ({@link waitingSubAgents}).
 */
export function crewOf(
  agents: readonly SubAgent[],
  now: number,
  waiting: ReadonlySet<string>,
  max: number = MAX_CREW,
): Crew {
  const all: { entry: CrewEntry; index: number }[] = [];
  agents.forEach((agent, index) => {
    const state = crewState(agent, now, waiting.has(agent.id));
    if (state !== null) all.push({ entry: { agent, state }, index });
  });
  if (all.length <= max) return { entries: all.map((e) => e.entry), more: 0 };
  const kept = [...all]
    .sort((a, b) => RANK[a.entry.state] - RANK[b.entry.state] || b.index - a.index)
    .slice(0, max)
    .sort((a, b) => a.index - b.index);
  return { entries: kept.map((e) => e.entry), more: all.length - kept.length };
}

/** The column shows when the watch is wanted and somebody stands in it. */
export function crewShown(watchWanted: boolean, crew: Crew): boolean {
  return watchWanted && crew.entries.length > 0;
}

/** What a figure wants to say; `urgent` texts (a change of state) skip the throttle. */
export interface WantedBubble {
  text: string;
  state: CrewState;
}

interface Slot {
  text: string;
  state: CrewState;
  since: number;
}

/**
 * Keeps bubbles calm: a text changes at most once per `minMs` per figure, the newest wanted text
 * wins once the time is over. A change of state shows at once. Holds only the shown texts.
 */
export class BubbleThrottle {
  private slots = new Map<string, Slot>();
  private readonly minMs: number;

  constructor(minMs: number = BUBBLE_MIN_MS) {
    this.minMs = minMs;
  }

  /** The texts to show for `wanted` at `now`; figures that left are forgotten. Idempotent per `now`. */
  update(wanted: ReadonlyMap<string, WantedBubble>, now: number): ReadonlyMap<string, string> {
    const next = new Map<string, Slot>();
    const shown = new Map<string, string>();
    for (const [id, want] of wanted) {
      const slot = this.slots.get(id);
      const change =
        slot !== undefined &&
        slot.text !== want.text &&
        (slot.state !== want.state || now - slot.since >= this.minMs);
      const kept: Slot =
        slot === undefined || change ? { text: want.text, state: want.state, since: now } : slot;
      // The state follows the figure even while an old text is held back.
      next.set(id, kept.state === want.state ? kept : { ...kept, state: want.state });
      shown.set(id, kept.text);
    }
    this.slots = next;
    return shown;
  }
}
