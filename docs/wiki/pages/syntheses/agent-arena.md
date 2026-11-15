---
title: Agent arena
type: synthesis
tags: [arena, worktree, git, agents]
sources: []
created: 2026-11-13
updated: 2026-11-15
---

# Agent arena

One task for 2 to 4 installed agents at once, each in its own worktree from the same commit. The
comparison shows per contestant its state, time, changed files and lines, and an optional test
result; "Take over" brings the winner's changes into the project folder as uncommitted changes.

## Flow

1. The arena dialog (File menu, quick access through the menu entry, a small button beside the
   project folder card in NEW SESSION) takes the task, 2 to 4 agents (programs that are not a known
   shell, `shellKind`), a project folder inside a git repository (`worktreeChance` of the start
   places) and an optional test command.
2. `arena_create` reads HEAD once (`rev-parse --verify HEAD^{commit}`) and creates one worktree per
   agent with `worktree::create_at` from that commit id, branch `hangar/arena-<6 hex>-<agent>`
   (the same agent twice gets `-2`). If one fails, the clean ones already made are removed again.
   A share or device path is refused as for `create_worktree`, and each new worktree counts as in
   use for the worktree cleanup until its session works in it (`PendingWorktrees`).
3. Each agent starts as a private session in its worktree (`start` with a prepared worktree), title
   `Arena · <program>`, and the sessions show side by side in the split view. No upstream check
   goes in as the first prompt ([[agent-instructions]]): the worktree sits at the arena's commit on
   purpose and the task follows; HANGAR.md applies as for every session.
4. The task goes in per session as a bracketed paste followed by Enter (as "Send to several"), once
   the session's output has been quiet for 2 s, never before 3 s and at the latest after 45 s
   (`promptReady`). The dialog says so before the start, because Hangar presses Enter for the user.
5. A contestant is "working" from the task on until the attention tracker reports it quiet after
   working; then it is "done" and its time stops (`trackAttention`). Working again restarts it.
6. When a contestant is done and a test command is set, the backend runs it once per finish.
7. "Take over" (after an inline confirmation) runs `arena_apply`; afterwards the dialog offers to
   close the other contestants and delete their worktrees with all changes (`arena_discard`).

The arena lives in the window's memory only; after a restart its sessions and worktrees remain as
ordinary sessions.

## Backend contract (`src-tauri/src/arena.rs`)

| Command | Answer |
| --- | --- |
| `arena_create(cwd, labels)` | `{ id, baseCommit, repoRoot, worktrees }` |
| `arena_diff_stat(worktree, baseCommit)` | `{ files: [{ path, added, removed }], added, removed, truncated }` |
| `arena_apply(worktree, baseCommit)` | `{ kind: "applied", files }` or `{ kind: "conflict", files }` |
| `arena_discard(worktree)` | nothing |
| `arena_run_tests(worktree, command)` | `{ passed, exitCode, timedOut, durationMs, output }` |

Every call checks the worktree record like `remove_worktree` (shape, inside Hangar's worktrees
folder, local git folders) and additionally that the branch is an arena branch.

**Errors:** an `ArenaError` of its own reaches the window as `[arena_<kind>] English text` (the
form of `ManagerError`); errors passed through from git and worktree keep their text without a
code. `arenaErrorText` (`src/arena.ts`) shows each code as a translated sentence, the common git
errors as for `create_worktree`, and anything else as "Unknown error", so no English text reaches
the user. `arena_program_not_found` keeps the fixed text `The program <name> was not found`, from
which the window reads the program name.

**Diff stats:** `git diff --numstat --no-renames <base> --` in the worktree covers committed,
staged and unstaged changes; new files come from `ls-files --others --exclude-standard` (ignored
files such as build output stay out) and are counted by lines in Hangar (a NUL byte or more than
8 MB counts as binary, 0 lines). Filters are switched off (`filter_overrides`), and `DIFF_ARGS`
keeps external diff and textconv off.

**Taking over:** the project folder (`repoRoot` of the record) must be the top of the repository
whose `git worktree list` names the contestant's worktree; otherwise nothing happens. It is then
refused before anything changes when a file the winner changed is modified,
staged or untracked in the project folder (`git status --untracked-files=all`, compared without
case on Windows and macOS) or a new file's path already exists there; the answer lists those
files. Otherwise tracked changes go through `git diff --binary --full-index --no-renames <base>`
into `git apply -` in the project folder (all or nothing, no `--index`, so nothing is staged), and
new files are copied; each target folder must resolve inside the project folder, and links are
refused. If the project folder's HEAD moved away from the base commit, `git apply` decides and its
message is shown. Nothing is ever committed.

**Discarding:** only after the user's confirmation and only for arena branches:
`worktree remove --force` and `branch -D`, with filters off; no repository hook runs, because
`Git` switches them off for every call (no caller names its own `core.hooksPath`). A folder
deleted by hand leaves git's record, which `worktree remove` takes away for this one worktree
(never `worktree prune`, which would also take the records of the user's own worktrees). Plain
session worktrees keep the never-force rule of `worktree::remove`.

## Test command: argv, not a shell

The command is split at whitespace by the rule of update commands (`updates::split_command`):
any shell syntax (`| & ; < > $` quotes and the like) is refused instead of run half right. The
program is looked up on PATH only (never in the worktree), a `.cmd`/`.bat` target passes the
checks of [[cmd-launcher]], the environment is a session's (`child_env`, lookup in the working
folder off, `NO_COLOR=1`), stdin is empty, and after 10 minutes, or as soon as the command ends,
its whole process tree is ended (Job Object, process group). The last 4000 bytes of stdout and
stderr are kept without color codes.

Why not the user's shell profile: a shell line would let anything typed there (or pasted from a
task) become command syntax, and the shell's own profile scripts would run in a folder whose files
an agent wrote. The test command still runs code from the worktree (that is its purpose); argv
keeps Hangar from adding a second interpreter on top. Commands that need a pipe or a variable go
into a script of the project (`pnpm test`, `npm run check`).

## Cost

No per-session cost or token API exists yet, so the comparison has no cost column. The hook is
marked in `src/ArenaDialog.tsx` (table header): once the backend exposes cost per session, the
column goes there, shown only when a value exists.

## Open

> [!question] Unverified
> Agent CLIs that ask a question before their prompt in a new folder (for example a folder trust
> question) may take the pasted task as the answer. "Send task again" in a contestant's details
> sends it once more. Not measured per agent.
