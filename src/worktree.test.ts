import { describe, expect, it } from "vitest";

import { createTranslator, LANGUAGES } from "./i18n";
import {
  createErrorText,
  insideFolder,
  keptMessage,
  orphanedWorktrees,
  parseKept,
  parseRemoval,
  parseWorktree,
  removalErrorNotice,
  removalNotice,
  shortBranch,
  worktreeInUse,
  worktreeLabel,
  worktreeTitle,
} from "./worktree";

const en = createTranslator("en");
const worktree = {
  path: "C:\\Users\\Ada\\AppData\\Local\\hangar\\worktrees\\app-1a2b3c4d\\claude-code-a1b2c3",
  branch: "hangar/claude-code-a1b2c3",
  repoRoot: "C:\\src\\app",
};

describe("worktreeLabel", () => {
  it("is the program name, never empty", () => {
    expect(worktreeLabel(" Claude Code ")).toBe("Claude Code");
    expect(worktreeLabel("  ")).toBe("session");
  });
});

describe("shortBranch", () => {
  it("drops Hangar's prefix only", () => {
    expect(shortBranch("hangar/claude-code-a1b2c3")).toBe("claude-code-a1b2c3");
    expect(shortBranch("main")).toBe("main");
    expect(shortBranch("hangar/")).toBe("hangar/");
  });
});

describe("parseWorktree", () => {
  it("accepts what the backend creates", () => {
    expect(parseWorktree(worktree)).toEqual(worktree);
  });

  it("refuses branches Hangar did not create and missing fields", () => {
    expect(parseWorktree({ ...worktree, branch: "main" })).toBeNull();
    expect(parseWorktree({ ...worktree, branch: "hangar/" })).toBeNull();
    expect(parseWorktree({ ...worktree, path: "" })).toBeNull();
    expect(parseWorktree({ path: worktree.path })).toBeNull();
    expect(parseWorktree(null)).toBeNull();
    expect(parseWorktree("x")).toBeNull();
  });
});

describe("parseRemoval", () => {
  it("reads the answer and treats a missing reason as none", () => {
    expect(parseRemoval({ removed: true, branchDeleted: true, reason: null })).toEqual({
      removed: true,
      branchDeleted: true,
      reason: null,
    });
    expect(parseRemoval({ removed: false, branchDeleted: false })).toEqual({
      removed: false,
      branchDeleted: false,
      reason: null,
    });
  });

  it("reads the kept reason", () => {
    expect(
      parseRemoval({
        removed: false,
        branchDeleted: false,
        reason: { cause: "gitFailed", detail: " fatal: boom " },
      }),
    ).toEqual({
      removed: false,
      branchDeleted: false,
      reason: { cause: "gitFailed", detail: "fatal: boom" },
    });
  });

  it("refuses malformed answers", () => {
    expect(parseRemoval({ removed: "yes", branchDeleted: true })).toBeNull();
    expect(parseRemoval({ removed: true, branchDeleted: true, reason: 3 })).toBeNull();
    // Reasons were plain texts before; a text is no reason any more.
    expect(parseRemoval({ removed: false, branchDeleted: false, reason: "locked" })).toBeNull();
    expect(parseRemoval(undefined)).toBeNull();
  });
});

describe("parseKept", () => {
  it("keeps a cause this window does not know as unknown", () => {
    expect(parseKept({ cause: "somethingNew", detail: "x" })).toEqual({
      cause: null,
      detail: "x",
    });
    expect(parseKept({ cause: "partly" })).toEqual({ cause: "partly", detail: null });
    expect(parseKept(null)).toBeNull();
    expect(parseKept({ cause: 3, detail: null })).toBeUndefined();
    expect(parseKept({ cause: "partly", detail: 3 })).toBeUndefined();
  });
});

describe("keptMessage", () => {
  const causes = [
    ["changes", "worktree.keptChanges"],
    ["ignored", "worktree.keptIgnored"],
    ["partly", "worktree.keptPartly"],
    ["unsafeFilters", "worktree.keptUnsafeFilters"],
    ["notCheckedInTime", "worktree.keptNotCheckedInTime"],
  ] as const;

  it("words every cause in every language", () => {
    for (const language of LANGUAGES) {
      const t = createTranslator(language);
      for (const [cause, key] of causes) {
        const text = keptMessage({ cause, detail: null }, t);
        expect(text).toBe(t(key));
        expect(text).not.toBe(key);
      }
    }
  });

  it("passes only git's own message through", () => {
    expect(keptMessage({ cause: "gitFailed", detail: "fatal: boom" }, en)).toBe(
      en("worktree.keptBecause", { reason: "fatal: boom" }),
    );
    expect(keptMessage({ cause: "gitFailed", detail: null }, en)).toBe(en("worktree.kept"));
    expect(keptMessage({ cause: "changes", detail: "ignored detail" }, en)).toBe(
      en("worktree.keptChanges"),
    );
  });

  it("falls back to the general sentence for no or an unknown cause", () => {
    const de = createTranslator("de");
    expect(keptMessage(null, de)).toBe(de("worktree.kept"));
    expect(keptMessage(parseKept({ cause: "somethingNew", detail: "x" }) ?? null, de)).toBe(
      de("worktree.kept"),
    );
  });
});

describe("removalNotice", () => {
  it("says nothing when folder and branch are gone", () => {
    expect(removalNotice({ removed: true, branchDeleted: true, reason: null }, worktree, en)).toBe(
      null,
    );
  });

  it("names git's reason and the folder when the worktree stays", () => {
    const detail = "fatal: unable to delete the folder: Permission denied";
    const notice = removalNotice(
      { removed: false, branchDeleted: false, reason: { cause: "gitFailed", detail } },
      worktree,
      en,
    );
    expect(notice).toEqual({
      message: en("worktree.keptBecause", { reason: detail }),
      path: worktree.path,
    });
    expect(
      removalNotice({ removed: false, branchDeleted: false, reason: null }, worktree, en)?.message,
    ).toBe(en("worktree.kept"));
  });

  it("translates the everyday reasons", () => {
    const de = createTranslator("de");
    expect(
      removalNotice(
        { removed: false, branchDeleted: false, reason: { cause: "changes", detail: null } },
        worktree,
        de,
      ),
    ).toEqual({ message: de("worktree.keptChanges"), path: worktree.path });
  });

  it("names the branch when only the branch stays", () => {
    expect(
      removalNotice({ removed: true, branchDeleted: false, reason: null }, worktree, en),
    ).toEqual({ message: en("worktree.branchKept"), path: worktree.branch });
  });

  it("keeps the error text and the folder when removing failed", () => {
    expect(removalErrorNotice("git did not finish in time", worktree, en)).toEqual({
      message: en("worktree.removeFailed", { error: "git did not finish in time" }),
      path: worktree.path,
    });
  });
});

describe("createErrorText", () => {
  it("translates the common cases", () => {
    expect(createErrorText("git was not found on PATH", en)).toBe(en("worktree.noGit"));
    expect(createErrorText("This folder is not inside a git repository", en)).toBe(
      en("worktree.notRepo"),
    );
    expect(
      createErrorText(
        "The repository's git folder lies on another network share or device; Hangar does not run git there",
        en,
      ),
    ).toBe(en("worktree.share"));
  });

  it("keeps git's words otherwise", () => {
    expect(createErrorText("fatal: invalid reference: HEAD", en)).toBe(
      en("worktree.createFailed", { error: "fatal: invalid reference: HEAD" }),
    );
  });
});

describe("parseWorktree and hidden characters", () => {
  it("refuses bidi, zero-width and control characters in any field", () => {
    expect(parseWorktree({ ...worktree, branch: "hangar/x\u202e" })).toBeNull();
    expect(parseWorktree({ ...worktree, path: `${worktree.path}\u200b` })).toBeNull();
    expect(parseWorktree({ ...worktree, repoRoot: "C:/src\u0007" })).toBeNull();
  });
});

describe("insideFolder and worktreeInUse", () => {
  const wt = {
    path: "C:/Data/worktrees/app-1/claude-1",
    branch: "hangar/claude-1",
    repoRoot: "C:/src/app",
  };
  const session = (id: number, cwd: string, status = "running") => ({ id, cwd, status });

  it("compares like the platform: case and separators on Windows", () => {
    expect(insideFolder("c:\\data\\worktrees\\app-1\\claude-1\\src", wt.path)).toBe(true);
    expect(insideFolder("C:/Data/worktrees/app-1/claude-1", wt.path)).toBe(true);
    expect(insideFolder("C:/Data/worktrees/app-1/claude-10", wt.path)).toBe(false);
    expect(insideFolder("/home/a/wt/x", "/home/a/wt")).toBe(true);
    expect(insideFolder("/home/a/wtx", "/home/a/wt")).toBe(false);
  });

  it("sees another running session inside the worktree, never the closing one", () => {
    expect(worktreeInUse(wt, [session(1, wt.path)], 1)).toBe(false);
    expect(worktreeInUse(wt, [session(1, wt.path), session(2, `${wt.path}/sub`)], 1)).toBe(true);
    expect(worktreeInUse(wt, [session(1, wt.path), session(2, wt.path, "exited")], 1)).toBe(false);
    expect(worktreeInUse(wt, [session(2, "C:/src/app")], 1)).toBe(false);
  });
});

describe("orphanedWorktrees", () => {
  const wt = (name: string) => ({
    path: `/data/worktrees/app-1/${name}`,
    branch: `hangar/${name}`,
    repoRoot: "/src/app",
  });

  it("lists each worktree once, only when sessions are not remembered", () => {
    const sessions = [
      { id: 1, cwd: "/a", status: "running", worktree: wt("a") },
      { id: 2, cwd: "/b", status: "running" },
      { id: -3, cwd: "/c", status: "exited", worktree: wt("c") },
      { id: 4, cwd: "/a", status: "running", worktree: wt("a") },
      { id: 5, cwd: "/data/worktrees/app-1/a/src", status: "running" },
    ];
    expect(orphanedWorktrees(sessions, true)).toEqual([]);
    expect(orphanedWorktrees(sessions, false)).toEqual([
      { ids: [1, 4, 5], worktree: wt("a") },
      { ids: [-3], worktree: wt("c") },
    ]);
  });
});

describe("worktreeTitle", () => {
  it("adds the branch suffix to the program name", () => {
    expect(worktreeTitle("Claude Code", "hangar/claude-code-1a2b3c")).toBe("Claude Code · 1a2b3c");
    expect(worktreeTitle("Codex", "hangar/x")).toBe("Codex · x");
  });
});
