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

/** Since when the session's oldest pending gate decision waits (Unix ms); `null`: none. */
export function gateWaitingSince(
  pending: readonly { sessionId: number; createdAt: number }[],
  sessionId: number,
): number | null {
  let since: number | null = null;
  for (const decision of pending) {
    if (decision.sessionId === sessionId && (since === null || decision.createdAt < since)) {
      since = decision.createdAt;
    }
  }
  return since;
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
 * one after its linger, a quiet one that is history). `awaited`: this is the sub-agent the
 * session's pending decision most likely belongs to.
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

/**
 * The sub-agent a decision that has waited since `waitingSince` (Unix ms) most likely belongs to:
 * among the unfinished ones that wrote nothing since then, the one that wrote last. One that has
 * written since is not waiting; `null` when none fits. The sub-agent's own id is not known to the
 * gate (see the wiki), so this stays a guess.
 */
export function awaitedAgent(agents: readonly SubAgent[], waitingSince: number): string | null {
  let best: SubAgent | null = null;
  for (const agent of agents) {
    if (agent.status === "finished" || agent.lastActivity === null) continue;
    if (agent.lastActivity >= waitingSince) continue;
    if (best === null || agent.lastActivity >= (best.lastActivity ?? 0)) best = agent;
  }
  return best?.id ?? null;
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
 */
export function crewOf(
  agents: readonly SubAgent[],
  now: number,
  waitingSince: number | null,
  max: number = MAX_CREW,
): Crew {
  const awaited = waitingSince === null ? null : awaitedAgent(agents, waitingSince);
  const all: { entry: CrewEntry; index: number }[] = [];
  agents.forEach((agent, index) => {
    const state = crewState(agent, now, agent.id === awaited);
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
