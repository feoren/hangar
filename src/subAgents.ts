// Sub-agent view (contract: docs/wiki/pages/syntheses/agent-workflow-features.md, item 23): the
// backend streams full snapshots of a session's sub-agents and the activity log of the one shown;
// this module checks both, keeps the log bounded, decides which sub-agents the sidebar's tree
// lists (finished ones can be removed from it) and formats times. Pure, so it is tested without
// React. Nothing here is ever persisted: texts live only in the view's memory.

import type { MessageKey, Translate } from "./i18n";

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
  /** What that call works on: a file name, a pattern, a command line (only with excerpts). */
  target: string | null;
  /** The call's own one-line description, shown instead of a verb (only with excerpts). */
  note: string | null;
  /** Context size after the newest model response, as Claude Code counts "tokens". */
  tokens: number | null;
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
/** Backend limit for an activity's target and note. */
const MAX_ACTIVITY = 120;

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

/** Reads one field of a checked object without a cast. */
function fieldOf(value: object, name: string): unknown {
  return name in value ? Reflect.get(value, name) : undefined;
}

function subAgent(value: unknown): SubAgent | null {
  if (typeof value !== "object" || value === null) return null;
  const field = (name: string): unknown => fieldOf(value, name);
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
    target: text(field("target"), MAX_ACTIVITY),
    note: text(field("note"), MAX_ACTIVITY),
    tokens: count(field("tokens")),
    excerpt: text(field("excerpt"), MAX_EXCERPT),
  };
}

/** A non-negative whole number, else `null`. */
function count(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

/**
 * Checks one channel message. Only `snapshot` is read here (logs: `parseSubAgentLog`); anything
 * else yields `null` and is ignored. Broken entries and duplicate IDs are dropped, the rest stays.
 */
export function parseSubAgentEvent(value: unknown): SubAgentSnapshot | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const type = fieldOf(value, "type");
  const located = fieldOf(value, "located");
  const list = fieldOf(value, "agents");
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
}

export const EMPTY_VIEW: SubAgentView = { located: null, agents: [] };

/**
 * The same view without assistant text, for when excerpts are no longer wanted (view closed,
 * excerpts switched off): nothing of them stays in memory until the next snapshot arrives.
 */
export function withoutExcerpts(view: SubAgentView): SubAgentView {
  const bare = (agent: SubAgent) =>
    agent.excerpt === null && agent.target === null && agent.note === null;
  if (view.agents.every(bare)) return view;
  return {
    ...view,
    agents: view.agents.map((agent) => ({ ...agent, excerpt: null, target: null, note: null })),
  };
}

/** The view after a snapshot: a snapshot is complete and replaces the previous one. */
export function viewOfSnapshot(snapshot: SubAgentSnapshot): SubAgentView {
  return { located: snapshot.located, agents: snapshot.agents };
}

// Activity log of the one sub-agent the view shows (Rust: `LogEntry` and `LogBatch`).

/** One line of the log; every text is redacted and capped by the backend. */
export type LogEntry =
  /** The task the sub-agent was given. */
  | { kind: "prompt"; text: string }
  | { kind: "text"; text: string }
  /** A tool call and its key argument (file path, command line, pattern). */
  | { kind: "tool"; name: string; detail: string | null }
  /**
   * A tool result: a summary when the tool has one ("Read 120 lines"), else its first lines and
   * how many lines follow.
   */
  | {
      kind: "result";
      lines: string[];
      moreLines: number;
      error: boolean;
      summary: ResultSummary | null;
    }
  /** Lines were left out here (long transcript, burst, or the view's own limit). */
  | { kind: "gap" };

/** What Claude Code shows below a tool call (Rust: `ResultSummary`). */
export type ResultSummary =
  | { kind: "read"; lines: number }
  | { kind: "wrote"; lines: number }
  | { kind: "edited"; added: number; removed: number }
  | { kind: "files"; count: number }
  | { kind: "lines"; count: number }
  | { kind: "matches"; count: number };

export interface LogBatch {
  agentId: string;
  /** Replaces everything shown before. */
  reset: boolean;
  entries: LogEntry[];
  /** Tool calls of the sub-agent so far, also those no longer in the log. */
  toolUses: number;
}

/** Backend limits per entry; longer texts are cut, never trusted to be short. */
const MAX_LOG_TEXT = 4000;
const MAX_LOG_DETAIL = 200;
const MAX_TOOL_NAME = 64;
/** Backend limit of result lines per entry. */
const MAX_RESULT_LINES = 3;
/** Lines the view keeps; older ones give way to a gap line (the task stays). */
export const MAX_LOG_LINES = 1000;
/** Backend limit per batch: a reset holds up to as many lines as the view keeps. */
const MAX_LOG_BATCH = MAX_LOG_LINES;

function resultSummary(value: unknown): ResultSummary | null {
  if (typeof value !== "object" || value === null) return null;
  const field = (name: string): unknown => fieldOf(value, name);
  switch (field("kind")) {
    case "read":
    case "wrote": {
      const lines = count(field("lines"));
      if (lines === null) return null;
      return field("kind") === "read" ? { kind: "read", lines } : { kind: "wrote", lines };
    }
    case "edited": {
      const added = count(field("added"));
      const removed = count(field("removed"));
      return added === null || removed === null ? null : { kind: "edited", added, removed };
    }
    case "files":
    case "lines":
    case "matches": {
      const n = count(field("count"));
      const kind = field("kind");
      if (n === null) return null;
      return kind === "files"
        ? { kind: "files", count: n }
        : kind === "lines"
          ? { kind: "lines", count: n }
          : { kind: "matches", count: n };
    }
    default:
      return null;
  }
}

function logEntry(value: unknown): LogEntry | null {
  if (typeof value !== "object" || value === null) return null;
  const field = (name: string): unknown => fieldOf(value, name);
  switch (field("kind")) {
    case "prompt":
    case "text": {
      const body = text(field("text"), MAX_LOG_TEXT);
      if (body === null) return null;
      return field("kind") === "prompt"
        ? { kind: "prompt", text: body }
        : { kind: "text", text: body };
    }
    case "tool": {
      const name = text(field("name"), MAX_TOOL_NAME);
      if (name === null) return null;
      return { kind: "tool", name, detail: text(field("detail"), MAX_LOG_DETAIL) };
    }
    case "result": {
      const list = field("lines");
      const lines = Array.isArray(list)
        ? list
            .slice(0, 4 * MAX_RESULT_LINES)
            .map((line) => text(line, MAX_LOG_DETAIL))
            .filter((line) => line !== null)
            .slice(0, MAX_RESULT_LINES)
        : [];
      return {
        kind: "result",
        lines,
        moreLines: count(field("moreLines")) ?? 0,
        error: field("error") === true,
        summary: resultSummary(field("summary")),
      };
    }
    case "gap":
      return { kind: "gap" };
    default:
      return null;
  }
}

/** Checks one channel message of type `log`; anything else yields `null`. */
export function parseSubAgentLog(value: unknown): LogBatch | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const agentId = fieldOf(value, "agentId");
  const reset = fieldOf(value, "reset");
  const list = fieldOf(value, "entries");
  if (
    fieldOf(value, "type") !== "log" ||
    typeof agentId !== "string" ||
    agentId === "" ||
    typeof reset !== "boolean" ||
    !Array.isArray(list)
  ) {
    return null;
  }
  const entries: LogEntry[] = [];
  for (const item of list.slice(0, MAX_LOG_BATCH)) {
    const entry = logEntry(item);
    if (entry !== null) entries.push(entry);
  }
  return { agentId, reset, entries, toolUses: count(fieldOf(value, "toolUses")) ?? 0 };
}

/** A log entry with a key that stays the same while it is shown. */
export interface LogLine {
  id: number;
  entry: LogEntry;
}

export interface SubAgentLog {
  /** The sub-agent this log belongs to; `null` while none is shown. */
  agentId: string | null;
  /** `false` until the first batch for `agentId` arrived. */
  loaded: boolean;
  lines: readonly LogLine[];
  /** Next line id. */
  next: number;
  /** Tool calls of the sub-agent so far, from the newest batch. */
  toolUses: number;
}

export const EMPTY_LOG: SubAgentLog = {
  agentId: null,
  loaded: false,
  lines: [],
  next: 0,
  toolUses: 0,
};

/** An empty log for `agentId` (the view switched to it); the same object if nothing changes. */
export function logFor(log: SubAgentLog, agentId: string | null): SubAgentLog {
  if (log.agentId === agentId) return log;
  return { agentId, loaded: false, lines: [], next: log.next, toolUses: 0 };
}

/**
 * Next log after a batch. A batch for another sub-agent is dropped. A reset starts over. Until
 * the first reset after a switch (`loaded` false) appends are dropped too: they are still in
 * flight from an earlier focus on the same sub-agent, and the backend opens every focus with a
 * reset. The log keeps at most `max` lines: older ones give way to one gap line, but a leading
 * task stays, so the view always says what the sub-agent was asked to do.
 */
export function applyLog(
  log: SubAgentLog,
  batch: LogBatch,
  max: number = MAX_LOG_LINES,
): SubAgentLog {
  if (batch.agentId !== log.agentId || (!log.loaded && !batch.reset)) return log;
  let next = log.next;
  const lines: LogLine[] = batch.reset ? [] : [...log.lines];
  for (const entry of batch.entries) {
    // Two gaps in a row say nothing more than one.
    if (entry.kind === "gap" && lines.at(-1)?.entry.kind === "gap") continue;
    lines.push({ id: next, entry });
    next += 1;
  }
  if (lines.length > max) {
    const first = lines[0];
    const head = first?.entry.kind === "prompt" ? [first] : [];
    let rest = lines.slice(lines.length - (max - head.length - 1));
    if (rest[0]?.entry.kind === "gap") rest = rest.slice(1);
    const gap: LogLine = { id: next, entry: { kind: "gap" } };
    next += 1;
    return {
      agentId: log.agentId,
      loaded: true,
      lines: [...head, gap, ...rest],
      next,
      toolUses: batch.toolUses,
    };
  }
  return { agentId: log.agentId, loaded: true, lines, next, toolUses: batch.toolUses };
}

/** The log without any text, for when excerpts are switched off while it is shown. */
export function withoutLogText(log: SubAgentLog): SubAgentLog {
  if (log.lines.length === 0 && !log.loaded) return log;
  return { ...log, loaded: false, lines: [] };
}

// Removing finished sub-agents from the tree and the list (view state only).

/** Sub-agent ids removed per watch key (session and conversation). */
export type Dismissed = ReadonlyMap<string, ReadonlySet<string>>;

const NONE: ReadonlySet<string> = new Set();

export function dismissedIn(dismissed: Dismissed, key: string | null): ReadonlySet<string> {
  return key === null ? NONE : (dismissed.get(key) ?? NONE);
}

/**
 * Removes the finished ones among `ids` from the view of `key`; running and quiet ones stay,
 * since they may still change. Transcripts are never touched. The same map comes back when
 * nothing changes.
 */
export function dismissFinished(
  dismissed: Dismissed,
  key: string,
  agents: readonly SubAgent[],
  ids: readonly string[],
): Dismissed {
  const finished = new Set(agents.filter((a) => a.status === "finished").map((a) => a.id));
  const before = dismissedIn(dismissed, key);
  const added = ids.filter((id) => finished.has(id) && !before.has(id));
  if (added.length === 0) return dismissed;
  const next = new Map(dismissed);
  next.set(key, new Set([...before, ...added]));
  return next;
}

/** The sub-agents still shown. */
export function visibleAgents(
  agents: readonly SubAgent[],
  hidden: ReadonlySet<string>,
): readonly SubAgent[] {
  return hidden.size === 0 ? agents : agents.filter((agent) => !hidden.has(agent.id));
}

export function finishedIds(agents: readonly SubAgent[]): string[] {
  return agents.filter((agent) => agent.status === "finished").map((agent) => agent.id);
}

/** Rows of the sidebar's sub-agent tree under the active session. */
export const TREE_ROWS = 5;

export interface SubAgentTree {
  /** In start order. */
  agents: SubAgent[];
  /** Sub-agents left out; a "+N more" row opens the list of all of them. */
  more: number;
}

/**
 * The tree stays short: up to `max` sub-agents fit; with more, the `preferred` one (shown in
 * the view), running and newer ones keep a row and the last row says how many more there are.
 */
export function subAgentTree(
  agents: readonly SubAgent[],
  max: number = TREE_ROWS,
  preferred: string | null = null,
): SubAgentTree {
  if (agents.length <= max) return { agents: [...agents], more: 0 };
  const shown = featuredAgents(agents, Math.max(0, max - 1), preferred);
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

/**
 * Sub-agents worth a row when there are too many: the `preferred` one first, then running ones
 * before the rest, newer before older; returned in start order so they keep their places.
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

// What the tree row and the log show, the way Claude Code lists its background agents:
// "frontend  Editing sessionLabel.ts  4:12 · ↓ 119.9k".

/** Name of a sub-agent as Claude Code lists it: its type, else its description. */
export function agentName(agent: SubAgent, t: Translate): string {
  return agent.agentType ?? agent.description ?? t("subAgents.untitled");
}

/** "950", "96.3k", "1.2M": token counts as Claude Code shows them, the same in every language. */
export function formatTokens(tokens: number): string {
  const n = Number.isFinite(tokens) && tokens > 0 ? Math.round(tokens) : 0;
  if (n < 1000) return String(n);
  if (n < 999_950) return `${(n / 1000).toFixed(1)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

/** Verbs for the activity line; other tools show their name and target. */
const ACTIVITY_VERBS: ReadonlyMap<string, MessageKey> = new Map([
  ["Read", "subAgents.act.read"],
  ["NotebookRead", "subAgents.act.read"],
  ["Write", "subAgents.act.write"],
  ["Edit", "subAgents.act.edit"],
  ["MultiEdit", "subAgents.act.edit"],
  ["NotebookEdit", "subAgents.act.edit"],
  ["Bash", "subAgents.act.run"],
  ["PowerShell", "subAgents.act.run"],
  ["Grep", "subAgents.act.search"],
  ["Glob", "subAgents.act.search"],
  ["WebFetch", "subAgents.act.fetch"],
  ["WebSearch", "subAgents.act.webSearch"],
  ["Task", "subAgents.act.delegate"],
  ["Agent", "subAgents.act.delegate"],
]);

/**
 * What the sub-agent does right now, from its newest tool call: the call's own description if
 * it has one ("Run cargo test"), else a verb and its target ("Editing git.rs"), else the tool's
 * name (texts switched off). Finished ones say so.
 */
export function activityText(agent: SubAgent, t: Translate): string {
  if (agent.status === "finished") return t("subAgents.done");
  if (agent.note !== null) return agent.note;
  if (agent.tool === null) return t("subAgents.starting");
  if (agent.target === null) return agent.tool;
  const verb = ACTIVITY_VERBS.get(agent.tool);
  return verb === undefined ? `${agent.tool} ${agent.target}` : t(verb, { target: agent.target });
}

/** The line Claude Code shows below a tool call for a structured result. */
export function summaryText(summary: ResultSummary, t: Translate): string {
  switch (summary.kind) {
    case "read":
      return t("subAgents.sum.read", { count: summary.lines });
    case "wrote":
      return t("subAgents.sum.wrote", { count: summary.lines });
    case "edited":
      return t("subAgents.sum.edited", { added: summary.added, removed: summary.removed });
    case "files":
      return t("subAgents.sum.files", { count: summary.count });
    case "lines":
      return t("subAgents.sum.lines", { count: summary.count });
    case "matches":
      return t("subAgents.sum.matches", { count: summary.count });
  }
}

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
