import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import {
  parseState,
  stateHint,
  cleanupFolders,
  cleanupResult,
  entryAction,
  groupByRepository,
  lastChange,
  outcomeNotice,
  parseCleanupEntries,
  parseCleanupOutcome,
  parseWorktreeSize,
  safeEntries,
  sizeText,
  stateLabel,
  type CleanupEntry,
} from "./worktreeCleanup";

const t = createTranslator("en");

function entry(overrides: Partial<CleanupEntry> = {}): CleanupEntry {
  return {
    path: "C:\\data\\worktrees\\repo-0123abcd\\fix-1a2b3c",
    repoRoot: "C:\\code\\repo",
    branch: "hangar/fix-1a2b3c",
    modified: 1000,
    state: { kind: "merged" },
    ...overrides,
  };
}

describe("parsing the backend's answers", () => {
  it("takes well-formed entries and drops malformed ones", () => {
    const parsed = parseCleanupEntries([
      {
        path: "C:\\w\\r-0123abcd\\a-1a2b3c",
        repoRoot: "C:\\r",
        branch: "hangar/a-1a2b3c",
        modified: 5,
        state: { kind: "clean" },
      },
      {
        path: "C:\\w\\r-0123abcd\\b-1a2b3c",
        repoRoot: null,
        branch: "hangar/b-1a2b3c",
        modified: null,
        state: { kind: "unknown", cause: "repoNotFound", detail: null },
      },
      { path: "x", repoRoot: null, branch: "main", modified: null, state: { kind: "clean" } },
      { path: "y", repoRoot: null, branch: "hangar/y", modified: null, state: { kind: "boom" } },
      "nonsense",
    ]);
    expect(parsed).toHaveLength(2);
    expect(parsed?.[1]?.state).toEqual({ kind: "unknown", cause: "repoNotFound", detail: null });
    expect(parsed?.[1]?.repoRoot).toBeNull();
    expect(parseCleanupEntries({})).toBeNull();
  });

  it("checks sizes and outcomes", () => {
    expect(parseWorktreeSize({ bytes: 10, files: 2, complete: false, newest: null })).toEqual({
      bytes: 10,
      files: 2,
      complete: false,
      newest: null,
    });
    expect(parseWorktreeSize({ bytes: -1, files: 2, complete: true, newest: null })).toBeNull();
    expect(
      parseCleanupOutcome({
        done: false,
        branchDeleted: false,
        state: { kind: "changes" },
        reason: null,
      }),
    ).toEqual({ done: false, branchDeleted: false, state: { kind: "changes" }, reason: null });
    expect(
      parseCleanupOutcome({
        done: false,
        branchDeleted: false,
        state: { kind: "merged" },
        reason: { cause: "ignored", detail: null },
      })?.reason,
    ).toEqual({ cause: "ignored", detail: null });
    expect(
      parseCleanupOutcome({
        done: false,
        branchDeleted: false,
        state: { kind: "merged" },
        reason: "locked",
      }),
    ).toBeNull();
    expect(parseCleanupOutcome({ done: true, branchDeleted: true, state: {} })).toBeNull();
  });
});

describe("actions", () => {
  it("offers removal only for safe entries and reveal for the ones with content", () => {
    expect(entryAction(entry())).toBe("remove");
    expect(entryAction(entry({ state: { kind: "clean" } }))).toBe("remove");
    expect(entryAction(entry({ state: { kind: "missing" } }))).toBe("prune");
    expect(entryAction(entry({ state: { kind: "changes" } }))).toBe("reveal");
    expect(entryAction(entry({ state: { kind: "ignored" } }))).toBe("reveal");
    expect(entryAction(entry({ state: { kind: "inUse" } }))).toBe("reveal");
    expect(entryAction(entry({ state: { kind: "unknown", cause: "locked", detail: null } }))).toBe(
      "reveal",
    );
    // Without a repository nothing can be removed; a missing folder cannot be shown.
    expect(entryAction(entry({ repoRoot: null }))).toBe("reveal");
    expect(entryAction(entry({ repoRoot: null, state: { kind: "missing" } }))).toBeNull();
  });

  it("collects the safe entries for the bulk action", () => {
    const entries = [
      entry({ path: "a" }),
      entry({ path: "b", state: { kind: "changes" } }),
      entry({ path: "c", state: { kind: "missing" } }),
      entry({ path: "d", state: { kind: "inUse" } }),
      entry({ path: "e", state: { kind: "clean" } }),
    ];
    expect(safeEntries(entries).map((e) => e.path)).toEqual(["a", "c", "e"]);
  });
});

describe("folders for the backend", () => {
  it("joins recent folders, favorites and session folders once, bounded", () => {
    const folders = cleanupFolders({
      favorites: ["C:\\Code\\A"],
      recent: ["c:\\code\\a\\", "C:\\code\\b"],
      sessions: [
        { cwd: "C:\\code\\c", worktree: { path: "C:\\w", branch: "hangar/x", repoRoot: "C:\\r" } },
        { cwd: "C:\\code\\b" },
      ],
    });
    expect(folders).toEqual(["C:\\r", "C:\\code\\c", "C:\\code\\b", "C:\\Code\\A"]);
    const many = Array.from({ length: 100 }, (_, i) => `/p/${String(i)}`);
    expect(cleanupFolders({ favorites: many, recent: [], sessions: [] })).toHaveLength(64);
  });
});

describe("texts", () => {
  it("names every state", () => {
    for (const kind of ["inUse", "missing", "changes", "ignored", "merged", "clean"] as const) {
      expect(stateLabel({ kind }, t)).not.toBe("");
    }
    expect(stateLabel({ kind: "unknown", cause: "locked", detail: null }, t)).not.toBe("");
  });

  it("marks a size that was cut off", () => {
    expect(sizeText(undefined, t, "en")).toBe("");
    expect(sizeText({ bytes: 2048, files: 1, complete: true, newest: null }, t, "en")).toBe(
      "2 KiB",
    );
    expect(sizeText({ bytes: 2048, files: 1, complete: false, newest: null }, t, "en")).toBe(
      "≥ 2 KiB",
    );
  });

  it("takes the newest of the listing and the size walk", () => {
    expect(lastChange(entry({ modified: 5 }), undefined)).toBe(5);
    expect(
      lastChange(entry({ modified: 5 }), { bytes: 0, files: 0, complete: true, newest: 9 }),
    ).toBe(9);
    expect(lastChange(entry({ modified: null }), undefined)).toBeNull();
  });

  it("tells what happened", () => {
    expect(
      outcomeNotice(
        { done: true, branchDeleted: true, state: { kind: "merged" }, reason: null },
        t,
      ),
    ).toBeNull();
    expect(
      outcomeNotice(
        { done: true, branchDeleted: false, state: { kind: "clean" }, reason: null },
        t,
      ),
    ).toBe(t("cleanup.branchKept"));
    expect(
      outcomeNotice(
        { done: false, branchDeleted: false, state: { kind: "changes" }, reason: null },
        t,
      ),
    ).toBe(t("cleanup.changedMeanwhile"));
    const kept = (cause: "gitFailed" | "partly" | null, detail: string | null) =>
      outcomeNotice(
        {
          done: false,
          branchDeleted: false,
          state: { kind: "merged" },
          reason: { cause, detail },
        },
        t,
      );
    expect(kept("gitFailed", "locked")).toBe(t("worktree.keptBecause", { reason: "locked" }));
    expect(kept("partly", null)).toBe(t("worktree.keptPartly"));
    expect(kept(null, "x")).toBe(t("worktree.kept"));
  });
});

describe("grouping", () => {
  it("groups by repository, the unknown repository last", () => {
    const groups = groupByRepository([
      entry({ path: "1", repoRoot: "C:\\code\\b" }),
      entry({ path: "2", repoRoot: null }),
      entry({ path: "3", repoRoot: "C:\\code\\a" }),
      entry({ path: "4", repoRoot: "c:\\code\\B" }),
    ]);
    expect(groups.map((g) => [g.name, g.entries.map((e) => e.path)])).toEqual([
      ["a", ["3"]],
      ["b", ["1", "4"]],
      [null, ["2"]],
    ]);
  });
});

describe("removal results", () => {
  it("drops the row when done and keeps it with the fresh state when refused", () => {
    const row = entry();
    expect(
      cleanupResult(row, { done: true, branchDeleted: true, state: { kind: "merged" } }, t),
    ).toEqual({ done: true, notice: null });
    expect(
      cleanupResult(
        row,
        { done: false, branchDeleted: false, state: { kind: "changes" }, reason: null },
        t,
      ),
    ).toEqual({
      done: false,
      entry: { ...row, state: { kind: "changes" } },
      notice: t("cleanup.changedMeanwhile"),
    });
    const broken = cleanupResult(row, "nonsense", t);
    expect(broken.done).toBe(false);
    if (!broken.done) expect(broken.entry).toBe(row);
  });
});

describe("causes", () => {
  const causes = [
    ["repoNotFound", "cleanup.causeRepoNotFound"],
    ["notKnownToGit", "cleanup.causeNotKnownToGit"],
    ["otherBranch", "cleanup.causeOtherBranch"],
    ["noBranch", "cleanup.causeNoBranch"],
    ["locked", "cleanup.causeLocked"],
    ["notCheckedInTime", "cleanup.causeNotCheckedInTime"],
    ["remoteRepo", "cleanup.causeRemoteRepo"],
    ["gitFailed", "cleanup.causeGitFailedPlain"],
  ] as const;

  it("parses every cause and words it without the backend's text", () => {
    for (const [cause, key] of causes) {
      const state = parseState({ kind: "unknown", cause, detail: null });
      expect(state).toEqual({ kind: "unknown", cause, detail: null });
      if (state !== null) expect(stateHint(state, t)).toBe(t(key));
    }
    // A branch name is a detail, but only git's message is ever shown.
    const other = parseState({ kind: "unknown", cause: "otherBranch", detail: "main" });
    if (other !== null) expect(stateHint(other, t)).toBe(t("cleanup.causeOtherBranch"));
    const failed = parseState({ kind: "unknown", cause: "gitFailed", detail: "fatal: bad object" });
    if (failed !== null) {
      expect(stateHint(failed, t)).toBe(
        t("cleanup.causeGitFailed", { detail: "fatal: bad object" }),
      );
    }
  });

  it("refuses unknown causes and old reason texts", () => {
    expect(parseState({ kind: "unknown", cause: "boom", detail: null })).toBeNull();
    expect(parseState({ kind: "unknown", reason: "The repository was not found" })).toBeNull();
    expect(parseState({ kind: "unknown", cause: "locked", detail: 5 })).toBeNull();
  });
});
