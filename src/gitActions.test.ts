import { describe, expect, it } from "vitest";

import {
  commitMessage,
  commitMode,
  diffKey,
  discardDeletes,
  GIT_MAX_PATHS,
  isConflict,
  lineComment,
  newLineNumbers,
  parseCommitOutcome,
  pathBatches,
  stageAction,
  stageAllBox,
  stageAllPaths,
  stageState,
} from "./gitActions";
import { unquotePath } from "./gitDiff";

// eslint-disable-next-line no-control-regex -- control characters are exactly the target here.
const SPECIAL = /[\u0000-\u001f\u007f-\u009f\u2028\u2029\u202a-\u202e\u2066-\u2069]/;

describe("stage all", () => {
  it("stages tracked changes only, as a commit with -a does, never a merge conflict", () => {
    const files = [file(" M", "a"), file(" D", "b"), file("??", "c"), file("UU", "d")];
    expect(stageAllPaths(files)).toEqual(["a", "b"]);
    expect(commitMode([file("??", "new")])).toBe("none");
    expect(commitMode([file("??", "new"), file(" M", "a")])).toBe("stageAll");
  });
});

describe("stageAllBox", () => {
  it("is empty and stages everything, untracked files too, while nothing is staged", () => {
    expect(stageAllBox([file(" M", "a"), file("??", "b")])).toEqual({
      state: "unstaged",
      action: "stage",
      paths: ["a", "b"],
      conflicts: 0,
    });
  });

  it("leaves files in a merge conflict out, since staging one marks it resolved", () => {
    const codes = ["UU", "AA", "DD", "AU", "UA", "DU", "UD"];
    const files = [file(" M", "a"), file("??", "b"), ...codes.map((code) => file(code, code))];
    expect(stageAllBox(files)).toEqual({
      state: "partial",
      action: "stage",
      paths: ["a", "b"],
      conflicts: codes.length,
    });
    // With everything else staged the click stages nothing and only counts the conflicts.
    expect(stageAllBox([file("M ", "a"), file("UU", "c")])).toEqual({
      state: "partial",
      action: "stage",
      paths: [],
      conflicts: 1,
    });
  });

  it("tells conflicts from other changes", () => {
    for (const code of ["UU", "AA", "DD", "AU", "UA", "DU", "UD"]) {
      expect(isConflict(code)).toBe(true);
    }
    for (const code of ["M ", " M", "MM", "A ", "AM", " D", "D ", "R ", "??"]) {
      expect(isConflict(code)).toBe(false);
    }
  });

  it("is mixed and stages only the rest while some changes are staged", () => {
    expect(stageAllBox([file("M ", "a"), file("??", "dir/"), file("MM", "c")])).toEqual({
      state: "partial",
      action: "stage",
      paths: ["dir/", "c"],
      conflicts: 0,
    });
    // One partly staged file alone is enough for the mixed look.
    expect(stageAllBox([file("MM", "a")])?.state).toBe("partial");
  });

  it("is checked and unstages everything once all is staged", () => {
    expect(stageAllBox([file("M ", "a"), file("A ", "b"), file("D ", "c")])).toEqual({
      state: "staged",
      action: "unstage",
      paths: ["a", "b", "c"],
      conflicts: 0,
    });
  });

  it("does not show without files", () => {
    expect(stageAllBox([])).toBeNull();
  });
});

describe("pathBatches", () => {
  it("splits a long list into calls the backend accepts", () => {
    expect(pathBatches(["a", "b", "c", "d", "e"], 2)).toEqual([["a", "b"], ["c", "d"], ["e"]]);
    expect(pathBatches([], 2)).toEqual([]);
    const many = Array.from({ length: GIT_MAX_PATHS + 1 }, (_, index) => String(index));
    expect(pathBatches(many).map((batch) => batch.length)).toEqual([GIT_MAX_PATHS, 1]);
  });
});

describe("diffKey", () => {
  // Hunk header, two commentable lines, a removed line, one more commentable line.
  const numbers = [null, 1, 2, null, 3];

  it("moves the cursor over commentable lines only", () => {
    expect(diffKey("ArrowDown", null, numbers, 300)).toEqual({ kind: "move", index: 1 });
    expect(diffKey("ArrowDown", 2, numbers, 300)).toEqual({ kind: "move", index: 4 });
    expect(diffKey("ArrowDown", 4, numbers, 300)).toEqual({ kind: "move", index: 4 });
    expect(diffKey("ArrowUp", 4, numbers, 300)).toEqual({ kind: "move", index: 2 });
    expect(diffKey("ArrowUp", null, numbers, 300)).toEqual({ kind: "move", index: 4 });
    expect(diffKey("Home", 4, numbers, 300)).toEqual({ kind: "move", index: 1 });
    expect(diffKey("End", 1, numbers, 300)).toEqual({ kind: "move", index: 4 });
  });

  it("stays within the shown lines", () => {
    expect(diffKey("End", null, numbers, 3)).toEqual({ kind: "move", index: 2 });
  });

  it("opens the comment field with Enter or c on the cursor's line", () => {
    expect(diffKey("Enter", 2, numbers, 300)).toEqual({ kind: "comment", index: 2 });
    expect(diffKey("c", 4, numbers, 300)).toEqual({ kind: "comment", index: 4 });
    expect(diffKey("Enter", null, numbers, 300)).toBeNull();
    expect(diffKey("x", 2, numbers, 300)).toBeNull();
    expect(diffKey("ArrowDown", null, [null, null], 300)).toBeNull();
  });
});
import type { FileChange } from "./gitDiff";

function file(status: string, path = "a.txt"): FileChange {
  return { path, status, additions: 0, deletions: 0 };
}

describe("stageState", () => {
  it("reads the index and the work tree column", () => {
    expect(stageState("M ")).toBe("staged");
    expect(stageState("A ")).toBe("staged");
    expect(stageState("D ")).toBe("staged");
    expect(stageState("R ")).toBe("staged");
    expect(stageState(" M")).toBe("unstaged");
    expect(stageState(" D")).toBe("unstaged");
    expect(stageState("??")).toBe("unstaged");
    expect(stageState("MM")).toBe("partial");
    expect(stageState("AM")).toBe("partial");
    expect(stageState("UU")).toBe("partial");
  });

  it("stages anything not fully staged and unstages the rest", () => {
    expect(stageAction("unstaged")).toBe("stage");
    expect(stageAction("partial")).toBe("stage");
    expect(stageAction("staged")).toBe("unstage");
  });
});

describe("discardDeletes", () => {
  it("deletes untracked files and files new in the index", () => {
    expect(discardDeletes("??")).toBe(true);
    expect(discardDeletes("A ")).toBe(true);
    expect(discardDeletes("AM")).toBe(true);
    expect(discardDeletes(" M")).toBe(false);
    expect(discardDeletes("M ")).toBe(false);
    expect(discardDeletes(" D")).toBe(false);
    expect(discardDeletes("R ")).toBe(false);
  });
});

describe("commitMode", () => {
  it("commits when something is staged, offers stage all otherwise", () => {
    expect(commitMode([])).toBe("none");
    expect(commitMode([file(" M"), file("??", "b")])).toBe("stageAll");
    expect(commitMode([file(" M"), file("A ", "b")])).toBe("commit");
    expect(commitMode([file("MM")])).toBe("commit");
  });

  it("never stages a file in a merge conflict before a commit", () => {
    // A conflict counts as partly staged, so the button commits and git refuses the unmerged file.
    for (const code of ["UU", "AA", "DD", "AU", "UA", "DU", "UD"]) {
      expect(commitMode([file(code, "c"), file(" M", "a")])).toBe("commit");
    }
    expect(stageAllPaths([file(" M", "a"), file("UU", "c"), file("??", "n")])).toEqual(["a"]);
  });

  it("trims the message and refuses an empty one", () => {
    expect(commitMessage("  fix: x \n")).toBe("fix: x");
    expect(commitMessage(" \n\t")).toBeNull();
  });
});

describe("newLineNumbers", () => {
  it("counts added and unchanged lines on the new side", () => {
    const lines = [
      "@@ -10,4 +12,5 @@ fn main() {",
      " context",
      "-removed",
      "+added",
      "+added too",
      " context",
      "\\ No newline at end of file",
      "@@ -40 +43,2 @@",
      "+later",
    ];
    expect(newLineNumbers(lines)).toEqual([null, 12, null, 13, 14, 15, null, null, 43]);
  });

  it("gives no numbers before a hunk header or after a malformed one", () => {
    expect(newLineNumbers([" stray", "@@ broken @@", "+x"])).toEqual([null, null, null]);
    // Combined diffs of merges have more than one old side.
    expect(newLineNumbers(["@@@ -1,2 -1,2 +1,3 @@@", "  x"])).toEqual([null, 1]);
  });
});

describe("lineComment", () => {
  it("names the file as a mention with its line", () => {
    expect(lineComment("src/main.rs", 42, "  Why is this public?  ")).toBe(
      "@src/main.rs:42: Why is this public?",
    );
    expect(lineComment("docs/my notes.md", 3, "typo\nhere")).toBe(
      '@"docs/my notes.md":3: typo here',
    );
  });

  it("sends nothing for an empty comment", () => {
    expect(lineComment("a.txt", 1, " \n ")).toBeNull();
  });

  it("shows control and reordering characters of the path as git quotes them", () => {
    // A file name from the repository is not the user's text: a line break in it would start a
    // new line of the prompt, ESC would reach the terminal, U+2028 and bidi controls would break
    // or reorder what the agent reads.
    expect(lineComment("src/one\ntwo.ts", 7, "marker")).toBe('@"src/one\\ntwo.ts":7: marker');
    expect(lineComment("a\u001b[0mb.ts", 1, "marker")).toBe('@"a\\033[0mb.ts":1: marker');
    expect(lineComment("a\u2028b.ts", 1, "marker")).toBe('@"a\\342\\200\\250b.ts":1: marker');
    expect(lineComment("a\u202eb.ts", 1, "marker")).toBe('@"a\\342\\200\\256b.ts":1: marker');
    expect(lineComment("a\u0085b.ts", 1, "marker")).toBe('@"a\\302\\205b.ts":1: marker');
  });

  it("keeps the masked path readable back into the real one", () => {
    const paths = ["x\ny.ts", 'say "hi"\n.ts', "back\\slash\t.ts", "v\u000bf\u000c.ts", "a\u2066b"];
    for (const path of paths) {
      const text = lineComment(path, 1, "marker") ?? "";
      expect(text).not.toMatch(SPECIAL);
      const quoted = /^@(".*"):1: marker$/.exec(text)?.[1] ?? "";
      expect(unquotePath(quoted)).toBe(path);
    }
  });
});

describe("parseCommitOutcome", () => {
  it("reads how far a commit was checked", () => {
    expect(parseCommitOutcome({ kind: "committed", checked: "full" })).toEqual({
      kind: "committed",
      checked: "full",
    });
    expect(parseCommitOutcome({ kind: "committed", checked: "partial" })).toEqual({
      kind: "committed",
      checked: "partial",
    });
    // Unknown or missing: not checked.
    expect(parseCommitOutcome({ kind: "committed" })).toEqual({
      kind: "committed",
      checked: "skipped",
    });
    expect(parseCommitOutcome(null)).toEqual({ kind: "committed", checked: "skipped" });
  });

  it("reads findings with known kinds and the checked tree", () => {
    const tree = "a1".repeat(20);
    expect(
      parseCommitOutcome({
        kind: "secrets",
        findings: [
          { path: "a.txt", kinds: ["githubToken", "nonsense"] },
          { path: ".env", kinds: ["secretFile"] },
          { path: "", kinds: ["githubToken"] },
          { path: "b.txt", kinds: ["x"] },
          "junk",
        ],
        more: 2,
        complete: false,
        tree,
      }),
    ).toEqual({
      kind: "secrets",
      findings: [
        { path: "a.txt", kinds: ["githubToken"] },
        { path: ".env", kinds: ["secretFile"] },
      ],
      more: 2,
      complete: false,
      tree,
    });
    // A tree that is no tree id cannot be handed back.
    const odd = parseCommitOutcome({ kind: "secrets", findings: [], tree: "HEAD" });
    expect(odd.kind === "secrets" ? odd.tree : "x").toBeNull();
  });

  it("never reads a secrets answer without readable findings as committed", () => {
    expect(parseCommitOutcome({ kind: "secrets", findings: [] })).toEqual({
      kind: "secrets",
      findings: [],
      more: 0,
      complete: true,
      tree: null,
    });
    expect(
      parseCommitOutcome({ kind: "secrets", findings: [{ path: "a", kinds: ["?"] }] }).kind,
    ).toBe("secrets");
  });
});
