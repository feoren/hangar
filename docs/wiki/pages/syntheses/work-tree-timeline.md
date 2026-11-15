---
title: Work tree timeline (contract)
type: synthesis
tags: [git, changes-panel, contract, security]
sources: []
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
sessions only (`shellKind` "agent", no update sessions, no placeholders). The prompt is the last
line the user submitted (`SessionTerminal.lastPrompt()`), cut to 200 characters by both sides.

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
> git's message; the clean-up's `prune` then fails too and deletes nothing.

## Snapshot (what `git stash create` does, without touching anything)

1. Folder checks of `git.rs` (`check_folder`, `toplevel`), filter drivers switched off
   (`filter_overrides`), hooks off through git.rs, `core.splitIndex=false`,
   `core.logAllRefUpdates=false`. One time limit of 30 s for the whole operation.
2. `git status --porcelain=v1 -z --untracked-files=all`: more than 2000 entries (or a list cut at
   4 MiB) skips the step ("tooManyFiles"). A changed file above 50 MiB, or all changed files
   together above 200 MiB (sizes on disk, links not followed), skip it too ("tooLarge").
3. The user's index (`rev-parse --git-path index`) is copied to a temporary file below
   `<app local data>/git-snapshots/tmp/`; with `GIT_INDEX_FILE` pointing there, `git add --all`
   and `git write-tree` give the work tree's tree. Ignored files never go in; tracked files stay
   even when ignored. The copy keeps the stat cache, so only changed files are hashed.
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
snapshot call waits for it): temporary indexes older than 10 minutes go; a side repository whose
`hangar-source` is missing or whose repository's objects folder is gone goes as a whole; with the
keys of `workspace.json` (`Store::workspace_keys`), the snapshots of every other key are deleted,
a side repository left without any goes as a whole, the others run `git prune
--expire=1.hour.ago` (objects younger than an hour stay, so a snapshot written meanwhile keeps
its objects). A `workspace.json` that exists but cannot be read keeps every snapshot.

## Going back (`snapshot_restore`)

Every check comes before anything is written, and before the undo point is saved:

1. Refused while a session whose folder lies in the same work tree (this one included) is working
   (`Working`). The backend takes the states the window reports for the team's presence
   (`team_session_states`, at most one second old, `TeamService::working_sessions`); sessions
   count when their folder (canonicalized) lies in the work tree's root (`key_within`).
2. The current tree is built as in a snapshot. Equal to the target: `alreadyThere`. Too many or
   too large changed files, or a git error: `CannotSaveCurrent`, nothing is written.
3. `diff-tree -r -z --no-renames --name-status --ignore-submodules=all <current> <target>`:
   every path is checked with `check_path` (no `..`, no drive, no `.git` in any case, no NTFS
   short name `git~<n>`, no `.git` with code points HFS+ ignores) and `refuse_links`.
4. Paths the user's index marks skip-worktree (`ls-files -t`, `S`) are left out: a sparse
   checkout keeps them out of the work tree on purpose.
5. With a filter driver configured, a file to write whose `filter` attribute names one refuses
   the jump (`refuse_filtered_files`): without its smudge filter it would get the wrong content.
6. A file to write where a folder stands that holds anything besides files being deleted refuses
   the jump (`FolderInTheWay`): `checkout-index --force` would delete that folder.
   An ignored file (in no snapshot, so not in the undo point either) where the target adds a
   file, or where the target needs a folder, refuses it too (`IgnoredInTheWay`): `--force` would
   overwrite or delete it. A file deleted before the write is no obstacle; on a file system that
   ignores case this includes a deleted path spelled differently (`README.md` to `Readme.md`).
   Test `restore_refuses_to_overwrite_an_ignored_file`.
7. The current state is recorded as an `undo` snapshot (also when it equals HEAD).
8. Files the target lacks are deleted, `refuse_links` again right before each delete, then their
   folders while empty (a folder with an ignored file stays). The folder and ignored file checks
   run again (now without exemptions), then the others are written with `read-tree <target>`
   into a temporary index and `checkout-index --force -z --stdin`, filters and hooks off. A failure here answers
   `incomplete` with the undo number: the window says the state before was saved.

Ignored files, the index, HEAD and the branch stay. The confirmation dialog says this and, when
other sessions work in the same work tree, that their files go back too. "Go back to this state"
and the dialog's confirm button are disabled while the session or the list's `busy` says an agent
works there; opening the dialog reads the list again. The undo snapshot is listed at the top as
"Before going back" and can be restored like any other.

## Commands

| Command | Windows | Answer |
| --- | --- | --- |
| `snapshot_bind(id, key)` | main | `null` |
| `snapshot_take(id, prompt?)` | main | `{kind: created, number, files} \| unchanged \| notARepository \| tooManyFiles(limit) \| tooLarge(fileMib, totalMib)` |
| `snapshot_list(id)` | main, panel | `{isRepo, snapshots[] newest first, skipped: null \| {reason: tooManyFiles, limit} \| {reason: tooLarge, fileMib, totalMib} \| {reason: failed, message}, others, busy}` |
| `snapshot_diff(id, number)` | main, panel | `GitDiff`, against the previous snapshot, the first against its parent |
| `snapshot_restore(id, number)` | main, panel | `{kind: restored, files, undo} \| alreadyThere \| {kind: incomplete, undo, message}` |
| `snapshot_forget(id)` | main | `null` |

The folder always comes from the session manager (through the binding); a call for a session
without a binding fails with `UnboundSession`. Calls of one session are serialized by its own lock
(`Snapshots`), so a slow snapshot of one session holds up neither another session's snapshot nor
its closing; `Snapshots` also remembers in memory why a session's newest step was not saved.
Every change emits `snapshots-changed` with the session ID to all windows.
