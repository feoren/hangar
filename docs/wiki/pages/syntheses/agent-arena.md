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
7. "Take over" (after an inline confirmation that lists the files) runs `arena_apply`; afterwards the dialog offers to
   close the other contestants and delete their worktrees with all changes (`arena_discard`).

The arena lives in the window's memory only; after a restart its sessions and worktrees remain as
ordinary sessions.

## Backend contract (`src-tauri/src/arena.rs`)

| Command | Answer |
| --- | --- |
| `arena_create(cwd, labels)` | `{ id, baseCommit, repoRoot, worktrees }` |
| `arena_diff_stat(worktree, baseCommit)` | `{ files: [{ path, added, removed }], added, removed, truncated }` |
| `arena_apply(worktree, baseCommit, files)` | `{ kind: "applied", files }` or `{ kind: "conflict", files }` |
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

**Taking over:** the confirmation lists the files that arrive (from the last diff stat; files
that can run code without being called, `runsCode` in `src/arena.ts`, come first and are marked:
`.envrc`, `package.json`, `.vscode/`, `.github/workflows/`, git and package hooks, build tool
configs, agent settings). `arena_apply(worktree, baseCommit, files)` gets exactly that list and
refuses with `arena_changed` when the contestant's changes differ from it by now, so nothing
arrives unseen; a cut list (`truncated`) cannot be taken over at all. The project folder
(`repoRoot` of the record) must be the top of the repository whose `git worktree list` names the
contestant's worktree; otherwise nothing happens. Then, before anything changes:

- Every path (tracked and new) must pass `safe_relative`: plain names, no component that is
  `.git` in any case, with trailing dots or spaces, with the code points HFS+ ignores or as the
  NTFS short name `git~<n>` (`git_actions::names_git_folder`, shared with the git panel); on
  Windows also no trailing dot or space and no device name (`nul`, `com1.txt`, `CONOUT$`).
  Otherwise `arena_unsafe_path`.
- `git diff --raw` with the patch's own arguments must show no symbolic link (mode 120000) or
  submodule (160000) on either side (`arena_link`).
- A file the winner changed that is modified, staged or untracked in the project folder
  (`git status --untracked-files=all`, compared without case on Windows and macOS), or a new file
  whose path already exists there, ends in a `conflict` answer listing those files.
- New files must be regular files reached without a link in the worktree; no component of any
  path in the project folder may be a link or junction (`arena_link`); the deepest existing folder
  of each new file must resolve inside the project folder and outside its git folder
  (`git rev-parse --absolute-git-dir`, resolved), which also catches a short name or alias the
  name check could not know.

Then the patch is made (`git diff --binary --full-index --no-renames <base>`), the new files are
copied, and only then the patch goes in with `git apply -` (all or nothing, no `--index`, so
nothing is staged). Each new file is opened without following a link (Windows
`FILE_FLAG_OPEN_REPARSE_POINT`, Unix `O_NOFOLLOW`) and its type is checked again on the handle; the
target is created with `create_new`, never written through something already there; each target
folder is checked before and after it is created. If a copy fails, or `git apply` fails
afterwards (the project's HEAD moved), the files and folders this take-over created go again and
the project folder is as before. The order is chosen so a patch never has to be taken back:
`git apply -R` restores the content but not the bytes (see measurements). Nothing is ever
committed.

**Discarding:** only after the user's confirmation, only for arena branches and only for a
folder `git worktree list` names as a worktree of the repository (a folder of the right shape that
git does not list stays, `OutsideFolder`). Hangar deletes the folder itself with
`std::fs::remove_dir_all`, which removes a link or junction without entering it (Rust 1.58.1 and
later, CVE-2022-21658), then `worktree remove --force` takes git's record of the now missing
folder, and `branch -D` the branch, with filters off; no repository hook runs, because `Git`
switches them off for every call. A folder that cannot be deleted (a program still has files
open) answers `arena_remove`. A folder deleted by hand leaves git's record, which
`worktree remove` takes away for this one worktree (never `worktree prune`, which would also take
the records of the user's own worktrees). Plain session worktrees keep the never-force rule of
`worktree::remove`.

## Measurements (Windows 11, Git 2.56.0.windows.4, 2026-11-15)

Measured under `tests/.tmp/` and pinned by the tests in `arena.rs`:

- **Trailing dots:** Win32 drops them, so `target/.git./hooks` opens `target/.git/hooks` (asserted
  in `new_files_never_land_in_the_git_folder`). Git Bash (MSYS) creates a literal `.git.` folder,
  and `git ls-files --others` in such a repository lists the contents of the real `.git` as
  `.git./HEAD`, `.git./config` and so on: names like that do reach the arena from git.
- **Short names:** 8dot3 creation is off on the project volume (`fsutil 8dot3name query`), but
  `fsutil file setshortname .git GIT~1` succeeded for the user, after which `GIT~1/hooks` opens
  `.git/hooks`; the test sets one where it may and then checks the refusal (the case ran here).
- **`git worktree remove --force` and junctions:** it did not follow a junction to a folder outside
  (the marker file stayed), but it did not delete the junction either: the worktree folder with
  the junction in it remained, with exit code 0. `remove_dir_all` removes the junction and the
  folder, a read-only file included (`discarding_never_follows_a_link_out_of_the_worktree`).
- **Symlinks in a patch:** with `core.symlinks=false` (Git for Windows default) `git apply` writes
  a mode 120000 entry as a plain file holding the link target; with `core.symlinks=true` and no
  symlink privilege it failed with `unable to write file 'lnk' mode 120000`. On Unix it would make
  a real link. The arena refuses both.
- **Submodules:** with `--ignore-submodules=all` (in `DIFF_ARGS`) a committed gitlink appears
  neither in `--raw` nor in the patch, so `git apply` never sees one; the raw check refuses 160000
  anyway should that change.
- **`git apply -R`:** with `core.autocrlf=true` (system config of Git for Windows) applying and
  reversing a patch left `one` with CRLF where the file held `one` with LF; `git status` then
  reports the file as modified although `git diff` is empty. Hence new files are copied before the
  patch and taken back if it fails, instead of reversing a patch.

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
