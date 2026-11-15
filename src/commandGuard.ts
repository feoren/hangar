// The command guard's summary for Settings (backend `GuardInfo` in gate_service.rs): where its
// rules file is, which rules are in use, and what in the file could not be read.

import { field, isCount, list } from "./claims";
import { isShippedRule } from "./gate";
import type { Translate } from "./i18n";
import { revealInvisible } from "./invisible";

/** A rule or setting of command-guard.toml the backend could not use (Rust: `RuleProblem`). */
export interface GuardProblem {
  /** Position of the rule in the file, from 1; `null` for the file as a whole. */
  rule: number | null;
  id: string | null;
  /** From 1, for a syntax error. */
  line: number | null;
  message: string;
}

export interface GuardInfo {
  enabled: boolean;
  /** Absolute path of the rules file; empty when the backend has none. */
  file: string;
  rules: { id: string; description: string }[];
  problems: GuardProblem[];
  /** The file could not be read as a whole, so the shipped rules apply. */
  defaults: boolean;
  /** How many rules Hangar ships. */
  defaultCount: number;
}

/**
 * The agents whose team sessions send their shell commands to the guard (backend: Claude Code's
 * `Bash` and `PowerShell` hooks, the Pi and OpenCode extensions; ADR 0010).
 */
export const GUARDED_PROGRAMS = "Claude Code, Pi, OpenCode";

/** How many of the rules Hangar ships the file still holds; the user's own rules do not count. */
export function shippedRules(info: GuardInfo): number {
  return info.rules.filter((rule) => isShippedRule(rule.id)).length;
}

/** Whether Settings should point out that the file holds fewer rules than Hangar ships. */
export function fewerRules(info: GuardInfo): boolean {
  return !info.defaults && shippedRules(info) < info.defaultCount;
}

function optionalCount(value: unknown): number | null | undefined {
  if (value === null || value === undefined) return null;
  return isCount(value) ? value : undefined;
}

function parseProblem(value: unknown): GuardProblem | null {
  const rule = optionalCount(field(value, "rule"));
  const line = optionalCount(field(value, "line"));
  const id = field(value, "id");
  const message = field(value, "message");
  if (rule === undefined || line === undefined || typeof message !== "string") return null;
  if (id !== null && id !== undefined && typeof id !== "string") return null;
  return { rule, line, id: typeof id === "string" ? id : null, message };
}

function parseRuleSummary(value: unknown): { id: string; description: string } | null {
  const id = field(value, "id");
  const description = field(value, "description");
  if (typeof id !== "string" || id === "" || typeof description !== "string") return null;
  return { id, description };
}

/** Checks the answer of `command_guard_info`; `null` if it is malformed. */
export function parseGuardInfo(value: unknown): GuardInfo | null {
  const enabled = field(value, "enabled");
  const file = field(value, "file");
  const defaults = field(value, "defaults");
  const defaultCount = field(value, "defaultCount");
  const rules = list(field(value, "rules"), parseRuleSummary);
  const problems = list(field(value, "problems"), parseProblem);
  if (typeof enabled !== "boolean" || typeof file !== "string" || typeof defaults !== "boolean") {
    return null;
  }
  if (rules === null || problems === null || !isCount(defaultCount)) return null;
  return { enabled, file, rules, problems, defaults, defaultCount };
}

/** Where a problem sits: "Rule 3 (git-push)", "Line 5", or `null` for the file as a whole. */
export function problemPlace(problem: GuardProblem, t: Translate): string | null {
  if (problem.rule !== null) {
    const place = t("settings.commandGuardRule", { n: problem.rule });
    return problem.id === null ? place : `${place} (${revealInvisible(problem.id)})`;
  }
  if (problem.line !== null) return t("settings.commandGuardLine", { line: problem.line });
  return null;
}
