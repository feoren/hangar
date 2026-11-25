// Running sub-agents per session, for the count on its sidebar row (backend:
// src-tauri/src/subagent_counts.rs). Checks the backend's snapshot and formats the count. Pure,
// so it is tested without React; numbers only, nothing here is persisted.

import type { Translate } from "./i18n";
import type { SubAgent } from "./subAgents";

/** From this count on the badge shows "9+": two digits would not fit the round dot. */
export const MAX_SHOWN = 9;

function count(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

/**
 * Checks a `sub-agent-counts-changed` payload or the answer of `sub_agent_counts`: session id to
 * its running sub-agents, only those above zero. Broken entries are dropped; anything else
 * yields `null`.
 */
export function parseSubAgentCounts(value: unknown): Map<number, number> | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const list: unknown = Reflect.get(value, "sessions");
  if (!Array.isArray(list)) return null;
  const sessions = new Map<number, number>();
  for (const item of list) {
    if (typeof item !== "object" || item === null) continue;
    const id = count(Reflect.get(item, "sessionId"));
    const running = count(Reflect.get(item, "running"));
    if (id === null || running === null || running === 0) continue;
    sessions.set(id, running);
  }
  return sessions;
}

/** Running sub-agents of a watched session's snapshot, the same status its tree shows. */
export function runningIn(agents: readonly SubAgent[]): number {
  return agents.filter((agent) => agent.status === "running").length;
}

/** What the badge shows: the count, from ten on "9+". */
export function badgeText(running: number): string {
  return running > MAX_SHOWN ? `${String(MAX_SHOWN)}+` : String(running);
}

/** Tooltip and accessible name, with the exact count. */
export function badgeLabel(running: number, t: Translate): string {
  return t("subAgents.runningCount", { count: running });
}
