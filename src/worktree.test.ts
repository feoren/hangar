import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import {
  canUseWorktree,
  createErrorText,
  insideFolder,
  orphanedWorktrees,
  parseRemoval,
  parseWorktree,
  removalErrorNotice,
  removalNotice,
  shortBranch,
  worktreeInUse,
  worktreeLabel,
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

describe("canUseWorktree", () => {
  it("needs a branch for the folder: inside a repository", () => {
    const branches = new Map<string, string | null>([
      ["C:\\src\\app", "main"],
      ["C:\\Users\\Ada", null],
    ]);
    expect(canUseWorktree(branches, "C:\\src\\app")).toBe(true);
    expect(canUseWorktree(branches, "C:\\Users\\Ada")).toBe(false);
    // Answer still pending: off until known.
    expect(canUseWorktree(branches, "D:\\other")).toBe(false);
    expect(canUseWorktree(branches, null)).toBe(false);
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

  it("refuses malformed answers", () => {
    expect(parseRemoval({ removed: "yes", branchDeleted: true })).toBeNull();
    expect(parseRemoval({ removed: true, branchDeleted: true, reason: 3 })).toBeNull();
    expect(parseRemoval(undefined)).toBeNull();
  });
});

describe("removalNotice", () => {
  it("says nothing when folder and branch are gone", () => {
    expect(removalNotice({ removed: true, branchDeleted: true, reason: null }, worktree, en)).toBe(
      null,
    );
  });

  it("names git's reason and the folder when the worktree stays", () => {
    const reason = "fatal: unable to delete the folder: Permission denied";
    const notice = removalNotice({ removed: false, branchDeleted: false, reason }, worktree, en);
    expect(notice).toEqual({
      message: en("worktree.keptBecause", { reason }),
      path: worktree.path,
    });
    expect(
      removalNotice({ removed: false, branchDeleted: false, reason: " " }, worktree, en)?.message,
    ).toBe(en("worktree.kept"));
  });

  it("translates the everyday reasons instead of showing git's English", () => {
    const de = createTranslator("de");
    const kept = (reason: string) =>
      removalNotice({ removed: false, branchDeleted: false, reason }, worktree, de);
    expect(
      kept(
        `fatal: '${worktree.path}' contains modified or untracked files, use --force to delete it`,
      ),
    ).toEqual({ message: de("worktree.keptChanges"), path: worktree.path });
    expect(kept("The worktree contains ignored files: target, debug.log and 3 more")).toEqual({
      message: de("worktree.keptIgnored"),
      path: worktree.path,
    });
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
