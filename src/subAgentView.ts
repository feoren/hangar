// How Settings > Sub-agents lists the definitions: Hangar's own and every other one on two sides
// of a switch, as a grid of cards or as a table. Which side and which view a viewer chose last is
// remembered on this computer; without storage, the defaults apply.

import { isHangarAgent, type CodexDef, type LibraryEntry } from "./subAgentLibrary";

/** The two sides of the switch: Hangar's sub-agents and every other definition. */
export type AgentSide = "hangar" | "others";

export const AGENT_SIDES: readonly AgentSide[] = ["hangar", "others"];

/** Cards in a grid or one line per definition. */
export type AgentListView = "grid" | "table";

export const AGENT_LIST_VIEWS: readonly AgentListView[] = ["grid", "table"];

/** One listed definition: a Claude Code name (global and project files together) or an own Codex file. */
export type AgentItem =
  | { kind: "claude"; key: string; name: string; entry: LibraryEntry }
  | { kind: "codex"; key: string; name: string; def: CodexDef };

/** Claude Code's definitions first, in their order, then the own Codex CLI files. */
export function agentItems(
  entries: readonly LibraryEntry[],
  codex: readonly CodexDef[],
): AgentItem[] {
  return [
    ...entries.map((entry): AgentItem => ({
      kind: "claude",
      key: `claude:${entry.name}`,
      name: entry.name,
      entry,
    })),
    ...codex.map((def): AgentItem => ({
      kind: "codex",
      key: `codex:${def.folder ?? ""}/${def.file}`,
      name: def.name,
      def,
    })),
  ];
}

export function sideOf(item: AgentItem): AgentSide {
  return isHangarAgent(item.name) ? "hangar" : "others";
}

/** The items of each side, each in the order given. */
export function splitBySide(items: readonly AgentItem[]): Record<AgentSide, AgentItem[]> {
  const sides: Record<AgentSide, AgentItem[]> = { hangar: [], others: [] };
  for (const item of items) sides[sideOf(item)].push(item);
  return sides;
}

/**
 * The side to show: the one chosen last while it has entries, else Hangar's when it has any,
 * else the others. An empty side is still shown when both are empty or it was chosen and the
 * other one is empty too.
 */
export function pickSide(chosen: AgentSide | null, counts: Record<AgentSide, number>): AgentSide {
  if (chosen !== null && counts[chosen] > 0) return chosen;
  if (counts.hangar > 0) return "hangar";
  if (counts.others > 0) return "others";
  return chosen ?? "hangar";
}

export interface AgentListPrefs {
  /** The side picked by hand; null until the viewer picks one. */
  side: AgentSide | null;
  view: AgentListView;
}

export const DEFAULT_AGENT_LIST_PREFS: AgentListPrefs = { side: null, view: "grid" };

export const AGENT_LIST_STORAGE_KEY = "hangar.subAgentList";

function isSide(value: unknown): value is AgentSide {
  return AGENT_SIDES.some((side) => side === value);
}

function isView(value: unknown): value is AgentListView {
  return AGENT_LIST_VIEWS.some((view) => view === value);
}

/** Reads stored preferences; anything missing or malformed falls back to its default. */
export function parseAgentListPrefs(text: string | null): AgentListPrefs {
  if (text === null) return DEFAULT_AGENT_LIST_PREFS;
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return DEFAULT_AGENT_LIST_PREFS;
  }
  if (typeof value !== "object" || value === null) return DEFAULT_AGENT_LIST_PREFS;
  const side = "side" in value ? value.side : null;
  const view = "view" in value ? value.view : null;
  return {
    side: isSide(side) ? side : DEFAULT_AGENT_LIST_PREFS.side,
    view: isView(view) ? view : DEFAULT_AGENT_LIST_PREFS.view,
  };
}

export function loadAgentListPrefs(): AgentListPrefs {
  try {
    return parseAgentListPrefs(localStorage.getItem(AGENT_LIST_STORAGE_KEY));
  } catch {
    return DEFAULT_AGENT_LIST_PREFS;
  }
}

export function saveAgentListPrefs(prefs: AgentListPrefs): void {
  try {
    localStorage.setItem(AGENT_LIST_STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // Without storage, the list opens with its defaults next time.
  }
}
