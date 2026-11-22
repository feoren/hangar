// Claude Code sub-agent definitions and their figures, as the window shows and starts them. Pure:
// the backend lists the files (agent_defs.rs) and keeps the chosen figures (agent_figures.rs);
// this module checks their answers, picks the figure an agent without a choice gets and decides
// what can start where. Contract: docs/wiki/pages/syntheses/sub-agent-library.md.

import { splitBackendError } from "./backendError";
import type { MessageKey } from "./i18n";
import { isDefProgram, type DefProgram } from "./subAgentModels";

export type DefScope = "global" | "project";

/** Rust `AgentDef`. */
export interface AgentDef {
  name: string;
  description: string;
  model: string | null;
  color: string | null;
  tools: string[];
  /** Frontmatter fields beyond the prompt it sets (`hooks`, `mcpServers`, `permissionMode`). */
  extras?: string[];
  scope: DefScope;
  /** The project folder of a project definition; `null` when global. */
  folder: string | null;
  file: string;
  /**
   * SHA-256 of the file without its `model` line (Rust `agent_defs::fingerprint`): a project copy
   * with the global definition's fingerprint is the same agent.
   */
  fingerprint?: string;
}

export type DefProblemKind =
  | "tooLarge"
  | "notText"
  | "noFrontmatter"
  | "missingName"
  | "invalidName"
  | "missingDescription"
  | "duplicate"
  | "link"
  | "unreadable"
  | "tooMany"
  | "notToml"
  | "missingInstructions";

const PROBLEM_KINDS: readonly DefProblemKind[] = [
  "tooLarge",
  "notText",
  "noFrontmatter",
  "missingName",
  "invalidName",
  "missingDescription",
  "duplicate",
  "link",
  "unreadable",
  "tooMany",
  "notToml",
  "missingInstructions",
];

/** Rust `DefProblem`: a file that is not listed as a definition, and why. */
export interface DefProblem {
  program: DefProgram;
  scope: DefScope;
  folder: string | null;
  file: string;
  problem: DefProblemKind;
}

/** Rust `CodexDef`: a Codex CLI definition, listed but never started as an agent. */
export interface CodexDef {
  name: string;
  description: string;
  model: string | null;
  effort: string | null;
  scope: DefScope;
  folder: string | null;
  file: string;
  /** Hangar's copy of the Claude Code definition of this name. */
  managed: boolean;
  /** The user's file Hangar set aside for the copy, beside it. */
  backup: string | null;
  /** Every backup of this name beside a copy (the original, hand edits set aside later). */
  backups: string[];
}

/** Rust `IssueKind`: why a copy run left a file or folder alone. */
export type SyncIssueKind =
  "unreadable" | "io" | "offMachine" | "noBackupName" | "tracked" | "link";

const ISSUE_KINDS: readonly SyncIssueKind[] = [
  "unreadable",
  "io",
  "offMachine",
  "noBackupName",
  "tracked",
  "link",
];

/** Rust `agent_provision::Issue`. */
export interface SyncIssue {
  program: DefProgram;
  scope: DefScope;
  folder: string | null;
  file: string;
  name: string | null;
  kind: SyncIssueKind;
}

/** Rust `TargetView`: a program that gets copies of the user's definitions. */
export interface ProvideTarget {
  program: DefProgram;
  enabled: boolean;
}

export interface Library {
  /** Claude Code's definitions, the ones that start as an agent. */
  defs: AgentDef[];
  codexDefs: CodexDef[];
  problems: DefProblem[];
  /** Installed programs that read sub-agent definitions. */
  installed: DefProgram[];
  targets: ProvideTarget[];
  /** Codex CLI's main model, as a suggestion. */
  codexModels: string[];
  /** What the newest copy runs left alone. */
  syncIssues: SyncIssue[];
  /** Global definitions that are Hangar's unchanged bundled copies. */
  bundled: string[];
}

export const EMPTY_LIBRARY: Library = {
  defs: [],
  codexDefs: [],
  problems: [],
  installed: [],
  targets: [],
  codexModels: [],
  syncIssues: [],
  bundled: [],
};

/** Rust `FigureView`: a chosen figure. */
export type FigureView = { kind: "builtin"; id: string } | { kind: "image"; dataUrl: string };

/** Rust `FigureInput`. `data`: the image file as Base64. */
export type FigureInput =
  { kind: "builtin"; id: string } | { kind: "image"; data: string } | { kind: "default" };

/** What is drawn for an agent: a bundled figure, an own image or its initial on its color. */
export type Figure =
  | { kind: "builtin"; id: string }
  | { kind: "image"; url: string }
  | { kind: "initial"; letter: string; color: string };

/**
 * The bundled figures (`src/assets/sub-agents/<id>.webp`, Rust `agent_figures::BUILTIN`); tests
 * keep all three equal.
 */
export const BUILTIN_FIGURES = [
  "backend-01",
  "backend-02",
  "backend-03",
  "comfyui-01",
  "contracts-01",
  "contracts-02",
  "developer-01",
  "developer-02",
  "developer-03",
  "developer-04",
  "finance-01",
  "finance-02",
  "frontend-01",
  "frontend-02",
  "frontend-03",
  "frontend-04",
  "manager-01",
  "manager-02",
  "manager-03",
  "manager-04",
  "marketing-01",
  "marketing-02",
  "n8n-01",
  "n8n-02",
  "n8n-03",
  "researcher-01",
  "researcher-02",
  "researcher-03",
  "researcher-04",
  "sales-01",
  "sales-02",
  "security-01",
  "security-02",
  "security-03",
  "social-media-01",
  "social-media-02",
  "sysadmin-01",
  "sysadmin-02",
  "tester-01",
  "tester-02",
  "tester-03",
] as const;

export type BuiltinFigure = (typeof BUILTIN_FIGURES)[number];

export function isBuiltinFigure(value: unknown): value is BuiltinFigure {
  return BUILTIN_FIGURES.some((id) => id === value);
}

/**
 * First row of visible pixels of each bundled figure, in pixels of the 360 px tall file, for the
 * figures that sit lower than the 2 px margin: a wide figure fills the width, not the height
 * (tools/figures.py prints it per file, a test there keeps this list equal to the files). The head
 * crop moves such a figure up by it, so the round box shows the face, not the hair.
 */
export const FIGURE_TOPS: Readonly<Partial<Record<BuiltinFigure, number>>> = {
  "comfyui-01": 56,
  "contracts-01": 9,
  "contracts-02": 67,
  "finance-02": 36,
  "frontend-03": 42,
  "n8n-01": 50,
  "n8n-03": 44,
  "researcher-03": 6,
  "social-media-01": 23,
  "social-media-02": 50,
  "sysadmin-01": 67,
  "tester-02": 14,
};

/** Empty margin around a bundled figure, in pixels of the 360 px tall file. */
export const FIGURE_PADDING = 2;

/** Top row of a bundled figure's visible pixels (2 for one that fills the height). */
export function figureTop(id: string): number {
  return isBuiltinFigure(id) ? (FIGURE_TOPS[id] ?? FIGURE_PADDING) : FIGURE_PADDING;
}

/** Fields a definition may set beyond its prompt (Rust `POWER_FIELDS`). */
export const POWER_FIELDS = ["hooks", "mcpServers", "permissionMode"] as const;

/** Largest own image the backend takes. */
export const MAX_FIGURE_BYTES = 2 * 1024 * 1024;
/** Image types an own figure may have; SVG never (it can carry script). */
export const FIGURE_TYPES = ["image/png", "image/gif", "image/webp", "image/jpeg"] as const;
/** Largest definition text the backend takes. */
export const MAX_DEF_BYTES = 256 * 1024;

/** The name rule of the backend (`agent_defs::valid_name`). */
export function validAgentName(name: unknown): name is string {
  return (
    typeof name === "string" && name.length <= 64 && /^[a-z0-9](?:[a-z0-9-]*[a-z0-9])?$/.test(name)
  );
}

/** Words in an agent's name that point to a role of the bundled figures. */
const ROLE_WORDS: Readonly<Record<string, string>> = {
  backend: "backend",
  api: "backend",
  server: "backend",
  database: "backend",
  developer: "developer",
  dev: "developer",
  engineer: "developer",
  coder: "developer",
  frontend: "frontend",
  ui: "frontend",
  ux: "frontend",
  designer: "frontend",
  manager: "manager",
  lead: "manager",
  planner: "manager",
  reviewer: "manager",
  researcher: "researcher",
  research: "researcher",
  explorer: "researcher",
  docs: "researcher",
  security: "security",
  sec: "security",
  auditor: "security",
  tester: "tester",
  test: "tester",
  qa: "tester",
  marketing: "marketing",
  brand: "marketing",
  sales: "sales",
  vertrieb: "sales",
  social: "social-media",
  community: "social-media",
  sysadmin: "sysadmin",
  admin: "sysadmin",
  ops: "sysadmin",
  devops: "sysadmin",
  n8n: "n8n",
  automation: "n8n",
  comfyui: "comfyui",
  finance: "finance",
  accounting: "finance",
  tax: "finance",
  contracts: "contracts",
  contract: "contracts",
  legal: "contracts",
};

/** Claude Code's agent colors (`color:` in the frontmatter), readable on light and dark. */
const COLORS: Readonly<Record<string, string>> = {
  red: "#d9534f",
  orange: "#e8833a",
  yellow: "#d4a72c",
  green: "#3fa66b",
  cyan: "#2fa8b8",
  blue: "#3b82d6",
  purple: "#8b5cc8",
  pink: "#d6589a",
};

function hash(text: string): number {
  let value = 2166136261;
  for (let index = 0; index < text.length; index++) {
    value = Math.imul(value ^ text.charCodeAt(index), 16777619) >>> 0;
  }
  return value;
}

/** Name prefix of the sub-agents that ship with Hangar (Rust `bundled_agents::BUNDLED`). */
export const HANGAR_PREFIX = "hangar-";

/**
 * Only Hangar's own sub-agents wear figures. Any other agent (the user's own, a project's, a
 * Codex CLI definition, a built-in type such as `Explore`) shows its initial: a figure would
 * suggest it came with Hangar.
 */
export function isHangarAgent(name: string): boolean {
  return name.startsWith(HANGAR_PREFIX) && name.length > HANGAR_PREFIX.length;
}

const TINTS = Object.values(COLORS);

/** The initial of an agent without a figure, on its frontmatter color, else one its name picks. */
export function initialFigure(name: string, color: string | null): Figure {
  const tint = (color === null ? undefined : COLORS[color]) ?? TINTS[hash(name) % TINTS.length];
  return {
    kind: "initial",
    letter: name.slice(0, 1).toUpperCase(),
    color: tint ?? NEUTRAL_FIGURE_COLOR,
  };
}

/**
 * The figure of an agent without a chosen one. A Hangar agent wears the role figure the word
 * after the prefix points to (`hangar-tester` -> tester), else a bundled figure picked from the
 * name's hash, the same on every start. Every other agent gets its initial.
 */
export function defaultFigure(name: string, color: string | null): Figure {
  if (!isHangarAgent(name)) return initialFigure(name, color);
  for (const word of name.slice(HANGAR_PREFIX.length).split("-")) {
    const role = ROLE_WORDS[word];
    const id = role === undefined ? undefined : `${role}-01`;
    if (isBuiltinFigure(id)) return { kind: "builtin", id };
  }
  const id = NAME_FIGURES[hash(name) % NAME_FIGURES.length] ?? "developer-01";
  return { kind: "builtin", id };
}

/**
 * The figures a name without a role word is spread over. Frozen at the first set: picking from
 * `BUILTIN_FIGURES` would hand such an agent another figure whenever new figures are bundled.
 */
export const NAME_FIGURES: readonly BuiltinFigure[] = [
  "backend-01",
  "developer-01",
  "developer-02",
  "frontend-01",
  "frontend-02",
  "manager-01",
  "manager-02",
  "researcher-01",
  "researcher-02",
  "security-01",
  "security-02",
  "tester-01",
];

/**
 * The chosen figure, else the default one. A choice stored for a name that is no Hangar agent is
 * ignored, not deleted.
 */
export function figureFor(
  name: string,
  color: string | null,
  chosen: ReadonlyMap<string, FigureView>,
): Figure {
  if (!isHangarAgent(name)) return initialFigure(name, color);
  const view = chosen.get(name);
  if (view?.kind === "builtin" && isBuiltinFigure(view.id)) return view;
  if (view?.kind === "image") return { kind: "image", url: view.dataUrl };
  return defaultFigure(name, color);
}

/** One name with every place it is defined: the global file first, then the projects. */
export interface LibraryEntry {
  name: string;
  /** The definition the row describes: the global one, else the first project's. */
  def: AgentDef;
  global: AgentDef | null;
  projects: AgentDef[];
}

/**
 * The listing grouped by name, in listing order. A name defined globally and in projects is one
 * entry; in a project folder Claude Code takes the project's definition (`effectiveDef`).
 */
export function libraryEntries(library: Library): LibraryEntry[] {
  const entries = new Map<string, LibraryEntry>();
  for (const def of library.defs) {
    const entry = entries.get(def.name);
    if (entry === undefined) {
      entries.set(def.name, {
        name: def.name,
        def,
        global: def.scope === "global" ? def : null,
        projects: def.scope === "project" ? [def] : [],
      });
    } else if (def.scope === "global") {
      entries.set(def.name, { ...entry, def, global: def });
    } else {
      entry.projects.push(def);
    }
  }
  return [...entries.values()];
}

/** Paths compared the way the user's file system does on Windows: case and slash insensitive. */
function samePath(a: string, b: string): boolean {
  const norm = (path: string) =>
    path
      .replace(/[\\/]+$/, "")
      .replace(/\\/g, "/")
      .toLowerCase();
  return norm(a) === norm(b);
}

/**
 * The definitions a session started in `folder` can run as: that project's, then the global
 * ones it does not override, each name once.
 */
export function defsFor(library: Library, folder: string | null): AgentDef[] {
  const project =
    folder === null
      ? []
      : library.defs.filter(
          (def) => def.scope === "project" && def.folder !== null && samePath(def.folder, folder),
        );
  const taken = new Set(project.map((def) => def.name));
  const global = library.defs.filter((def) => def.scope === "global" && !taken.has(def.name));
  return [...project, ...global];
}

/**
 * A project definition whose name a global definition has as well, with other content: it
 * overrides it there. A copy with the same fingerprint is the same agent and overrides nothing.
 */
export function overridesGlobal(library: Library, def: AgentDef): boolean {
  return (
    def.scope === "project" &&
    library.defs.some(
      (other) =>
        other.scope === "global" &&
        other.name === def.name &&
        (def.fingerprint === undefined || other.fingerprint !== def.fingerprint),
    )
  );
}

/** A project definition with the fingerprint of the global definition of its name. */
function sameAsGlobal(library: Library, def: AgentDef): boolean {
  return (
    def.fingerprint !== undefined &&
    library.defs.some(
      (other) =>
        other.scope === "global" &&
        other.name === def.name &&
        other.fingerprint === def.fingerprint,
    )
  );
}

/** Color of the figure of a project definition that overrides a global one. */
export const NEUTRAL_FIGURE_COLOR = "#6b7785";

/**
 * The figure a definition is drawn with. Figures belong to names and so to the global
 * definitions; a project definition that overrides a global one of its name never wears that
 * figure (a repository could otherwise pass off its own agent as the trusted one): it shows its
 * initial on a neutral color.
 */
export function defFigure(
  def: AgentDef,
  library: Library,
  chosen: ReadonlyMap<string, FigureView>,
): Figure {
  if (overridesGlobal(library, def)) {
    return {
      kind: "initial",
      letter: def.name.slice(0, 1).toUpperCase(),
      color: NEUTRAL_FIGURE_COLOR,
    };
  }
  // A project's own definition is the repository's, never Hangar's, whatever its name; only a
  // copy of the global definition (equal fingerprint) is that agent.
  if (def.scope === "project" && !sameAsGlobal(library, def)) {
    return initialFigure(def.name, def.color);
  }
  return figureFor(def.name, def.color, chosen);
}

/** A session's figure, and the project folder when its definition comes from a project. */
export interface SessionFigure {
  figure: Figure;
  project: string | null;
}

/**
 * The listing shown was read for `cwd`: it has arrived (`read` is not `null`) and `cwd` is one
 * of the project folders it was read for. Only then does it name every definition a session
 * there can run as; otherwise a project definition may be missing that overrides a global one.
 */
export function libraryCovers(read: readonly string[] | null, cwd: string): boolean {
  return read !== null && read.some((folder) => samePath(folder, cwd));
}

/**
 * The figure of a session running as `name` in `cwd`: the definition Claude Code takes there
 * (`defsFor`), or the name's own figure when no definition of it is listed right now. `read`:
 * the folders the listing was read for, `null` before the first one arrives. While the listing
 * does not cover `cwd` (not read yet, or `cwd` beyond the folder limit), the session wears its
 * default figure without the chosen one: a project definition not yet listed could override the
 * global one, and a repository's agent must never wear the trusted figure, not even briefly.
 */
export function sessionFigure(
  library: Library,
  chosen: ReadonlyMap<string, FigureView>,
  name: string,
  cwd: string,
  read: readonly string[] | null,
): SessionFigure {
  if (!libraryCovers(read, cwd)) return { figure: defaultFigure(name, null), project: null };
  const def = defsFor(library, cwd).find((entry) => entry.name === name);
  if (def === undefined) return { figure: figureFor(name, null, chosen), project: null };
  return {
    figure: defFigure(def, library, chosen),
    project: def.scope === "project" ? def.folder : null,
  };
}

/**
 * The figure of a running sub-agent of type `name` in a session started in `cwd`: the definition
 * Claude Code takes in that folder (`defsFor`), so a project that overrides the name elsewhere
 * leaves this session's figure alone. Without a folder, any project override keeps the chosen
 * figure off, as a repository's agent must never wear the trusted one. `null` when no definition
 * of that name is listed. `read` as in `sessionFigure`: while the listing does not cover `cwd`,
 * the default figure without the chosen one, as a project definition not yet listed could
 * override the global one.
 */
export function runningFigure(
  library: Library,
  chosen: ReadonlyMap<string, FigureView>,
  name: string,
  cwd: string | null,
  read: readonly string[] | null,
): Figure | null {
  if (cwd !== null && !libraryCovers(read, cwd)) return defaultFigure(name, null);
  if (cwd === null) {
    const defs = library.defs.filter((entry) => entry.name === name);
    const def = defs.find((entry) => overridesGlobal(library, entry)) ?? defs[0];
    return def === undefined ? null : defFigure(def, library, chosen);
  }
  const def = defsFor(library, cwd).find((entry) => entry.name === name);
  return def === undefined ? null : defFigure(def, library, chosen);
}

/** The first line of a description, for a menu entry. */
export function firstLine(text: string): string {
  return text.split(/\r?\n/)[0]?.trim() ?? "";
}

function text(value: unknown, max: number): string | null {
  return typeof value === "string" ? value.slice(0, max) : null;
}

function field(value: object, name: string): unknown {
  return name in value ? Reflect.get(value, name) : undefined;
}

function scopeOf(value: unknown): DefScope | null {
  return value === "global" || value === "project" ? value : null;
}

function parseDef(value: unknown): AgentDef | null {
  if (typeof value !== "object" || value === null) return null;
  const name = field(value, "name");
  const description = text(field(value, "description"), 4000);
  const scope = scopeOf(field(value, "scope"));
  const file = text(field(value, "file"), 512);
  const folder = field(value, "folder");
  const tools = field(value, "tools");
  const fingerprint = text(field(value, "fingerprint"), 64);
  const extraField = field(value, "extras");
  const extras: unknown[] = Array.isArray(extraField) ? extraField : [];
  if (!validAgentName(name) || description === null || scope === null || file === null) {
    return null;
  }
  if (scope === "project" && typeof folder !== "string") return null;
  return {
    name,
    description,
    model: text(field(value, "model"), 64),
    color: text(field(value, "color"), 64),
    tools: Array.isArray(tools)
      ? tools.filter((tool): tool is string => typeof tool === "string").slice(0, 64)
      : [],
    extras: POWER_FIELDS.filter((name) => extras.includes(name)),
    scope,
    folder: scope === "project" && typeof folder === "string" ? folder : null,
    file,
    ...(fingerprint === null ? {} : { fingerprint }),
  };
}

function parseProblem(value: unknown): DefProblem | null {
  if (typeof value !== "object" || value === null) return null;
  const scope = scopeOf(field(value, "scope"));
  const file = text(field(value, "file"), 512);
  const problem = field(value, "problem");
  const folder = field(value, "folder");
  const program = field(value, "program");
  const kind = PROBLEM_KINDS.find((known) => known === problem);
  if (scope === null || file === null || kind === undefined) return null;
  return {
    program: isDefProgram(program) ? program : "claude",
    scope,
    folder: typeof folder === "string" ? folder : null,
    file,
    problem: kind,
  };
}

function parseCodexDef(value: unknown): CodexDef | null {
  if (typeof value !== "object" || value === null) return null;
  const name = field(value, "name");
  const description = text(field(value, "description"), 4000);
  const scope = scopeOf(field(value, "scope"));
  const file = text(field(value, "file"), 512);
  const folder = field(value, "folder");
  if (!validAgentName(name) || description === null || scope === null || file === null) {
    return null;
  }
  if (scope === "project" && typeof folder !== "string") return null;
  return {
    name,
    description,
    model: text(field(value, "model"), 64),
    effort: text(field(value, "effort"), 64),
    scope,
    folder: scope === "project" && typeof folder === "string" ? folder : null,
    file,
    managed: field(value, "managed") === true,
    backup: text(field(value, "backup"), 160),
    backups: strings(field(value, "backups"), 160).slice(0, 16),
  };
}

function strings(value: unknown, max: number): string[] {
  if (!Array.isArray(value)) return [];
  const items: unknown[] = value;
  return items.filter((item): item is string => typeof item === "string" && item.length <= max);
}

function parseIssue(value: unknown): SyncIssue | null {
  if (typeof value !== "object" || value === null) return null;
  const program = field(value, "program");
  const scope = scopeOf(field(value, "scope"));
  const folder = field(value, "folder");
  const file = text(field(value, "file"), 512);
  const name = field(value, "name");
  const kind = ISSUE_KINDS.find((known) => known === field(value, "kind"));
  if (!isDefProgram(program) || scope === null || file === null || kind === undefined) {
    return null;
  }
  return {
    program,
    scope,
    folder: typeof folder === "string" ? folder : null,
    file,
    name: validAgentName(name) ? name : null,
    kind,
  };
}

function issues(value: unknown): SyncIssue[] {
  if (!Array.isArray(value)) return [];
  const items: unknown[] = value;
  return items
    .map(parseIssue)
    .filter((issue): issue is SyncIssue => issue !== null)
    .slice(0, 64);
}

function programs(value: unknown): DefProgram[] {
  return Array.isArray(value) ? value.filter(isDefProgram) : [];
}

function parseTargets(value: unknown): ProvideTarget[] {
  if (!Array.isArray(value)) return [];
  const out: ProvideTarget[] = [];
  const entries: unknown[] = value;
  for (const entry of entries) {
    if (typeof entry !== "object" || entry === null) continue;
    const program = field(entry, "program");
    const enabled = field(entry, "enabled");
    if (isDefProgram(program) && typeof enabled === "boolean") out.push({ program, enabled });
  }
  return out;
}

/** Checks `sub_agent_library`'s answer; anything unexpected is left out. */
export function parseLibrary(value: unknown): Library | null {
  if (typeof value !== "object" || value === null) return null;
  const defs = field(value, "defs");
  const problems = field(value, "problems");
  if (!Array.isArray(defs) || !Array.isArray(problems)) return null;
  const codexDefs = field(value, "codexDefs");
  const models = field(value, "codexModels");
  return {
    defs: defs.map(parseDef).filter((def): def is AgentDef => def !== null),
    codexDefs: Array.isArray(codexDefs)
      ? codexDefs.map(parseCodexDef).filter((def): def is CodexDef => def !== null)
      : [],
    problems: problems.map(parseProblem).filter((p): p is DefProblem => p !== null),
    installed: programs(field(value, "installed")),
    targets: parseTargets(field(value, "targets")),
    codexModels: Array.isArray(models)
      ? models.filter((model): model is string => typeof model === "string").slice(0, 8)
      : [],
    syncIssues: issues(field(value, "syncIssues")),
    bundled: strings(field(value, "bundled"), 64).filter(validAgentName).slice(0, 64),
  };
}

/** Whether the global definition `name` is Hangar's bundled copy, unchanged. */
export function isFromHangar(library: Library, entry: LibraryEntry): boolean {
  return entry.global !== null && library.bundled.includes(entry.name);
}

/** Checks the figures map; unknown figures and invalid names are left out. */
export function parseFigures(value: unknown): Map<string, FigureView> {
  const out = new Map<string, FigureView>();
  if (typeof value !== "object" || value === null || Array.isArray(value)) return out;
  const entries: [string, unknown][] = Object.entries(value);
  for (const [name, view] of entries) {
    if (!validAgentName(name) || typeof view !== "object" || view === null) continue;
    const kind = field(view, "kind");
    const id = field(view, "id");
    const dataUrl = field(view, "dataUrl");
    if (kind === "builtin" && isBuiltinFigure(id)) out.set(name, { kind, id });
    if (kind === "image" && typeof dataUrl === "string" && isFigureDataUrl(dataUrl)) {
      out.set(name, { kind, dataUrl });
    }
  }
  return out;
}

/** A `data:` URL of one of the image types the backend checked; nothing else is shown. */
export function isFigureDataUrl(url: string): boolean {
  return FIGURE_TYPES.some((type) => url.startsWith(`data:${type};base64,`));
}

/** Codes of Rust `FigureError` the window words itself. */
export type FigureErrorCode = "no_image" | "not_an_image" | "too_large";

/** The stable code of a figure error (`[code] text`), `null` for any other error. */
export function figureErrorCode(error: unknown): FigureErrorCode | null {
  if (typeof error !== "string") return null;
  const { code } = splitBackendError(error);
  return code === "no_image" || code === "not_an_image" || code === "too_large" ? code : null;
}

/** Rust `DefError` as the window gets it. */
export interface DefError {
  kind: "tooLarge" | "invalid" | "exists" | "reserved" | "notFound" | "io" | "model" | "confirm";
  message: string;
  problem: DefProblemKind | null;
}

export function parseDefError(value: unknown): DefError | null {
  if (typeof value !== "object" || value === null) return null;
  const kind = field(value, "kind");
  const message = field(value, "message");
  const problem = field(value, "problem");
  const kinds = [
    "tooLarge",
    "invalid",
    "exists",
    "reserved",
    "notFound",
    "io",
    "model",
    "confirm",
  ] as const;
  const known = kinds.find((candidate) => candidate === kind);
  if (known === undefined || typeof message !== "string") return null;
  return {
    kind: known,
    message,
    problem: PROBLEM_KINDS.find((candidate) => candidate === problem) ?? null,
  };
}

/** The name a pasted text would be saved under, for the form's hint; `null` without one. */
export function pastedName(text: string): string | null {
  const head = /^\uFEFF?---\r?\n([\s\S]*?)\r?\n---/.exec(text);
  const line = head?.[1]?.split(/\r?\n/).find((entry) => /^name\s*:/.test(entry));
  const name = line
    ?.replace(/^name\s*:/, "")
    .trim()
    .replace(/^["']|["']$/g, "");
  return validAgentName(name) ? name : null;
}

/** A template for the paste field. */
export const DEF_TEMPLATE = `---
name: code-reviewer
description: Reviews changes for bugs and unclear code. Use after every larger change.
model: sonnet
color: blue
---

You are a careful code reviewer. ...
`;

/** Role of a built-in figure (`developer-02`), worded for its button. */
const ROLE_NAMES: Readonly<Record<string, MessageKey>> = {
  backend: "subAgentLib.role.backend",
  developer: "subAgentLib.role.developer",
  frontend: "subAgentLib.role.frontend",
  manager: "subAgentLib.role.manager",
  researcher: "subAgentLib.role.researcher",
  security: "subAgentLib.role.security",
  tester: "subAgentLib.role.tester",
  marketing: "subAgentLib.role.marketing",
  sales: "subAgentLib.role.sales",
  "social-media": "subAgentLib.role.socialMedia",
  sysadmin: "subAgentLib.role.sysadmin",
  n8n: "subAgentLib.role.n8n",
  comfyui: "subAgentLib.role.comfyui",
  finance: "subAgentLib.role.finance",
  contracts: "subAgentLib.role.contracts",
};

/** "Developer 2" for `developer-02`; the id itself for a role without a name. */
export function figureName(id: string, t: (key: MessageKey) => string): string {
  const dash = id.lastIndexOf("-");
  const role = ROLE_NAMES[id.slice(0, dash)];
  const number = Number(id.slice(dash + 1));
  return role === undefined || !Number.isFinite(number) ? id : `${t(role)} ${String(number)}`;
}
