// The speech bubble of a crew figure: a short sentence from the newest tool call and its target
// (docs/wiki/pages/syntheses/crew-column.md). Pure and bounded; the backend already redacted the
// target, here it is only made short. Nothing is stored.

import type { MessageKey, Translate } from "./i18n";
import type { CrewState } from "./crew";
import type { SubAgent } from "./subAgents";

/** Longest command, pattern or URL in a bubble. */
export const MAX_TARGET = 28;
/** Longest tool name in a bubble. */
export const MAX_TOOL = 24;

type Kind = "read" | "write" | "edit" | "run" | "search" | "fetch" | "webSearch" | "delegate";

const KINDS: ReadonlyMap<string, Kind> = new Map([
  ["Read", "read"],
  ["NotebookRead", "read"],
  ["Write", "write"],
  ["Edit", "edit"],
  ["MultiEdit", "edit"],
  ["NotebookEdit", "edit"],
  ["Bash", "run"],
  ["PowerShell", "run"],
  ["Grep", "search"],
  ["Glob", "search"],
  ["WebFetch", "fetch"],
  ["WebSearch", "webSearch"],
  ["Task", "delegate"],
  ["Agent", "delegate"],
]);

const WITH_TARGET: Record<Kind, MessageKey> = {
  read: "subAgents.act.read",
  write: "subAgents.act.write",
  edit: "subAgents.act.edit",
  run: "subAgents.act.run",
  search: "subAgents.act.search",
  fetch: "subAgents.act.fetch",
  webSearch: "subAgents.act.webSearch",
  delegate: "subAgents.act.delegate",
};

const WITHOUT_TARGET: Record<Kind, MessageKey> = {
  read: "crew.say.read",
  write: "crew.say.write",
  edit: "crew.say.edit",
  run: "crew.say.run",
  search: "crew.say.search",
  fetch: "crew.say.fetch",
  webSearch: "crew.say.webSearch",
  delegate: "crew.say.delegate",
};

/** `text` cut to `max` characters with an ellipsis. */
export function clip(text: string, max: number): string {
  const chars = Array.from(text);
  return chars.length <= max
    ? text
    : `${chars
        .slice(0, max - 1)
        .join("")
        .trimEnd()}…`;
}

/** The file name of a path with either separator; other targets are returned unchanged. */
export function baseName(path: string): string {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("/"), trimmed.lastIndexOf("\\"));
  return cut < 0 ? trimmed : trimmed.slice(cut + 1);
}

/** The target as the bubble shows it: a file name for file tools, else the first line, cut. */
export function shortTarget(kind: Kind, target: string): string {
  const firstLine = target.split(/\r?\n/, 1)[0]?.trim() ?? "";
  const text =
    kind === "read" || kind === "write" || kind === "edit" ? baseName(firstLine) : firstLine;
  return clip(text.replace(/\s+/g, " "), MAX_TARGET);
}

/** A tool name for the bubble: MCP names (`mcp__server__tool`) to their last part, cut. */
export function shortTool(tool: string): string {
  const last =
    tool
      .split("__")
      .filter((part) => part !== "")
      .at(-1) ?? tool;
  return clip(last, MAX_TOOL);
}

/**
 * Whether the bubble shows typing dots instead of words: a working figure whose shown text is
 * still the start (no tool yet). Read from the shown text, not the agent, so the dots and a held
 * back text (BubbleThrottle) never disagree.
 */
export function bubbleTyping(state: CrewState, shown: string, t: Translate): boolean {
  return state === "working" && shown === t("subAgents.starting");
}

/** What the figure says in `state`; `agent` supplies the newest tool call. */
export function bubbleText(agent: SubAgent, state: CrewState, t: Translate): string {
  if (state === "done") return t("subAgents.done");
  if (state === "waiting") return t("crew.say.waiting");
  if (state === "quiet") return t("crew.say.quiet");
  if (agent.tool === null) return t("subAgents.starting");
  const kind = KINDS.get(agent.tool);
  if (kind !== undefined) {
    const target = agent.target === null ? "" : shortTarget(kind, agent.target);
    return target === "" ? t(WITHOUT_TARGET[kind]) : t(WITH_TARGET[kind], { target });
  }
  return t("crew.say.other", { tool: shortTool(agent.tool) });
}
