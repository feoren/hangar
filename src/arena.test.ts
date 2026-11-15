import { describe, expect, it } from "vitest";

import {
  applyPreview,
  arenaErrorText,
  arenaProblem,
  canApply,
  clockText,
  contestantState,
  elapsedMs,
  leader,
  newContestant,
  others,
  parseApplyOutcome,
  parseArena,
  parseDiffStat,
  parseTestRun,
  promptReady,
  PROMPT_MAX_WAIT_MS,
  runsCode,
  testsDue,
  trackAttention,
  type ArenaRun,
  type DiffStat,
  type Standing,
} from "./arena";
import { createTranslator } from "./i18n";

const SHA = "a".repeat(40);
const worktree = (agent: string) => ({
  path: `C:\\data\\worktrees\\repo-1234abcd\\arena-1a2b3c-${agent}`,
  branch: `hangar/arena-1a2b3c-${agent}`,
  repoRoot: "C:\\src\\repo",
});

const stat = (files: number, added = 1, removed = 0): DiffStat => ({
  files: Array.from({ length: files }, (_, i) => ({ path: `f${String(i)}`, added, removed })),
  added: added * files,
  removed: removed * files,
  truncated: false,
});

describe("arena answers", () => {
  it("accepts the backend's arena and refuses foreign or broken ones", () => {
    const good = {
      id: "1a2b3c",
      baseCommit: SHA,
      repoRoot: "C:\\src\\repo",
      worktrees: [worktree("claude"), worktree("codex")],
    };
    expect(parseArena(good)?.worktrees).toHaveLength(2);
    expect(parseArena({ ...good, id: "XYZ" })).toBeNull();
    expect(parseArena({ ...good, baseCommit: "HEAD" })).toBeNull();
    expect(parseArena({ ...good, worktrees: [worktree("claude")] })).toBeNull();
    const foreign = { ...worktree("x"), branch: "hangar/fix-1a2b3c" };
    expect(parseArena({ ...good, worktrees: [worktree("claude"), foreign] })).toBeNull();
    expect(parseArena(null)).toBeNull();
  });

  it("checks diff stats, apply outcomes and test runs", () => {
    expect(parseDiffStat(stat(2))).toEqual(stat(2));
    expect(parseDiffStat({ ...stat(1), added: -1 })).toBeNull();
    expect(
      parseDiffStat({ files: [{ path: 3 }], added: 0, removed: 0, truncated: false }),
    ).toBeNull();
    expect(parseApplyOutcome({ kind: "applied", files: ["a"] })).toEqual({
      kind: "applied",
      files: ["a"],
    });
    expect(parseApplyOutcome({ kind: "conflict", files: ["a", "b"] })?.kind).toBe("conflict");
    expect(parseApplyOutcome({ kind: "other", files: [] })).toBeNull();
    expect(parseApplyOutcome({ kind: "applied", files: [1] })).toBeNull();
    const run = { passed: false, exitCode: null, timedOut: true, durationMs: 5, output: "" };
    expect(parseTestRun(run)).toEqual(run);
    expect(parseTestRun({ ...run, exitCode: "1" })).toBeNull();
  });
});

describe("contestants", () => {
  const base = newContestant(7, "claude", "Claude Code", worktree("claude"));

  it("is starting until the task is sent, then working, done after working, ended on exit", () => {
    expect(contestantState(base, "idle", false)).toBe("starting");
    const sent = { ...base, promptSent: true };
    // Right after the task, before any output: not done yet.
    expect(contestantState(sent, "idle", false)).toBe("working");
    expect(contestantState(sent, "working", false)).toBe("working");
    expect(contestantState({ ...sent, sawWorking: true }, "attention", false)).toBe("done");
    expect(contestantState({ ...sent, sawWorking: true }, "working", true)).toBe("ended");
  });

  it("tracks the finish time across working phases", () => {
    // The start screen before the task does not count.
    expect(trackAttention(base, "working", 10)).toBe(base);
    let c = { ...base, promptSent: true };
    expect(trackAttention(c, "idle", 10)).toBe(c);
    c = trackAttention(c, "working", 20);
    expect(c.sawWorking).toBe(true);
    expect(trackAttention(c, "working", 25)).toBe(c);
    c = trackAttention(c, "attention", 30);
    expect(c.doneAt).toBe(30);
    expect(trackAttention(c, "idle", 40)).toBe(c);
    c = trackAttention(c, "working", 50);
    expect(c.doneAt).toBeNull();
  });

  it("measures from the arena start to the finish", () => {
    const arena: ArenaRun = {
      id: "1a2b3c",
      baseCommit: SHA,
      repoRoot: "C:\\src\\repo",
      prompt: "x",
      testCommand: "",
      startedAt: 1000,
      contestants: [base],
    };
    expect(elapsedMs(arena, base, 4000)).toBe(3000);
    expect(elapsedMs(arena, { ...base, doneAt: 2500 }, 9000)).toBe(1500);
    expect(others(arena, 7, new Set([7]))).toEqual([]);
    const second = { ...base, sessionId: 8 };
    const two = { ...arena, contestants: [base, second] };
    expect(others(two, 7, new Set([7, 8]))).toEqual([second]);
    expect(others(two, 7, new Set([7]))).toEqual([]);
  });

  it("reads times like a stopwatch", () => {
    expect(clockText(-5)).toBe("0:00");
    expect(clockText(42_900)).toBe("0:42");
    expect(clockText(245_000)).toBe("4:05");
    expect(clockText(3_723_000)).toBe("1:02:03");
  });

  it("sends the task once the start screen is quiet, never too early or too late", () => {
    expect(promptReady(0, null, 2000)).toBe(false);
    expect(promptReady(0, 500, 2000)).toBe(false);
    expect(promptReady(0, null, 10000)).toBe(false);
    expect(promptReady(0, 9000, 10000)).toBe(false);
    expect(promptReady(0, 7000, 10000)).toBe(true);
    expect(promptReady(0, PROMPT_MAX_WAIT_MS - 1, PROMPT_MAX_WAIT_MS)).toBe(true);
  });

  it("runs the tests once per finish", () => {
    const done = { ...base, promptSent: true, sawWorking: true, doneAt: 30 };
    expect(testsDue(done, "done", "pnpm test")).toBe(true);
    expect(testsDue(done, "done", "  ")).toBe(false);
    expect(testsDue(done, "working", "pnpm test")).toBe(false);
    expect(testsDue({ ...done, testedFor: 30 }, "done", "pnpm test")).toBe(false);
    expect(testsDue({ ...done, testedFor: 10 }, "done", "pnpm test")).toBe(true);
    expect(testsDue({ ...done, tests: { kind: "running" } }, "done", "pnpm test")).toBe(false);
  });
});

describe("comparison", () => {
  const passed = {
    kind: "done",
    run: { passed: true, exitCode: 0, timedOut: false, durationMs: 1, output: "" },
  } as const;
  const failed = {
    kind: "done",
    run: { passed: false, exitCode: 1, timedOut: false, durationMs: 1, output: "" },
  } as const;
  const standing = (over: Partial<Standing>): Standing => ({
    sessionId: 1,
    state: "done",
    elapsedMs: 1000,
    stat: stat(1),
    tests: { kind: "none" },
    ...over,
  });

  it("leads with passed tests, then the quicker, then the smaller change", () => {
    expect(leader([])).toBeNull();
    expect(
      leader([
        standing({ sessionId: 1, tests: failed, elapsedMs: 10 }),
        standing({ sessionId: 2, tests: passed, elapsedMs: 900 }),
        standing({ sessionId: 3, elapsedMs: 20 }),
      ]),
    ).toBe(2);
    expect(
      leader([
        standing({ sessionId: 1, elapsedMs: 500 }),
        standing({ sessionId: 2, elapsedMs: 400 }),
      ]),
    ).toBe(2);
    expect(
      leader([
        standing({ sessionId: 1, stat: stat(3) }),
        standing({ sessionId: 2, stat: stat(1) }),
      ]),
    ).toBe(2);
  });

  it("only finished contestants with changes count", () => {
    expect(
      leader([
        standing({ sessionId: 1, state: "working" }),
        standing({ sessionId: 2, stat: stat(0) }),
        standing({ sessionId: 3, stat: null }),
      ]),
    ).toBeNull();
    expect(canApply("done", stat(1))).toBe(true);
    expect(canApply("ended", stat(1))).toBe(true);
    expect(canApply("working", stat(1))).toBe(false);
    expect(canApply("done", stat(0))).toBe(false);
    expect(canApply("done", null)).toBe(false);
    // A cut list cannot be confirmed file by file.
    expect(canApply("done", { ...stat(3), truncated: true })).toBe(false);
  });

  it("names what is missing before the start", () => {
    expect(arenaProblem(" ", 2, "C:\\src")).toBe("task");
    expect(arenaProblem("fix it", 1, "C:\\src")).toBe("agents");
    expect(arenaProblem("fix it", 5, "C:\\src")).toBe("agents");
    expect(arenaProblem("fix it", 2, null)).toBe("folder");
    expect(arenaProblem("fix it", 4, "C:\\src")).toBeNull();
  });
});

describe("arenaErrorText", () => {
  const de = createTranslator("de");

  it("translates every code of the backend", () => {
    const cases: [string, string][] = [
      ["[arena_count] An arena needs 2 to 4 agents", de("arena.errorCount", { min: 2, max: 4 })],
      ["[arena_no_commit] The repository has no commit yet", de("arena.errorNoCommit")],
      ["[arena_not_arena] This worktree does not belong to an arena", de("arena.errorNotArena")],
      ["[arena_missing] The contestant's worktree no longer exists", de("arena.errorMissing")],
      ["[arena_nothing_to_apply] The contestant changed nothing", de("arena.errorNothing")],
      ["[arena_too_large] The changes are too large to take over", de("arena.errorTooLarge")],
      ["[arena_apply] git could not apply the changes: patch failed", de("arena.errorApply")],
      ["[arena_copy] a.txt cannot be taken over: outside", de("arena.errorCopy")],
      ["[arena_unsafe_path] .git./x cannot be taken over", de("arena.errorUnsafePath")],
      ["[arena_link] lnk is a link or a submodule", de("arena.errorLink")],
      [
        "[arena_changed] The changes are no longer the ones that were shown",
        de("arena.errorChanged"),
      ],
      ["[arena_remove] The worktree could not be deleted: in use", de("arena.errorRemove")],
      ["[arena_command] The test command is not usable: empty", de("arena.errorCommand")],
      ["[arena_unsafe_batch] Argument 1 contains '&'", de("arena.errorUnsafe")],
      ["[arena_spawn] The test command could not be started: denied", de("arena.errorSpawn")],
    ];
    for (const [message, text] of cases) expect(arenaErrorText(message, de)).toBe(text);
    expect(de("arena.errorCount", { min: 2, max: 4 })).toBe("Eine Arena braucht 2 bis 4 Agenten.");
  });

  it("names the missing program", () => {
    expect(arenaErrorText("[arena_program_not_found] The program pnpm was not found", de)).toBe(
      "pnpm wurde nicht gefunden. Ist es installiert und im PATH?",
    );
    expect(arenaErrorText("[arena_program_not_found] Something else", de)).toBe(
      de("arena.errorNoProgram"),
    );
  });

  it("translates common git errors passed through", () => {
    expect(arenaErrorText("git was not found on PATH", de)).toBe(de("worktree.noGit"));
    expect(arenaErrorText("This folder is not inside a git repository", de)).toBe(
      de("worktree.notRepo"),
    );
  });

  it("never shows English for anything else", () => {
    const unknown = de("error.unknown");
    expect(arenaErrorText("[arena_later] A new kind", de)).toBe(unknown);
    expect(arenaErrorText("fatal: bad object HEAD", de)).toBe(unknown);
    expect(arenaErrorText(new Error("invoke failed"), de)).toBe(unknown);
    expect(arenaErrorText(null, de)).toBe(unknown);
  });
});

describe("take-over confirmation", () => {
  it("marks files that can run code", () => {
    for (const path of [
      ".envrc",
      "package.json",
      "web/package.json",
      ".vscode/tasks.json",
      ".VSCode/settings.json",
      ".github/workflows/ci.yml",
      ".husky/pre-commit",
      ".claude/settings.json",
      "packages/app/.vscode/launch.json",
      "vite.config.ts",
      "tools/eslint.config.mjs",
      ".eslintrc.cjs",
      "Makefile",
      "build.rs",
      "tests/conftest.py",
      ".gitattributes",
    ]) {
      expect(runsCode(path), path).toBe(true);
    }
    for (const path of [
      "src/main.ts",
      "README.md",
      "src/hooks/useThing.ts",
      ".github/ISSUE_TEMPLATE/bug.md",
      "docs/package.json.md",
      "config.ts",
      "my.vscode/x",
    ]) {
      expect(runsCode(path), path).toBe(false);
    }
  });

  it("lists files that run code first and never hides them", () => {
    const files = ["a.ts", "b.ts", "c.ts", ".envrc", "d.ts", ".vscode/tasks.json"].map((path) => ({
      path,
      added: 1,
      removed: 0,
    }));
    const preview = applyPreview(files, 3);
    expect(preview.shown).toEqual([
      { path: ".envrc", runsCode: true },
      { path: ".vscode/tasks.json", runsCode: true },
      { path: "a.ts", runsCode: false },
    ]);
    expect(preview.more).toBe(3);
    expect(preview.runsCode).toBe(2);
    const tight = applyPreview(files, 1);
    expect(tight.shown.map((file) => file.path)).toEqual([".envrc", ".vscode/tasks.json"]);
    expect(tight.more).toBe(4);
    const all = applyPreview(files.slice(0, 2));
    expect(all.more).toBe(0);
    expect(all.runsCode).toBe(0);
  });
});
