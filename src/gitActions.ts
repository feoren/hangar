// Git actions of the changes panel (contract: docs/wiki/pages/syntheses/commit-history.md, "Git
// actions"): what a file's stage box shows and does, what the commit button offers, the line
// numbers of a diff for line comments and the text such a comment sends to the agent. Pure, so the
// rules can be tested; the panel calls the backend (`gitStage`, `gitUnstage`, `gitDiscard`,
// `gitCommit` in api.ts).

import type { FileChange } from "./gitDiff";
import { lineKind } from "./gitDiff";
import { quotePath } from "./pathQuote";

/** Whether a file's changes are in the index: all of them, some of them, or none. */
export type StageState = "staged" | "partial" | "unstaged";

/** From the porcelain XY code: X is the index, Y the work tree. Untracked files are unstaged. */
export function stageState(status: string): StageState {
  const x = status.charAt(0);
  const y = status.charAt(1);
  if (status === "??" || x === " " || x === "") return "unstaged";
  if (y === " " || y === "") return "staged";
  return "partial";
}

/** A click on the stage box: a fully staged file leaves the index, anything else goes in. */
export function stageAction(state: StageState): "stage" | "unstage" {
  return state === "staged" ? "unstage" : "stage";
}

/** Paths per stage or unstage call; the backend refuses more (`MAX_PATHS` in git_actions.rs). */
export const GIT_MAX_PATHS = 10_000;

/** The box over the file list that stages or unstages every file: its look and its click. */
export interface StageAllBox {
  /** Checked: every file fully staged; mixed: some changes staged; empty: none. */
  state: StageState;
  action: "stage" | "unstage";
  /** The files the click stages or unstages. */
  paths: string[];
}

/**
 * The header box of the changes list; `null` without files. Anything not fully staged makes a
 * click stage the rest, untracked files included (the user picks "all", unlike the commit
 * button's `git commit -a`); only when everything is staged does a click unstage everything.
 */
export function stageAllBox(files: readonly FileChange[]): StageAllBox | null {
  if (files.length === 0) return null;
  const states = files.map((file) => stageState(file.status));
  if (states.every((state) => state === "staged")) {
    return { state: "staged", action: "unstage", paths: files.map((file) => file.path) };
  }
  const state = states.every((each) => each === "unstaged") ? "unstaged" : "partial";
  const paths = files.filter((_, index) => states[index] !== "staged").map((file) => file.path);
  return { state, action: "stage", paths };
}

/** Paths in calls of at most `size` each, so a huge list stays within the backend's limit. */
export function pathBatches(paths: readonly string[], size = GIT_MAX_PATHS): string[][] {
  const batches: string[][] = [];
  for (let start = 0; start < paths.length; start += Math.max(1, size)) {
    batches.push(paths.slice(start, start + Math.max(1, size)));
  }
  return batches;
}

/**
 * What the commit button does: commit the staged files, or (nothing staged) stage the changed and
 * deleted tracked files and then commit, like `git commit -a`, said so on the button. New files
 * only go in through their own stage box. `none`: nothing to commit.
 */
export type CommitMode = "commit" | "stageAll" | "none";

export function commitMode(files: readonly FileChange[]): CommitMode {
  if (files.some((file) => stageState(file.status) !== "unstaged")) return "commit";
  return stageAllPaths(files).length > 0 ? "stageAll" : "none";
}

/** What "stage changed files and commit" stages: every listed file except untracked ones. */
export function stageAllPaths(files: readonly FileChange[]): string[] {
  return files.filter((file) => file.status !== "??").map((file) => file.path);
}

/**
 * Whether discarding the file deletes it: an untracked file, or one new in the index (HEAD has no
 * version of it to go back to, so `git_discard` removes it from the disk too).
 */
export function discardDeletes(status: string): boolean {
  return status === "??" || status.charAt(0) === "A";
}

/** The commit message as it is sent: trimmed; `null` when nothing is left. */
export function commitMessage(text: string): string | null {
  const message = text.trim();
  return message === "" ? null : message;
}

/**
 * Line number in the changed file for each line of a file's diff (as `splitDiff` keeps them, from
 * the first hunk header on): added and unchanged lines count on the new side and can take a
 * comment; removed lines, hunk headers and notes get `null`.
 */
export function newLineNumbers(lines: readonly string[]): (number | null)[] {
  let next: number | null = null;
  return lines.map((line) => {
    const kind = lineKind(line);
    if (kind === "hunk") {
      const start = /^@@+ (?:-\d+(?:,\d+)? )+\+(\d+)(?:,\d+)? @@/.exec(line)?.[1];
      next = start === undefined ? null : Number(start);
      return null;
    }
    if (next === null || kind === "del" || kind === "note") return null;
    const number = next;
    next += 1;
    return number;
  });
}

/**
 * What a line comment sends into the agent's prompt: `@path:line: comment`, the path relative to
 * the repository and quoted as a mention is (fileTree.ts, `mentionText`), the comment on one line.
 * `null` for an empty comment.
 */
export function lineComment(path: string, line: number, comment: string): string | null {
  const text = comment.replace(/\s+/g, " ").trim();
  if (text === "") return null;
  return `@${quotePath(path, "agent")}:${String(line)}: ${text}`;
}

/** What a key does in a focused diff: move the line cursor, or open the comment field. */
export type DiffKey = { kind: "move"; index: number } | { kind: "comment"; index: number } | null;

/**
 * Keyboard path for line comments without a Tab stop per line: the diff takes the focus once,
 * arrows (Home, End) move a cursor over the lines that take a comment (`numbers` not `null`,
 * within the first `shown` lines), Enter or `c` comment on the line under it. The caller leaves
 * keys with Ctrl, Alt or Meta alone.
 */
export function diffKey(
  key: string,
  cursor: number | null,
  numbers: readonly (number | null)[],
  shown: number,
): DiffKey {
  const limit = Math.min(shown, numbers.length);
  const commentable: number[] = [];
  for (let index = 0; index < limit; index += 1) {
    if (numbers[index] !== null) commentable.push(index);
  }
  const first = commentable[0];
  const last = commentable[commentable.length - 1];
  if (first === undefined || last === undefined) return null;
  const current = cursor !== null && commentable.includes(cursor) ? cursor : null;
  switch (key) {
    case "ArrowDown":
      return {
        kind: "move",
        index: current === null ? first : (commentable.find((i) => i > current) ?? current),
      };
    case "ArrowUp":
      return {
        kind: "move",
        index:
          current === null
            ? last
            : ([...commentable].reverse().find((i) => i < current) ?? current),
      };
    case "Home":
      return { kind: "move", index: first };
    case "End":
      return { kind: "move", index: last };
    case "Enter":
    case "c":
    case "C":
      return current === null ? null : { kind: "comment", index: current };
    default:
      return null;
  }
}
