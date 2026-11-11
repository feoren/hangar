// Context meter of a session row: how full the agent's context window is (backend:
// src-tauri/src/context_usage.rs). Checks the backend's snapshot and formats it. Pure, so it is
// tested without React; numbers only, nothing here is persisted.

import type { Translate } from "./i18n";
import { formatTokens } from "./subAgents";

/** Context use of one session (Rust: `SessionContext`). */
export interface SessionContext {
  /** Tokens in the context after the newest response. */
  used: number;
  /** Size of the model's context window. */
  window: number;
}

/** From this share on the meter warns: the agent will soon compact or run out of room. */
export const HIGH_SHARE = 0.8;

function count(value: unknown): number | null {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0 ? value : null;
}

/**
 * Checks a `context-usage-changed` payload or the answer of `context_usage`: session id to its
 * use. Broken entries (and a window of zero) are dropped; anything else yields `null`.
 */
export function parseContextSnapshot(value: unknown): Map<number, SessionContext> | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const list: unknown = Reflect.get(value, "sessions");
  if (!Array.isArray(list)) return null;
  const sessions = new Map<number, SessionContext>();
  for (const item of list) {
    if (typeof item !== "object" || item === null) continue;
    const id = count(Reflect.get(item, "sessionId"));
    const used = count(Reflect.get(item, "used"));
    const window = count(Reflect.get(item, "window"));
    if (id === null || used === null || window === null || window === 0) continue;
    sessions.set(id, { used, window });
  }
  return sessions;
}

/** Share of the window in use, 0 to 1 (a context beyond its window counts as full). */
export function contextShare(context: SessionContext): number {
  return context.window > 0 ? Math.min(1, context.used / context.window) : 0;
}

/** "12 %", "12%", "%12": the language's percent format, whole numbers. */
export function formatShare(share: number, language: string): string {
  return new Intl.NumberFormat(language, { style: "percent", maximumFractionDigits: 0 }).format(
    share,
  );
}

/** "123k", "96.3k", "1M": compact token counts, without a trailing ".0". */
export function compactTokens(tokens: number): string {
  return formatTokens(tokens).replace(/\.0(?=[kM]$)/, "");
}

/** Tooltip and accessible name: the share, the compact numbers and the exact ones. */
export function contextHint(context: SessionContext, language: string, t: Translate): string {
  const exact = new Intl.NumberFormat(language);
  return [
    t("context.hint", {
      percent: formatShare(contextShare(context), language),
      used: compactTokens(context.used),
      window: compactTokens(context.window),
    }),
    t("context.exact", {
      used: exact.format(context.used),
      window: exact.format(context.window),
    }),
  ].join("\n");
}
