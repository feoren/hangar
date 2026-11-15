---
title: Work tree timeline (contract)
type: synthesis
tags: [git, changes-panel, contract, security]
sources: ["[[timeline-restore-measurement]]"]
created: 2026-11-13
updated: 2026-11-15
---

# Work tree timeline (contract)

"Time travel per agent step": after every step of an agent session Hangar quietly saves the state
of the session's work tree, the "Timeline" view of the Changes tab lists those snapshots, shows
what each step changed and goes back to any of them. Backend `src-tauri/src/git_snapshots.rs`,
commands in `src-tauri/src/commands.rs` (`snapshot_*`), frontend `src/timeline.ts` (pure logic)
and `src/TimelineView.tsx`.

## Key binding

The main window binds each session's workspace key (`SavedSession.key`, a UUID that survives
restarts) to its session ID with `snapshot_bind` as soon as it wires the terminal. Every other
command names the session by ID only; the backend (`Snapshots`) looks the key and the session's
folder up. A session keeps its key, a key held by another running session is refused, the key of
a closed session moves to the session that restores it. The panel window therefore cannot reach
another session's snapshots: it never names a key, and its ID is checked against the session the
main window handed it (`PanelScope`).

## Trigger

The main window calls `snapshot_take(id, prompt)` on the same transition that offers the done
notice (`finishedWork` in `src/doneNotice.ts`: working to idle or waiting, not the gate, and only
after the user typed), also when a queued message continues the session right away, for agent
sessions only (`takesSnapshots` in `src/timeline.ts`: an agent profile by `runsAgent`, no update
sessions, no placeholders).

The prompt (`StepPrompt`, `SessionTerminal.takeStepPrompt()`) is the first non-empty line
submitted with Enter while the agent was not working (attention state idle or waiting), cut to
200 characters on arrival and by the backend, and reset once the step's snapshot was asked for.
Lines submitted while the agent works (a password prompt of a tool it runs) are never kept, nor a
line whose cursor row ends in a password hint and a colon (`Password:`, `Passphrase:`,
`Kennwort:`, `PIN:`, `token:` and others); a prompt of the user's own that ends that way is
dropped too. The prompt is stored in plain text as the subject of the snapshot commit in the side
repository, below Hangar's local data folder, and leaves the machine only if the user copies
that folder.

## Where the view is offered

The "Timeline" view of the Changes tab exists only for sessions for which `takesSnapshots` holds,
decided by the profile (an exited agent session keeps it; `props.agent` is false once a session
exited and is not used). For other sessions a stored view `timeline` is shown as `open`; the
stored value stays, so the view is back on the next agent session. With "Highlight waiting
sessions" (`attentionEnabled`) off, the working state is never detected and no step is saved:
the view then shows `timeline.detectionOff` instead of the list. Main and panel window decide
alike (`timelineAvailability`).

## Side repository (nothing goes into the user's .git)

Each repository gets a bare side repository `<app local data>/git-snapshots/<hash>.git`, created
with `git init --bare --template=` on first use. `<hash>` is the first 128 bits of SHA-256 over
`path_key` of the repository's common git folder (`rev-parse --path-format=absolute
--git-common-dir`), so linked worktrees share one side repository. The file `hangar-source` in it
names that common folder.

git runs in the user's work tree with `GIT_OBJECT_DIRECTORY=<side>/objects` and
`GIT_ALTERNATE_OBJECT_DIRECTORIES=<common>/objects`: blobs, trees and commits written by a
snapshot land in the side repository, unchanged files are read from the user's objects. Ref calls
(`for-each-ref`, `update-ref`) run in the side repository. A common folder whose path holds the
alternates separator (`;` on Windows, `:` elsewhere) or starts with `"` cannot be named there and
is refused (`UnusablePath`). Test `nothing_is_written_into_the_users_repository`: after a snapshot
and a jump back the user's git folder holds the same files with the same sizes, and no
`refs/hangar/` exists there, so no clone, fetch or push of the user's repository can carry them.

> [!question] Unverified
> A snapshot commit's parent is the user's HEAD at that time, and its unchanged blobs and trees
> may exist only in the user's objects. If the user rewrites history and a `git gc` there drops
> them, the first step's diff (against its parent) and a jump back to such a snapshot fail with
> git's message; the clean-up's `gc` then fails too and deletes nothing.

## Snapshot (what `git stash create` does, without touching anything)

1. Folder checks of `git.rs` (`check_folder`, `toplevel`), filter drivers switched off
   (`filter_overrides`), hooks and fsmonitor off through git.rs, `core.splitIndex=false`,
   `core.logAllRefUpdates=false`, `core.protectNTFS=true`, `core.protectHFS=true`,
   `core.ignoreStat=false`. One time limit of 30 s for the whole operation.
2. `git status --porcelain=v1 -z --untracked-files=all`: more than 2000 entries (or a list cut at
   4 MiB) skips the step ("tooManyFiles"). A changed file above 50 MiB, or all changed files
   together above 200 MiB (sizes on disk, links not followed), skip it too ("tooLarge").
3. The user's index (`rev-parse --git-path index`) is copied to a temporary file below
   `<app local data>/git-snapshots/tmp/`; with `GIT_INDEX_FILE` pointing there, `git add --all`
   and `git write-tree` give the work tree's tree. Ignored files never go in; tracked files stay
   even when ignored. The copy keeps the stat cache, so only changed files are hashed. Before
   `add`, the assume-unchanged bit of every entry in the copy is cleared (`ls-files -v -z`, then
   `update-index --no-assume-unchanged -z --stdin`): `add` would keep such a file's old blob and a
   jump back would lose its newer content ([[timeline-restore-measurement]]). The user's index
   keeps its bits. The sizes are then checked again on the stored blobs (`diff-tree --raw`
   against HEAD's tree, `cat-file --batch-check=%(objectsize)`): a file that grew between the
   check on disk and `add` skips the step as `tooLarge`; its blobs stay unreferenced until the
   next `gc`.
4. The tree is compared with the newest snapshot's tree (for the first step: HEAD's tree). Equal:
   no snapshot.
5. `commit-tree --no-gpg-sign <tree> -p HEAD -F -` with Hangar's own identity
   (`Hangar <snapshots@hangar.invalid>`) and the subject `hangar-snapshot <step|undo> <files>
   <prompt>`; then one `update-ref --stdin` transaction in the side repository creates
   `refs/hangar/snapshots/<key>/<n>` and deletes the oldest beyond 50.

HEAD, the branch, the index, `refs/stash` and every reflog stay as they were. End of line
conversion works as with `git stash`: text attributes and `core.autocrlf` apply on the way in and
out.

## Lifetime

At most 50 snapshots per session; numbers count up and are never reused. The window calls
`snapshot_forget(id)` only after `close_session` succeeded (the binding keeps the folder, so this
works after the session ended, and it runs before the session's worktree is removed). It also
calls it for the sessions `removeOrphanedWorktrees` closes, for background sessions ended after a
reload when "Remember sessions" is off, and on quit for every session when "Remember sessions" is
off. Quitting with it on keeps them: the restored session has the same key.

At start the backend cleans up once on a thread of its own (`Snapshots::clean_up`, every other
snapshot call waits for it), but only if no other Hangar runs: a debug build ignores the single
instance lock (`single_instance.rs`) and shares the local data folder, workspace file included,
with the installed Hangar, so its clean-up would delete refs and side repositories the other one
uses. Every running Hangar therefore holds a shared lock on `git-snapshots/.lock` for its whole
run (`InstanceLock`, `std::fs::File::lock_shared`, stable since Rust 1.89, no crate needed); the
clean-up runs only in an instance that gets it exclusively (`try_lock`), which then turns it into
a shared one. `git-snapshots/.gate`, held exclusively for these few steps, keeps another instance
from slipping in between. If the lock cannot be taken, nothing is cleaned up.

The clean-up: temporary files and staging folders older than 10 minutes go; a side repository
without `hangar-source` goes; one whose repository's objects folder is gone goes only when the
repository's drive letter is there (the repository was deleted) or its newest snapshot is older
than 30 days, so a repository on an unplugged drive, a network share or in WSL keeps its
snapshots (UNC paths and Unix mount points are not probed: a share may hang, an empty mount point
stays). With the keys of `workspace.json` (`Store::workspace_keys`), the snapshots of every other
key are deleted, a side repository left without any goes as a whole, the others run `git gc
--quiet --prune=1.hour.ago` with the user's objects as an alternate (packs only the side
repository's own objects; objects younger than an hour stay, so a snapshot written meanwhile keeps
its objects). Last, all side repositories together are kept below 2 GiB: those whose newest
snapshot is oldest go as a whole first; the last one left instead loses the older half of each
session's snapshots and runs `gc` again. A `workspace.json` that exists but cannot be read keeps
every snapshot.

## Going back (`snapshot_restore`)

Every check comes before anything is written, and before the undo point is saved. One jump back
runs at a time in a Hangar, whichever session asks (`Snapshots::restoring`).

1. Refused while a session whose folder lies in the same work tree (this one included) is working
   (`Working`). The backend takes the states the window reports for the team's presence
   (`team_session_states`, at most one second old, `TeamService::working_sessions`); sessions
   count when their folder (canonicalized) lies in the work tree's root (`key_within`). The states
   are read again for the last look in step 9.
2. The current tree is built as in a snapshot. Equal to the target: `alreadyThere`. Too many or
   too large changed files, or a git error: `CannotSaveCurrent`, nothing is written.
3. `diff-tree -r -z --raw --no-abbrev --no-renames --ignore-submodules=all <current> <target>`
   (submodule entries, mode 160000, are left out): every path is checked with `check_path` (no
   `..`, no drive, no `.git` in any case, no NTFS short name `git~<n>`, no `.git` with code points
   HFS+ ignores) and `refuse_links`. A file to write with mode 120000 refuses the jump
   (`SymlinkEntry`): git would write a link whose target the snapshot names. Test
   `restore_refuses_a_symbolic_link_in_the_snapshot`.
4. Paths the user's index marks skip-worktree (`ls-files -t`, `S`) are left out: a sparse
   checkout keeps them out of the work tree on purpose.
5. With a filter driver configured, a file to write whose `filter` attribute names one refuses
   the jump (`refuse_filtered_files`): without its smudge filter it would get the wrong content.
6. A file to write where a folder stands that holds anything besides files being deleted refuses
   the jump (`FolderInTheWay`). An ignored file (in no snapshot, so not in the undo point either)
   where the target adds a file, or where the target needs a folder, refuses it too
   (`IgnoredInTheWay`). A file deleted before the write is no obstacle; on a file system that
   ignores case this includes a deleted path spelled differently (`README.md` to `Readme.md`).
   Test `restore_refuses_to_overwrite_an_ignored_file`.
7. The files to write are staged: `read-tree <target>` into a temporary index and
   `checkout-index --force -z --stdin --prefix=<local data>/git-snapshots/tmp/<pid>-<n>.stage/`
   with `core.longpaths=true`, so git converts them (line endings, `ident`) as in the work tree,
   which it never touches ([[timeline-restore-measurement]], test
   `restore_writes_line_endings_as_git_does`). A failure here changes nothing.
8. The current state is recorded as an `undo` snapshot (also when it equals HEAD).
9. The last look: the working states are read again, and the work tree's tree is built once more;
   a session that started working, or a tree that differs from step 2 (a file saved during the
   checks), answers `Working` before anything is written (the undo point stays, equal to the
   state then). Test `restore_stops_when_the_work_tree_changes_meanwhile`.
10. Files the target lacks are deleted, `refuse_links` again right before each delete and the
    canonical parent checked as below, then their folders while empty (a folder with an ignored
    file stays). The folder and ignored file checks run again (now without exemptions). Then each
    staged file is installed by Hangar itself (`install`): every folder on the way is created if
    missing and must be a plain folder (no link, no junction, no reparse point, by
    `symlink_metadata`); the deepest one, canonicalized (`fs::canonicalize`: links resolved, long
    names, which also covers NTFS short names like `GI7EBA~1`), must lie inside the canonical root
    and outside the canonical `--git-dir` and `--git-common-dir` (`Outside`); a file already at
    the target must canonically be the one the path names (a short name `README~1.MD` names
    another file, `Outside`); a folder or a link at the target refuses. The content is copied into
    a new file `.hangar-<pid>-<n>.restore` beside the target (`create_new`, never through a link,
    with the staged file's permissions, so the executable bit) and renamed over the target: a
    rename replaces a file, never a folder, and never follows a link at the target. A failure here
    answers `incomplete` with the undo number: the window says the state before was saved. Test
    `install_checks_every_folder_right_before_writing`; the deletes and the files written
    before the failure stay done, test `a_failed_write_stops_and_names_the_file`.

Why staging and installing instead of `checkout-index` into the work tree, as before:
`checkout-index --force` deletes a folder in the way and writes through whatever the path names at
that moment, so checks before it only narrow the window, and re-running `refuse_links` for all
written paths would still leave the time of the whole checkout between check and write. Installing
each file itself moves every check right next to the write of that file and replaces
`--force`'s folder deletion by a refusal. Staging in Hangar's data folder and copying (instead of
renaming the staged file) works across volumes. The window left is the time between the canonical
check of one folder and the rename of one file into it.

Ignored files, the index, HEAD and the branch stay. The confirmation dialog names the work tree's
root, says what stays and, when other sessions work in the same work tree, that their files go
back too; when the target's parent is not the current HEAD (`headMoved`: a branch switched or a
commit made since) it warns that only the files go back, not the commit. "Go back to this state"
and the dialog's confirm button are disabled while the session or the list's `busy` says an agent
works there; opening the dialog reads the list again. The undo snapshot is listed at the top as
"Before going back" and can be restored like any other.

## Residual risks

- The working states are up to one second old, and the last look in step 9 takes a moment: an
  agent that starts writing within that window, or a file saved after the second tree was built,
  can still be overwritten. The undo point does not hold such a change.
- Two Hangar instances (an installed one and a debug build) can each run a jump back in the same
  work tree at once; within one Hangar they run one after the other, also from two sessions.
- A parent folder swapped for a junction between its canonical check and the rename of one file.
- The oldest snapshot as target: its parent (the HEAD back then) and unchanged blobs may exist only
  in the user's objects. After a history rewrite and a `git gc` in the user's repository, its diff
  and the jump back fail with git's message, and the side repository's `gc` fails and deletes
  nothing.
- `git gc` in the side repository runs at every start that cleans up; a large side repository
  makes that clean-up slower (it runs on its own thread, snapshot calls wait for it).
- Git for Windows without long path support in the staging folder: a deep path fails in step 7,
  before anything is written.

## Commands

| Command | Windows | Answer |
| --- | --- | --- |
| `snapshot_bind(id, key)` | main | `null` |
| `snapshot_take(id, prompt?)` | main | `{kind: created, number, files} \| unchanged \| notARepository \| tooManyFiles(limit) \| tooLarge(fileMib, totalMib)` |
| `snapshot_list(id)` | main, panel | `{isRepo, snapshots[] newest first (each with parent: sha or null), root, head: sha or null, skipped: null \| {reason: tooManyFiles, limit} \| {reason: tooLarge, fileMib, totalMib} \| {reason: failed, message}, others, busy}` |
| `snapshot_diff(id, number)` | main, panel | `GitDiff`, against the previous snapshot, the first against its parent |
| `snapshot_restore(id, number)` | main, panel | `{kind: restored, files, undo} \| alreadyThere \| {kind: incomplete, undo, message}` |
| `snapshot_forget(id)` | main | `null` |

The folder always comes from the session manager (through the binding); a call for a session
without a binding fails with `UnboundSession`. Calls of one session are serialized by its own lock
(`Snapshots`), so a slow snapshot of one session holds up neither another session's snapshot nor
its closing. Jumps back run one at a time per Hangar, whichever session (`restoring`, test
`jumps_back_run_one_at_a_time`). `Snapshots` also remembers in memory why a session's newest step was not saved.
Every change emits `snapshots-changed` with the session ID to the main and the panel window
(`emit_to`), not to the viewer windows.

Errors reach the window as text. The refusals carry a stable code: `[code] English text`, and for
those that name a path or reason `[code] English text` plus a newline and the detail (the English
text never holds a newline; the detail is everything after the first one). Codes:
`snapshot_working`, `snapshot_folder_in_the_way`, `snapshot_ignored_in_the_way`,
`snapshot_cannot_save_current` (detail: git's reason), `snapshot_linked`, `snapshot_filtered`,
`snapshot_symlink`, `snapshot_too_large_to_check`, `snapshot_outside`. `splitBackendError`
(`src/backendError.ts`) takes them apart, `timelineErrorText` (`src/timeline.ts`) shows them in
the window's language; an unknown code or none falls back to the generic text.
