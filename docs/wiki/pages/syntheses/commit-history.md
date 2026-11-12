---
title: Commit history (contract)
type: synthesis
tags: [git, changes-panel, contract, security]
sources: []
created: 2026-11-12
updated: 2026-11-12
---

# Commit history (contract)

Backend for showing committed changes next to the work tree's changes in the "Changes" tab of the
right panel. Code: `src-tauri/src/git_history.rs` (parsers, rules and tests), commands in
`src-tauri/src/commands.rs`. Both commands run through the hardened git runner of
`src-tauri/src/git.rs` with the same folder checks (`check_folder`, `check_git_links`,
`check_work_tree` through `toplevel`), the same 10 second deadline and the same output caps as
`git_diff` ([[agent-workflow-features]], "Git").

## Commands

| Command | Arguments | Result |
|---|---|---|
| `git_log` | `cwd: string`, `skip: number`, `limit: number` (clamped to 1..=100) | `GitLog` |
| `git_commit_diff` | `cwd: string`, `sha: string` (7 to 64 hex characters) | `GitCommitDiff` |

```ts
interface CommitSummary {
  sha: string;        // full id, 40 (SHA-1) or 64 (SHA-256) hex characters
  shortSha: string;   // git's abbreviation (core.abbrev)
  subject: string;    // at most 1000 characters
  author: string;     // author name as recorded (%an, no mailmap)
  authoredAt: number; // author date, Unix seconds
  pushed: boolean;    // reachable from the upstream; false without one
}
interface GitLog {
  isRepo: boolean;          // false: folder outside a work tree
  commits: CommitSummary[]; // reachable from HEAD, newest first
  upstream: string | null;  // "origin/main"; null without upstream or on a detached HEAD
  aheadCount: number;       // commits on HEAD the upstream lacks
  hasMore: boolean;         // another page follows (skip += commits.length)
}
interface CommitHeader {
  sha: string; shortSha: string;
  parents: string[];  // empty for a root commit, 2+ for a merge
  subject: string;
  body: string;       // message after the subject, at most 16 KiB
  author: string; authoredAt: number;
}
// GitDiff of git_diff plus the header; parseGitDiff in src/gitDiff.ts reads it unchanged.
interface GitCommitDiff extends GitDiff { commit: CommitHeader }
```

`files[].status` in a commit diff has the letter in the index column (`"A "`, `"M "`, `"D "`,
`"R "`, `"C "`, `"T "`), so `changeKind` classifies it like a staged change. Renames list the new
path. `diff` is cut at 2 MiB on a whole line and `truncated` set, as for `git_diff`.

A repository without commits gives an empty `GitLog` with `isRepo: true`. Errors are English
strings: "The folder is not inside a git repository" (`git_commit_diff` outside a work tree),
"Invalid commit id: expected 7 to 64 hexadecimal characters", "The commit was not found in this
repository" (unknown, ambiguous, or not a commit such as a tree id), plus the existing git errors.

## Frontend

The "Changes" tab has a switch "Uncommitted | Commits" (`ChangesPanel.tsx`; the window keeps the
choice in `App.tsx` state, not in the settings). `CommitHistory.tsx` reads pages of 30 commits and
re-reads as many as are listed on "Refresh" and on every attention change of the session.
`gitHistory.ts` validates both answers (`parseGitLog`, `parseCommitDiff`; only full ids pass),
merges pages (`appendPage`: no duplicates, an empty page ends the paging) and groups the list into
"not pushed" and the rest (`commitGroups`). A commit opens in place; its diff is read on the first
open and kept while the panel is mounted, and it is shown with the same `ChangeList` as the
uncommitted changes, so only its last file opens by itself.

## How it runs

- `git_log`: `git log --no-show-signature --no-notes --no-decorate --no-color --encoding=UTF-8
  --format=%H%x00%h%x00%at%x00%an%x00%s --skip=N --max-count=limit+1 HEAD --`. Fields are
  NUL-separated, records end at the newline git appends; a subject is one joined line and an
  author one header line, so neither holds a newline, and git reads commit objects as C strings,
  so no field holds a NUL. The subject is the last field. The one extra commit sets `hasMore`.
- Upstream: `git rev-parse --abbrev-ref --symbolic-full-name @{upstream}`; a failure (no
  upstream, detached HEAD, ref gone) means none. `git rev-list @{upstream}..HEAD` lists the
  unpushed commits once; its size is `aheadCount`, and every commit of the page not in it is
  `pushed`. If that list exceeds the 4 MiB list cap, each commit of the page is checked with
  `git merge-base --is-ancestor` instead and the count comes from `rev-list --count`.
- `git_commit_diff`: `sha` is checked to be hex only before git sees it, so it can never be read as
  an option or a revision expression; `git rev-parse --verify --quiet <sha>^{commit}` resolves it
  to a full id and refuses any other object. Header from `git log --max-count=1` with the body as
  last field, capped at 64 KiB of output; git joins a first paragraph without blank lines into
  one subject line, so a long one can fill the cap alone, and the body is then empty. Changes from `git diff-tree -r -M --no-commit-id --no-color --no-ext-diff
  --no-textconv --ignore-submodules=all` with `--name-status -z`, `--numstat -z` and `-p`, against
  the first parent (`<parent> <sha>`) or, for a root commit, `--root <sha>` (the empty tree). The
  repository's filter drivers are switched off as for `git_diff`, although tree-to-tree diffs read
  no work tree.

> [!question] Unverified
> A short id that equals the name of a ref resolves to that ref (git's lookup order for ids
> shorter than a full id). The answer always carries the resolved full `sha`, and the history list
> only ever passes full ids, so the window shows what it asked for. Observed once with git
> 2.56.0.windows.4 (a branch named like the first 7 characters of another commit's id), not yet
> filed as a raw measurement.
