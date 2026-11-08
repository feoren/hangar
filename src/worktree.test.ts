import { describe, expect, it } from "vitest";

import { createTranslator } from "./i18n";
import {
  canUseWorktree,
  createErrorText,
  parseRemoval,
  parseWorktree,
  removalErrorNotice,
  removalNotice,
  shortBranch,
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
