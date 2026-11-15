// Agent arena: one task for 2 to 4 agents, each in its own worktree from the same commit. Pure
// logic for the comparison: checks of the backend's answers (arena.rs), the state of each
// contestant, when to send the task, when to run the tests and which result leads.
// Contract: docs/wiki/pages/syntheses/agent-arena.md.

import type { Worktree } from "./api";
import type { AttentionState } from "./attention";
import { splitBackendError } from "./backendError";
import type { MessageKey, Translate } from "./i18n";
import { knownWorktreeErrorText, parseWorktree } from "./worktree";

export const ARENA_MIN = 2;
export const ARENA_MAX = 4;

/** Answer of `arena_create` (Rust `Arena`). */
export interface ArenaSetup {
  id: string;
  baseCommit: string;
  repoRoot: string;
  worktrees: Worktree[];
}

export interface FileStat {
  path: string;
  added: number;
  removed: number;
}

/** Answer of `arena_diff_stat` (Rust `DiffStat`). */
export interface DiffStat {
  files: FileStat[];
  added: number;
  removed: number;
  truncated: boolean;
}

/** Answer of `arena_apply` (Rust `ApplyOutcome`). */
export type ApplyOutcome =
  { kind: "applied"; files: string[] } | { kind: "conflict"; files: string[] };

/** Answer of `arena_run_tests` (Rust `TestRun`). */
export interface TestRun {
  passed: boolean;
  exitCode: number | null;
  timedOut: boolean;
  durationMs: number;
  output: string;
}

function field(value: unknown, name: string): unknown {
  return typeof value === "object" && value !== null ? Reflect.get(value, name) : undefined;
}

function count(value: unknown): number | null {
  return typeof value === "number" && Number.isInteger(value) && value >= 0 ? value : null;
}

/** The items of an array as `unknown`, so each is checked before use. */
function items(value: unknown[]): readonly unknown[] {
  return value;
}

function strings(value: unknown): string[] | null {
  if (!Array.isArray(value)) return null;
  const list: string[] = [];
  for (const item of items(value)) {
    if (typeof item !== "string") return null;
    list.push(item);
  }
  return list;
}

const COMMIT = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;

export function parseArena(value: unknown): ArenaSetup | null {
  const id = field(value, "id");
  const baseCommit = field(value, "baseCommit");
  const repoRoot = field(value, "repoRoot");
  const raw = field(value, "worktrees");
  if (typeof id !== "string" || !/^[0-9a-f]{6}$/.test(id)) return null;
  if (typeof baseCommit !== "string" || !COMMIT.test(baseCommit)) return null;
  if (typeof repoRoot !== "string" || repoRoot === "" || !Array.isArray(raw)) return null;
  const worktrees: Worktree[] = [];
  for (const item of items(raw)) {
    const worktree = parseWorktree(item);
    if (worktree === null || !worktree.branch.startsWith(`hangar/arena-${id}-`)) return null;
    worktrees.push(worktree);
  }
  return worktrees.length >= ARENA_MIN && worktrees.length <= ARENA_MAX
    ? { id, baseCommit, repoRoot, worktrees }
    : null;
}

export function parseDiffStat(value: unknown): DiffStat | null {
  const raw = field(value, "files");
  const added = count(field(value, "added"));
  const removed = count(field(value, "removed"));
  const truncated = field(value, "truncated");
  if (!Array.isArray(raw) || added === null || removed === null) return null;
  if (typeof truncated !== "boolean") return null;
  const files: FileStat[] = [];
  for (const item of items(raw)) {
    const path = field(item, "path");
    const fileAdded = count(field(item, "added"));
    const fileRemoved = count(field(item, "removed"));
    if (typeof path !== "string" || fileAdded === null || fileRemoved === null) return null;
    files.push({ path, added: fileAdded, removed: fileRemoved });
  }
  return { files, added, removed, truncated };
}

export function parseApplyOutcome(value: unknown): ApplyOutcome | null {
  const kind = field(value, "kind");
  const files = strings(field(value, "files"));
  if (files === null) return null;
  if (kind === "applied" || kind === "conflict") return { kind, files };
  return null;
}

export function parseTestRun(value: unknown): TestRun | null {
  const passed = field(value, "passed");
  const exitCode = field(value, "exitCode");
  const timedOut = field(value, "timedOut");
  const durationMs = count(field(value, "durationMs"));
  const output = field(value, "output");
  if (typeof passed !== "boolean" || typeof timedOut !== "boolean") return null;
  if (exitCode !== null && typeof exitCode !== "number") return null;
  if (durationMs === null || typeof output !== "string") return null;
  return { passed, exitCode, timedOut, durationMs, output };
}

/** The test run of a contestant, as the comparison shows it. */
export type TestState =
  | { kind: "none" }
  | { kind: "running" }
  | { kind: "done"; run: TestRun }
  | { kind: "error"; message: string };

/** One agent of the arena, as the app tracks it. */
export interface Contestant {
  sessionId: number;
  profileId: string;
  /** Program name, as the comparison lists it. */
  name: string;
  worktree: Worktree;
  /** The task went into the session (paste and Enter). */
  promptSent: boolean;
  /** The agent was seen working after the task arrived; only then can it be done. */
  sawWorking: boolean;
  /** When it last stopped working (Date.now); `null` while it works or has not started. */
  doneAt: number | null;
  tests: TestState;
  /** `doneAt` the last test run belongs to, so a done agent is tested once per finish. */
  testedFor: number | null;
}

export interface ArenaRun {
  id: string;
  baseCommit: string;
  /** The project folder the winner is taken over into. */
  repoRoot: string;
  prompt: string;
  /** Empty: no tests. */
  testCommand: string;
  /** Date.now at the start. */
  startedAt: number;
  contestants: Contestant[];
}

export function newContestant(
  sessionId: number,
  profileId: string,
  name: string,
  worktree: Worktree,
): Contestant {
  return {
    sessionId,
    profileId,
    name,
    worktree,
    promptSent: false,
    sawWorking: false,
    doneAt: null,
    tests: { kind: "none" },
    testedFor: null,
  };
}

export type ContestantState = "starting" | "working" | "done" | "ended";

/**
 * "starting" until the task is sent, "ended" once the process exited, "done" when the agent
 * worked on the task and went quiet, otherwise "working" (also right after the task arrived and
 * before the first output, so a slow start never reads as done).
 */
export function contestantState(
  contestant: Contestant,
  attention: AttentionState | undefined,
  exited: boolean,
): ContestantState {
  if (exited) return "ended";
  if (!contestant.promptSent) return "starting";
  if (attention === "working") return "working";
  return contestant.sawWorking ? "done" : "working";
}

/**
 * A contestant after an attention change: working clears the finish time, the first quiet moment
 * after working sets it. Before the task arrived nothing counts (the agent's own start screen).
 * Returns the same object when nothing changed.
 */
export function trackAttention(
  contestant: Contestant,
  attention: AttentionState | undefined,
  now: number,
): Contestant {
  if (!contestant.promptSent) return contestant;
  if (attention === "working") {
    return contestant.sawWorking && contestant.doneAt === null
      ? contestant
      : { ...contestant, sawWorking: true, doneAt: null };
  }
  if (contestant.sawWorking && contestant.doneAt === null) return { ...contestant, doneAt: now };
  return contestant;
}

/** Time from the arena's start to the contestant's finish, or to `now` while it works. */
export function elapsedMs(arena: ArenaRun, contestant: Contestant, now: number): number {
  return Math.max(0, (contestant.doneAt ?? now) - arena.startedAt);
}

/** A stopwatch reading, the same in every language: "0:42", "4:05", "1:02:03". */
export function clockText(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const seconds = String(total % 60).padStart(2, "0");
  const minutes = Math.floor(total / 60) % 60;
  const hours = Math.floor(total / 3600);
  return hours > 0
    ? `${String(hours)}:${String(minutes).padStart(2, "0")}:${seconds}`
    : `${String(minutes)}:${seconds}`;
}

/** Agent CLIs draw their start screen in bursts; quiet this long means the prompt is ready. */
export const PROMPT_QUIET_MS = 2000;
/** Never earlier: a CLI may pause between its banner and its prompt. */
export const PROMPT_MIN_WAIT_MS = 3000;
/** Never later: a CLI that keeps animating still gets the task. */
export const PROMPT_MAX_WAIT_MS = 45000;

/**
 * Whether to paste the task now. `startedAt`, `lastOutputAt` and `now` on one clock;
 * `lastOutputAt` `null` while the session printed nothing yet.
 */
export function promptReady(startedAt: number, lastOutputAt: number | null, now: number): boolean {
  const waited = now - startedAt;
  if (waited >= PROMPT_MAX_WAIT_MS) return true;
  if (waited < PROMPT_MIN_WAIT_MS || lastOutputAt === null) return false;
  return now - lastOutputAt >= PROMPT_QUIET_MS;
}

/** Tests are due once per finish, when a command is set and no run is going. */
export function testsDue(contestant: Contestant, state: ContestantState, command: string): boolean {
  return (
    command.trim() !== "" &&
    state === "done" &&
    contestant.doneAt !== null &&
    contestant.testedFor !== contestant.doneAt &&
    contestant.tests.kind !== "running"
  );
}

/** What the comparison needs per contestant to pick the leaders. */
export interface Standing {
  sessionId: number;
  state: ContestantState;
  elapsedMs: number;
  /** `null` while the changes are not known yet. */
  stat: DiffStat | null;
  tests: TestState;
}

/** Tests passed, failed or have not run (none, running or not startable). */
function testRank(tests: TestState): number {
  if (tests.kind !== "done") return 1;
  return tests.run.passed ? 0 : 2;
}

/**
 * The contestant to look at first: finished with changes, tests passed before untested before
 * failed, then the quicker one, then the smaller change. `null` while none is finished with
 * changes.
 */
export function leader(standings: readonly Standing[]): number | null {
  const ready = standings.filter(
    (s) =>
      (s.state === "done" || s.state === "ended") && s.stat !== null && s.stat.files.length > 0,
  );
  const size = (s: Standing) => (s.stat ? s.stat.added + s.stat.removed : 0);
  ready.sort(
    (a, b) =>
      testRank(a.tests) - testRank(b.tests) || a.elapsedMs - b.elapsedMs || size(a) - size(b),
  );
  return ready[0]?.sessionId ?? null;
}

/**
 * A winner can be taken over once it stopped working and changed something, and only with the
 * whole file list known: the user confirms exactly the files that arrive.
 */
export function canApply(state: ContestantState, stat: DiffStat | null): boolean {
  return (
    (state === "done" || state === "ended") &&
    stat !== null &&
    stat.files.length > 0 &&
    !stat.truncated
  );
}

/** File names that run code once the project is opened, built, installed or tested. */
const RUNS_CODE_NAMES: ReadonlySet<string> = new Set([
  ".envrc",
  "package.json",
  ".npmrc",
  ".yarnrc",
  ".yarnrc.yml",
  ".pnpmfile.cjs",
  "makefile",
  "justfile",
  "build.rs",
  "setup.py",
  "conftest.py",
  ".gitattributes",
  ".gitmodules",
  ".pre-commit-config.yaml",
  "lefthook.yml",
  ".lefthook.yml",
  ".mcp.json",
]);

/** Folders whose files an editor, CI, git or an agent CLI runs (tasks, workflows, hooks). */
const RUNS_CODE_FOLDERS: readonly string[] = [
  ".vscode/",
  ".idea/",
  ".devcontainer/",
  ".github/workflows/",
  ".husky/",
  ".githooks/",
  ".cargo/",
  ".claude/",
  ".codex/",
  ".gemini/",
];

/**
 * Whether a changed file can run code without being called by name: editor tasks, CI workflows,
 * git and package hooks, package scripts, build tool configs, agent settings. Paths as the
 * backend lists them (relative, `/`); compared without case.
 */
export function runsCode(path: string): boolean {
  const lower = path.toLowerCase();
  const name = lower.slice(lower.lastIndexOf("/") + 1);
  if (RUNS_CODE_NAMES.has(name)) return true;
  if (RUNS_CODE_FOLDERS.some((folder) => lower.startsWith(folder) || lower.includes(`/${folder}`)))
    return true;
  // `vite.config.ts`, `.eslintrc.cjs` and the like are programs the build runs.
  return /\.config\.[cm]?[jt]s$/.test(name) || /^\.[a-z]+rc\.[cm]?js$/.test(name);
}

/** Files listed in the confirmation before the rest is summed up. */
export const APPLY_PREVIEW_MAX = 8;

export interface ApplyPreview {
  /** Files that run code first, so they are never hidden in the rest. */
  shown: { path: string; runsCode: boolean }[];
  /** Files not listed. */
  more: number;
  runsCode: number;
}

/** The file list of the take-over confirmation. */
export function applyPreview(
  files: readonly FileStat[],
  max: number = APPLY_PREVIEW_MAX,
): ApplyPreview {
  const marked = files.map((file) => ({ path: file.path, runsCode: runsCode(file.path) }));
  const risky = marked.filter((file) => file.runsCode);
  const plain = marked.filter((file) => !file.runsCode);
  const shown = [...risky, ...plain.slice(0, Math.max(0, max - risky.length))];
  return { shown, more: marked.length - shown.length, runsCode: risky.length };
}

/** Start checks of the dialog; `null` when the arena can start. */
export type ArenaProblem = "task" | "agents" | "folder";

export function arenaProblem(
  prompt: string,
  agents: number,
  folder: string | null,
): ArenaProblem | null {
  if (prompt.trim() === "") return "task";
  if (agents < ARENA_MIN || agents > ARENA_MAX) return "agents";
  if (folder === null) return "folder";
  return null;
}

/** Contestants other than the winner whose sessions still exist (closing them is offered). */
export function others(
  arena: ArenaRun,
  winnerId: number,
  existing: ReadonlySet<number>,
): Contestant[] {
  return arena.contestants.filter((c) => c.sessionId !== winnerId && existing.has(c.sessionId));
}

/** Codes of `ArenaError` (arena.rs) with a sentence of their own. */
const ERROR_MESSAGES: ReadonlyMap<string, MessageKey> = new Map<string, MessageKey>([
  ["arena_no_commit", "arena.errorNoCommit"],
  ["arena_not_arena", "arena.errorNotArena"],
  ["arena_missing", "arena.errorMissing"],
  ["arena_nothing_to_apply", "arena.errorNothing"],
  ["arena_too_large", "arena.errorTooLarge"],
  ["arena_apply", "arena.errorApply"],
  ["arena_copy", "arena.errorCopy"],
  ["arena_unsafe_path", "arena.errorUnsafePath"],
  ["arena_link", "arena.errorLink"],
  ["arena_changed", "arena.errorChanged"],
  ["arena_remove", "arena.errorRemove"],
  ["arena_command", "arena.errorCommand"],
  ["arena_unsafe_batch", "arena.errorUnsafe"],
  ["arena_spawn", "arena.errorSpawn"],
]);

/** The backend's fixed text for `arena_program_not_found` (pinned by a test in arena.rs). */
const PROGRAM_NOT_FOUND = /^The program (.+) was not found$/;

/**
 * Clear text for a failed arena command, in the window's language. The backend answers
 * `[code] English text`; known codes and common git errors get their own sentence, anything
 * else a general one, so no English reaches the user.
 */
export function arenaErrorText(error: unknown, t: Translate): string {
  if (typeof error !== "string") return t("error.unknown");
  const split = splitBackendError(error);
  if (split === null) return knownWorktreeErrorText(error, t) ?? t("error.unknown");
  if (split.code === "arena_count") {
    return t("arena.errorCount", { min: ARENA_MIN, max: ARENA_MAX });
  }
  if (split.code === "arena_program_not_found") {
    const program = PROGRAM_NOT_FOUND.exec(split.text)?.[1];
    return program === undefined ? t("arena.errorNoProgram") : t("arena.errorProgram", { program });
  }
  const key = ERROR_MESSAGES.get(split.code);
  return key === undefined ? t("error.unknown") : t(key);
}
