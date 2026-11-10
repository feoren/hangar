// Sub-agent view (contract: docs/wiki/pages/syntheses/agent-workflow-features.md, item 23): the
// backend streams full snapshots of a session's sub-agents; this module checks them, keeps a short
// history of tool names, plans which pane shows what and formats times. Pure, so it is tested
// without React. Nothing here is ever persisted: excerpts live only in the view's memory.

import type { MessageKey, Translate } from "./i18n";
import type { LayoutId } from "./snapLayouts";

export type SubAgentStatus = "running" | "finished" | "stale";

/** One sub-agent as the backend sends it (Rust: `SubAgent` in src-tauri/src/subagents/mod.rs). */
export interface SubAgent {
  id: string;
  description: string | null;
  agentType: string | null;
  spawnDepth: number | null;
  status: SubAgentStatus;
  /** Unix milliseconds. */
  startedAt: number | null;
  lastActivity: number | null;
  /** Newest tool call name, such as `Grep`. */
  tool: string | null;
  /** Newest assistant text (redacted by the backend, at most 400 characters). */
  excerpt: string | null;
}

export interface SubAgentSnapshot {
  /** `false` until the agent CLI wrote the session's transcript (after the first prompt). */
  located: boolean;
  /** Oldest first. */
  agents: SubAgent[];
}

/** Backend limit; anything beyond it is dropped rather than rendered. */
const MAX_AGENTS = 256;
/** Backend limit for an excerpt; a longer one is cut, never trusted to be short. */
const MAX_EXCERPT = 400;
/** Description, type and tool name are short labels; longer ones are cut for the layout. */
const MAX_LABEL = 200;
/** Tool names kept per sub-agent before the current one. */
export const TOOL_HISTORY = 4;

function isStatus(value: unknown): value is SubAgentStatus {
  return value === "running" || value === "finished" || value === "stale";
}

function text(value: unknown, max: number): string | null {
  if (typeof value !== "string") return null;
  const trimmed = value.trim();
  if (trimmed === "") return null;
  return Array.from(trimmed).slice(0, max).join("");
}

function time(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : null;
}

function subAgent(value: unknown): SubAgent | null {
  if (typeof value !== "object" || value === null) return null;
  const field = (name: string): unknown => (name in value ? Reflect.get(value, name) : undefined);
  const id = field("id");
  const status = field("status");
  if (typeof id !== "string" || id === "" || !isStatus(status)) return null;
  const depth = field("spawnDepth");
  return {
    id,
    description: text(field("description"), MAX_LABEL),
    agentType: text(field("agentType"), MAX_LABEL),
    spawnDepth: typeof depth === "number" && Number.isInteger(depth) && depth >= 0 ? depth : null,
    status,
    startedAt: time(field("startedAt")),
    lastActivity: time(field("lastActivity")),
    tool: text(field("tool"), MAX_LABEL),
    excerpt: text(field("excerpt"), MAX_EXCERPT),
  };
}

/**
 * Checks one channel message. Only `snapshot` is known; anything else yields `null` and is
 * ignored. Broken entries and duplicate IDs are dropped, the rest stays.
 */
export function parseSubAgentEvent(value: unknown): SubAgentSnapshot | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const type: unknown = "type" in value ? value.type : undefined;
  const located: unknown = "located" in value ? value.located : undefined;
  const list: unknown = "agents" in value ? value.agents : undefined;
  if (type !== "snapshot" || typeof located !== "boolean" || !Array.isArray(list)) return null;
  const seen = new Set<string>();
  const agents: SubAgent[] = [];
  for (const item of list) {
    if (agents.length >= MAX_AGENTS) break;
    const agent = subAgent(item);
    if (agent === null || seen.has(agent.id)) continue;
    seen.add(agent.id);
    agents.push(agent);
  }
  return { located, agents };
}

/** What the view shows, in memory only. */
export interface SubAgentView {
  /** `null` until the first snapshot arrived. */
  located: boolean | null;
  agents: readonly SubAgent[];
  /** Earlier tool names per sub-agent, newest first, without the current one. */
  history: ReadonlyMap<string, readonly string[]>;
  /** Most sub-agents seen at once: the layout only grows while the view is open. */
  peak: number;
}

export const EMPTY_VIEW: SubAgentView = { located: null, agents: [], history: new Map(), peak: 0 };

/**
 * The same view without assistant text, for when excerpts are no longer wanted (view closed,
 * excerpts switched off): nothing of them stays in memory until the next snapshot arrives.
 */
export function withoutExcerpts(view: SubAgentView): SubAgentView {
  if (view.agents.every((agent) => agent.excerpt === null)) return view;
  return { ...view, agents: view.agents.map((agent) => ({ ...agent, excerpt: null })) };
}

/** Rows of the sidebar's sub-agent tree under the active session. */
export const TREE_ROWS = 5;

export interface SubAgentTree {
  /** In start order. */
  agents: SubAgent[];
  /** Sub-agents left out; a "+N more" row opens the view with all of them. */
  more: number;
}

/**
 * The tree stays short: up to `max` sub-agents fit; with more, running and newer ones keep a
 * row and the last row says how many more there are.
 */
export function subAgentTree(agents: readonly SubAgent[], max: number = TREE_ROWS): SubAgentTree {
  if (agents.length <= max) return { agents: [...agents], more: 0 };
  const shown = featuredAgents(agents, Math.max(0, max - 1));
  return { agents: shown, more: agents.length - shown.length };
}

/**
 * Session whose sub-agents are watched: the open view's, else the active session's while the
 * sidebar shows its tree. One at most, since the backend keeps one watch per session and every
 * watch polls the agent's transcript.
 */
export function watchedSession(
  viewOf: number | null,
  activeId: number | null,
  treeShown: boolean,
): number | null {
  return viewOf ?? (treeShown ? activeId : null);
}

/** Most sub-agents seen since the view opened on `key` (session and conversation). */
export interface ViewPeak {
  key: string | null;
  count: number;
}

/**
 * Peak for the view's layout: grows with the count while the view stays on the same session and
 * conversation, starts over when it opens, closes (`key` null) or the conversation changes. The
 * same object comes back when nothing changed, so a caller can skip the state update.
 */
export function nextViewPeak(peak: ViewPeak, key: string | null, count: number): ViewPeak {
  if (key === null) return peak.key === null && peak.count === 0 ? peak : { key: null, count: 0 };
  const base = peak.key === key ? peak.count : 0;
  const next = Math.max(base, count);
  return peak.key === key && next === peak.count ? peak : { key, count: next };
}

/**
 * Next view after a snapshot: the snapshot replaces the agents; a sub-agent whose tool changed
 * puts the previous tool name in front of its history.
 */
export function applySnapshot(view: SubAgentView, snapshot: SubAgentSnapshot): SubAgentView {
  const before = new Map(view.agents.map((agent) => [agent.id, agent]));
  const history = new Map<string, readonly string[]>();
  for (const agent of snapshot.agents) {
    const kept = view.history.get(agent.id) ?? [];
    const previous = before.get(agent.id)?.tool ?? null;
    history.set(
      agent.id,
      previous !== null && previous !== agent.tool
        ? [previous, ...kept].slice(0, TOOL_HISTORY)
        : kept,
    );
  }
  return {
    located: snapshot.located,
    agents: snapshot.agents,
    history,
    peak: Math.max(view.peak, snapshot.agents.length),
  };
}

/** Content of one zone besides the session's own (zone 0). */
export type SubAgentPane =
  | { kind: "agent"; id: string }
  /** Every sub-agent in a compact list (more sub-agents than zones). */
  | { kind: "list" }
  /** Not located yet, none yet, unavailable or an error: a short text. */
  | { kind: "notice" }
  | { kind: "empty" };

export interface SubAgentPlan {
  layout: Exclude<LayoutId, "single">;
  /** Panes for zones 1 and up, in zone order. */
  panes: SubAgentPane[];
}

/** Zones for sub-agents in the largest layout (quad has four, the session keeps one). */
const MAX_AGENT_ZONES = 3;

/**
 * Sub-agents worth a place of their own when there are too many for one each: the `preferred`
 * one (picked in the sidebar) first, then running ones before the rest, newer before older; shown
 * in start order so they keep their places.
 */
export function featuredAgents(
  agents: readonly SubAgent[],
  count: number,
  preferred: string | null = null,
): SubAgent[] {
  const ranked = agents
    .map((agent, index) => ({ agent, index }))
    .sort((a, b) => {
      const picked = Number(b.agent.id === preferred) - Number(a.agent.id === preferred);
      if (picked !== 0) return picked;
      const live = Number(b.agent.status === "running") - Number(a.agent.status === "running");
      return live !== 0 ? live : b.index - a.index;
    })
    .slice(0, count);
  return ranked.sort((a, b) => a.index - b.index).map((entry) => entry.agent);
}

/**
 * Only the layout of `planSubAgentPanes`, from counts alone: the app places the session's
 * terminal with it and so re-renders only when the count changes, not with every snapshot.
 */
export function subAgentLayout(
  count: number,
  peak: number,
  showNotice: boolean,
): SubAgentPlan["layout"] {
  if (showNotice || count === 0) return "pair";
  const zones = Math.max(peak, count);
  return zones === 1 ? "pair" : zones === 2 ? "mainStack" : "quad";
}

/**
 * Layout of the view. The session stays in the main zone (zone 0); one sub-agent sits beside
 * it, two stack on the right, three fill a grid. With four or more, the grid shows two featured
 * sub-agents and a list of all of them. The layout follows `peak`, so it never shrinks while
 * the view is open and the session's terminal is resized as rarely as possible. A `preferred`
 * sub-agent (picked in the sidebar) always gets a pane of its own.
 */
export function planSubAgentPanes(
  view: SubAgentView,
  showNotice: boolean,
  preferred: string | null = null,
): SubAgentPlan {
  const agents = view.agents;
  const layout = subAgentLayout(agents.length, view.peak, showNotice);
  if (showNotice || agents.length === 0) return { layout, panes: [{ kind: "notice" }] };
  const count = Math.max(view.peak, agents.length);
  if (count > MAX_AGENT_ZONES) {
    const featured = featuredAgents(agents, MAX_AGENT_ZONES - 1, preferred);
    const panes: SubAgentPane[] = featured.map((agent) => ({ kind: "agent", id: agent.id }));
    while (panes.length < MAX_AGENT_ZONES - 1) panes.push({ kind: "empty" });
    return { layout, panes: [...panes, { kind: "list" }] };
  }
  const panes: SubAgentPane[] = [];
  for (let index = 0; index < count; index += 1) {
    const agent = agents[index];
    panes.push(agent === undefined ? { kind: "empty" } : { kind: "agent", id: agent.id });
  }
  return { layout, panes };
}

/** Run time so far (running, stale) or in total (finished); `null` without a start time. */
export function elapsedMs(agent: SubAgent, now: number): number | null {
  if (agent.startedAt === null) return null;
  const end = agent.status === "finished" ? (agent.lastActivity ?? now) : now;
  return Math.max(0, end - agent.startedAt);
}

/** "0:42", "12:05", "1:02:03": the same in every language. */
export function formatElapsed(ms: number): string {
  const total = Number.isFinite(ms) && ms > 0 ? Math.floor(ms / 1000) : 0;
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = String(total % 60).padStart(2, "0");
  return hours > 0
    ? `${String(hours)}:${String(minutes).padStart(2, "0")}:${seconds}`
    : `${String(minutes)}:${seconds}`;
}

export const STATUS_LABELS: Record<SubAgentStatus, MessageKey> = {
  running: "subAgents.running",
  finished: "subAgents.finished",
  stale: "subAgents.stale",
};

/** Name of a sub-agent: its description, else its type, else a generic name. */
export function agentTitle(agent: SubAgent, t: Translate): string {
  return agent.description ?? agent.agentType ?? t("subAgents.untitled");
}

/** Counts per status for the list's summary. */
export function statusCounts(agents: readonly SubAgent[]): Record<SubAgentStatus, number> {
  const counts: Record<SubAgentStatus, number> = { running: 0, finished: 0, stale: 0 };
  for (const agent of agents) counts[agent.status] += 1;
  return counts;
}

/**
 * Summary of the list: running and finished always, quiet ones only when there are any, so the
 * numbers shown add up to the total.
 */
export function countsText(agents: readonly SubAgent[], t: Translate): string {
  const counts = statusCounts(agents);
  const base = t("subAgents.counts", { running: counts.running, finished: counts.finished });
  return counts.stale > 0
    ? `${base} · ${t("subAgents.countStale", { stale: counts.stale })}`
    : base;
}
