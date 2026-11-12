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
re-reads as many as are listed on "Refresh" and on every attention change of the session
(`readLog`: in pages of at most 100, so a list grown by "Load more" keeps its length; "Load more"
is off while that read runs).
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

## Git actions

The "Uncommitted" view also changes the repository. Code: `src-tauri/src/git_actions.rs` (rules
and tests with real repositories), commands in `src-tauri/src/commands.rs`, frontend in
`src/gitActions.ts` (pure rules), `src/CommitBox.tsx`, `src/ChangeList.tsx` and
`src/ChangesPanel.tsx`. All four commands take the session's ID, never a folder: the backend looks
up the session's working folder like `reveal_session_folder` does, then runs the same folder
checks as `git_diff` (`check_folder`, `toplevel`). Granted to the main window and the panel window.

| Command | Arguments | Result |
|---|---|---|
| `git_stage` | `id: number`, `paths: string[]` | `null` |
| `git_unstage` | `id: number`, `paths: string[]` | `null` |
| `git_discard` | `id: number`, `paths: string[]` | `null` |
| `git_commit` | `id: number`, `message: string` | `null` |

`paths` are relative to the repository root, exactly as `git_diff` lists them (`/` separated, a
trailing `/` for an untracked folder), 1 to 10,000 per call, each at most 4096 bytes. Refused
before git runs: empty, absolute, a drive or `:` on Windows, a NUL, an empty, `.` or `..`
component (on Windows also one that is empty after trailing dots and spaces), and any `.git`
component. git itself refuses a path beyond a symbolic link.

- Hooks: stage, unstage and discard pass `-c core.hooksPath=<folder>`, an empty folder
  `git-no-hooks` below Hangar's local data folder (created on first use; refused if anything is
  in it), so no repository hook (`post-index-change`, for example) runs for them. Only a commit
  runs hooks.
- Links (discard only, checked before git runs): every component of every path below the work
  tree root is read with `symlink_metadata`; a symbolic link or, on Windows, any reparse point
  (junctions included, which Git for Windows may take for a plain folder, so `git clean -f -d`
  could delete the junction target's content) is refused with "<path> leads through or contains a
  link (symbolic link or junction); discard it in the terminal". An untracked folder is walked
  without following links, at most 100,000 entries; a larger or unreadable one is refused too.
- Deadlines: stage, unstage and discard 30 seconds plus 10 ms per path, commit 600 seconds. A
  timeout says that `.git/index.lock` may be left behind and must be removed by hand once no git
  runs; Hangar never removes it (another git may hold it).
- git's environment (all calls, git.rs) also drops `INHERITED_SESSION_MARKERS` of a calling
  agent session (manager.rs), so its identifiers never reach hooks or helpers.

- Paths reach git as literal pathspecs: `git --literal-pathspecs ... --pathspec-from-file=-
  --pathspec-file-nul` with the list on stdin, so no path is read as an option, a glob or pathspec
  magic and no list overflows the command line. `ls-files` and `clean` read no pathspec file;
  they get the paths after `--`, 64 per call.
- `git_stage`: `git add --all` (new, changed and deleted files).
- `git_unstage`: `git reset --quiet`; works without commits too. The original path of a rename
  is added, so the rename is unstaged as a whole.
- `git_discard`: tracked files `git restore --source=HEAD --staged --worktree` (a file new in the
  index is removed; renames with their original path), in a repository without commits
  `git rm -f` (status lists files there, never folders, so no `-r`); untracked files and folders `git clean -f -d`, which keeps ignored files and
  nested repositories. Paths without changes are skipped. The window asks first, naming the file
  and saying it cannot be undone (`Dialog`); for an untracked file or one new in the index it
  asks to delete the file (`discardDeletes`), because that is what happens.
- Filters: stage, unstage and discard switch the repository's filter drivers off as the read
  commands do. When any driver is configured (a global Git LFS install is enough), the files at
  and below the paths are listed (`ls-files --cached --others --exclude-standard`) and checked with
  `check-attr filter`; a file a filter would convert is refused with "A file uses a git filter (for
  example Git LFS); stage or discard it in the terminal", because staging or restoring it without
  its filter would store or write the wrong content.
- `git_commit`: `git commit --quiet --file=-`, the trimmed message on stdin (at most 64 KiB; no
  control characters except tab, LF and CR, no DEL, no C1, so no escape sequence reaches a log
  reader's terminal). Hooks, signing and the identity run as configured; nothing is bypassed and
  no filter override is passed, because hooks would inherit it. Deadline 600 seconds. A
  failure is git's message (stderr, or stdout where git reports there, such as "no changes added
  to commit"); a missing identity fails with git's "Author identity unknown" text.

Errors are English strings: "Unknown session: N", "Invalid file path: <reason>", "No files given",
"Too many files: at most 10000 per call", the filter message above, "The commit message is empty",
"The commit message is too long or holds control characters", the link and timeout messages
above, plus the existing git errors.

### Frontend

- Per file a stage box (checked: all staged, mixed: partly, empty: unstaged or untracked; a click
  on a fully staged file unstages it, otherwise it stages) and a discard button on hover. While an
  action runs the box is `aria-disabled`, not `disabled`, so it keeps the keyboard focus.
- The commit box sits at the top of the view while there are changes: "Commit" commits what is
  staged; with nothing staged the button reads "Stage changed and commit" and stages the changed
  and deleted tracked files first, as a commit with `-a` would (`stageAllPaths`). Untracked files
  only go in through their own box; with nothing but untracked files the box is hidden. In an agent session "Suggestion from the agent" pastes a request for a commit
  message (for `git diff --cached`, or `git diff HEAD` when nothing is staged) into its prompt
  without Enter; the answer appears in the terminal and is not read back.
- Line comments, agent sessions only: a click on an added or unchanged diff line (or the comment
  icon on hover) opens a one-line field; Enter pastes `@<path>:<line>: <comment>` into the
  session's prompt through the notes' send path (bracketed paste, no Enter; the panel window goes
  through `panel_to_main`). The path is relative to the repository and quoted as a mention is
  (`mentionText`); the line is the new side's number from the hunk header (`newLineNumbers`).
  Removed lines take no comment. Keyboard: the diff is one Tab stop (`aria-activedescendant`);
  arrows, Home and End move a cursor over the commentable lines, Enter or `c` opens the field,
  Escape closes it and returns the focus to the diff (`diffKey`).
- After every action the panel reads the changes again; git's message shows above the list.

> [!question] Unverified
> A hook that runs longer than 600 seconds is killed with the commit, which may leave
> `.git/index.lock` behind. Not measured. The link check was measured with an NTFS junction
> (`mklink /J`) on Windows 11 in the unit tests; Unix symbolic links are covered by the same test
> on Unix runners.
