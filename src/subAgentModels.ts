// Models of sub-agents, as Settings > Sub-agents chooses them: per definition and as a default
// per program for every session Hangar starts. Pure; the backend checks everything again
// (agent_models.rs). Contract: docs/wiki/pages/syntheses/sub-agent-library.md.

import type { CodexDef, Library, MdCopy, SyncIssue } from "./subAgentLibrary";

/**
 * Rust `DefProgram`: whose definitions and how its sessions take a default model. Copilot CLI,
 * OpenCode and Gemini CLI only get copies of the user's definitions, no model.
 */
export type DefProgram = "claude" | "codex" | "copilot" | "opencode" | "gemini";

export const DEF_PROGRAMS: readonly DefProgram[] = [
  "claude",
  "codex",
  "copilot",
  "opencode",
  "gemini",
];

export function isDefProgram(value: unknown): value is DefProgram {
  return DEF_PROGRAMS.some((program) => program === value);
}

/** Rust `DefProgram::takes_model`: a default model for its sub-agents in Settings. */
export function takesModel(program: DefProgram): boolean {
  return program === "claude" || program === "codex";
}

/** Claude Code's model aliases offered (Rust `CLAUDE_ALIASES`); Fable costs clearly more. */
export const CLAUDE_ALIASES = ["opus", "sonnet", "haiku", "fable"] as const;
/** Codex CLI's reasoning efforts offered: those every model takes (Rust `CODEX_EFFORTS`). */
export const CODEX_EFFORTS = ["low", "medium", "high", "xhigh", "max"] as const;
export const MAX_MODEL_LENGTH = 64;

/** The backend's rule (`agent_models::valid_model`): a plain id, never `inherit`. */
export function validModel(value: string): boolean {
  return (
    value.length <= MAX_MODEL_LENGTH &&
    /^[A-Za-z0-9][A-Za-z0-9._:-]*$/.test(value) &&
    value.toLowerCase() !== "inherit"
  );
}

/** Rust `ModelChoice`; `null` leaves the program's own choice. */
export interface ModelChoice {
  model: string | null;
  effort: string | null;
}

export const NO_CHOICE: ModelChoice = { model: null, effort: null };

/** Rust `SubAgentSettings`. */
export interface SubAgentSettings {
  models: Partial<Record<DefProgram, ModelChoice>>;
  /** Programs that get no copies of the user's definitions. */
  provideOff: DefProgram[];
  /** Hangar's bundled sub-agents are not placed in `~/.claude/agents`. */
  bundledOff: boolean;
}

export const DEFAULT_SETTINGS: SubAgentSettings = {
  models: {},
  provideOff: [],
  bundledOff: false,
};

function field(value: object, name: string): unknown {
  return name in value ? Reflect.get(value, name) : undefined;
}

function text(value: unknown): string | null {
  return typeof value === "string" && value.length <= MAX_MODEL_LENGTH ? value : null;
}

function parseChoice(value: unknown): ModelChoice | null {
  if (typeof value !== "object" || value === null) return null;
  const model = text(field(value, "model"));
  const effort = text(field(value, "effort"));
  return model === null && effort === null ? null : { model, effort };
}

/** Checks `sub_agent_settings`' answer; anything unexpected gives the defaults' part. */
export function parseSubAgentSettings(value: unknown): SubAgentSettings {
  if (typeof value !== "object" || value === null) return DEFAULT_SETTINGS;
  const models: Partial<Record<DefProgram, ModelChoice>> = {};
  const raw = field(value, "models");
  if (typeof raw === "object" && raw !== null) {
    for (const program of DEF_PROGRAMS.filter(takesModel)) {
      const choice = parseChoice(field(raw, program));
      if (choice !== null) models[program] = choice;
    }
  }
  const off = field(value, "provideOff");
  const provideOff = Array.isArray(off) ? DEF_PROGRAMS.filter((p) => off.includes(p)) : [];
  return { models, provideOff, bundledOff: field(value, "bundledOff") === true };
}

/** `settings` with the default of `program` set; an empty choice removes it. */
export function withModel(
  settings: SubAgentSettings,
  program: DefProgram,
  choice: ModelChoice,
): SubAgentSettings {
  const models: Partial<Record<DefProgram, ModelChoice>> = {};
  for (const other of DEF_PROGRAMS) {
    const kept = other === program ? choice : settings.models[other];
    if (kept !== undefined && (kept.model !== null || kept.effort !== null)) models[other] = kept;
  }
  return { ...settings, models };
}

/** `settings` with copies for `program` on or off. */
export function withProvide(
  settings: SubAgentSettings,
  program: DefProgram,
  on: boolean,
): SubAgentSettings {
  const rest = settings.provideOff.filter((entry) => entry !== program);
  return { ...settings, provideOff: on ? rest : [...rest, program] };
}

/** `settings` with Hangar's bundled sub-agents on or off. */
export function withBundled(settings: SubAgentSettings, on: boolean): SubAgentSettings {
  return { ...settings, bundledOff: !on };
}

/** Value of the Claude model field for "another id", never a model. */
export const OTHER_MODEL = "\u0000other";

/** How Claude Code's model aliases read in the window; any other id is shown as it is. */
const MODEL_ALIAS_NAMES: Readonly<Record<string, string>> = {
  opus: "Opus",
  sonnet: "Sonnet",
  haiku: "Haiku",
};

/** A model id or alias as the window shows it ("sonnet" reads "Sonnet"). */
export function modelName(model: string): string {
  return MODEL_ALIAS_NAMES[model] ?? model;
}

/**
 * The entries of the Claude model field: the session's model, the aliases, a value the file
 * holds that is none of them (shown as it is, so nothing is lost) and "another id".
 */
export function claudeOptions(current: string | null): string[] {
  const aliases: string[] = [...CLAUDE_ALIASES];
  const known = current === null || aliases.includes(current);
  return ["", ...aliases, ...(known ? [] : [current]), OTHER_MODEL];
}

/** Whether the backend takes `effort` (`agent_models::valid_effort`); `null` is the session's. */
export function offeredEffort(effort: string | null): boolean {
  return effort === null || CODEX_EFFORTS.some((offered) => offered === effort);
}

/** The efforts offered, with a value the file holds that is none of them. */
export function effortOptions(current: string | null): string[] {
  const efforts: string[] = [...CODEX_EFFORTS];
  const known = current === null || efforts.includes(current);
  return ["", ...efforts, ...(known ? [] : [current])];
}

/** Model suggestions for Codex CLI: its main model and every model its definitions name. */
export function codexSuggestions(library: Library, mainModels: readonly string[]): string[] {
  const all = [...mainModels, ...library.codexDefs.map((def) => def.model)];
  return [...new Set(all.filter((model): model is string => model !== null && validModel(model)))];
}

/** Hangar's copies of the definition `name` (the global one first). */
export function copiesOf(library: Library, name: string): CodexDef[] {
  const copies = library.codexDefs.filter((def) => def.managed && def.name === name);
  return [
    ...copies.filter((def) => def.scope === "global"),
    ...copies.filter((def) => def.scope !== "global"),
  ];
}

/** The Codex CLI definitions of the user's own: not Hangar's copies. */
export function ownCodexDefs(library: Library): CodexDef[] {
  return library.codexDefs.filter((def) => !def.managed);
}

/** Whether copies go to `program` on this machine (installed, not switched off). */
export function provides(library: Library, program: DefProgram): boolean {
  return library.targets.some((target) => target.program === program && target.enabled);
}

/** What the newest copy runs left alone for the definition `name`. */
export function issuesOf(library: Library, name: string): SyncIssue[] {
  return library.syncIssues.filter((issue) => issue.name === name);
}

/**
 * The failures of `program`'s copy runs, for the list below its switch. A tracked project file
 * is no failure: the row of its definition names it.
 */
export function failures(library: Library, program: DefProgram): SyncIssue[] {
  return library.syncIssues.filter(
    (issue) => issue.program === program && issue.kind !== "tracked",
  );
}

/** The backups beside a copy (Codex CLI's or a Markdown one), the one its marker names first. */
export function backupsOf(copy: Pick<CodexDef, "backup" | "backups">): string[] {
  const all = copy.backup === null ? copy.backups : [copy.backup, ...copy.backups];
  return [...new Set(all)];
}

/** The Markdown programs that get copies here, in the order Settings lists them. */
export function markdownTargets(library: Library): DefProgram[] {
  return library.targets
    .filter((target) => target.enabled && !takesModel(target.program))
    .map((target) => target.program);
}

/** Hangar's copy of the definition `name` in the global folder of `program`. */
export function mdCopyOf(library: Library, program: DefProgram, name: string): MdCopy | undefined {
  return library.mdCopies.find((copy) => copy.program === program && copy.name === name);
}
