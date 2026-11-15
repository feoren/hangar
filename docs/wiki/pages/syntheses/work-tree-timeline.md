---
title: Work tree timeline (contract)
type: synthesis
tags: [git, changes-panel, contract, security]
sources: []
created: 2026-11-13
updated: 2026-11-13
---

# Work tree timeline (contract)

"Time travel per agent step": after every step of an agent session Hangar quietly saves the state
of the session's work tree, the "Timeline" view of the Changes tab lists those snapshots, shows
what each step changed and goes back to any of them. Backend `src-tauri/src/git_snapshots.rs`,
commands in `src-tauri/src/commands.rs` (`snapshot_*`), frontend `src/timeline.ts` (pure logic)
and `src/TimelineView.tsx`.

## Trigger

The main window calls `snapshot_take` on the same transition that offers the done notice
(`finishedWork` in `src/doneNotice.ts`: working to idle or waiting, not the gate, and only after
the user typed), for agent sessions only (`shellKind` "agent", no update sessions, no
placeholders). It passes the session's workspace key (`SavedSession.key`, a UUID that survives
restarts) and the last prompt (`terminal.lastPrompt()`, cut to 200 characters by both sides).

## Snapshot (what `git stash create` does, without touching anything)

1. Folder checks of `git.rs` (`check_folder`, `toplevel`), filter drivers switched off
   (`filter_overrides`), `core.hooksPath` at Hangar's empty hooks folder, `core.splitIndex=false`,
   `core.logAllRefUpdates=false`. One time limit of 30 s for the whole operation.
2. `git status --porcelain=v1 -z --untracked-files=all`: more than 2000 entries (or a list cut at
   4 MiB) skips the step; the timeline says so ("tooManyFiles").
3. The user's index (`rev-parse --git-path index`) is copied to a temporary file below
   `<app local data>/git-snapshots/`; with `GIT_INDEX_FILE` pointing there, `git add --all` and
   `git write-tree` give the work tree's tree. Ignored files never go in; tracked files stay even
   when ignored. The copy keeps the stat cache, so only changed files are hashed.
4. The tree is compared with the newest snapshot's tree (for the first step: HEAD's tree). Equal:
   no snapshot.
5. `commit-tree --no-gpg-sign <tree> -p HEAD -F -` with Hangar's own identity
   (`Hangar <snapshots@hangar.invalid>`) and the subject `hangar-snapshot <step|undo> <files>
   <prompt>`; then one `update-ref --stdin` transaction creates
   `refs/hangar/snapshots/<key>/<n>` and deletes the oldest beyond 50.

HEAD, the branch, the index, `refs/stash` and every reflog stay as they were (test
`a_snapshot_touches_nothing_of_the_users`). End of line conversion works as with `git stash`:
text attributes and `core.autocrlf` apply on the way in and out.

## Where the refs go (and where not)

Measured in `snapshot_refs_stay_local` with the installed git: neither `git clone` (regular
transport), `git fetch`, `git push <remote> main`, `git push --all` nor `git push --tags` carries
`refs/hangar/`. The default fetch refspec covers `refs/heads/*` only, `--all` pushes branches and
`--tags` tags. `git push --mirror` does carry them (measured in the same test): a mirror push
publishes the snapshots of every open session.

> [!question] Unverified
> A refspec like `refs/*:refs/*` pushes them as well, and a `git clone --mirror` of the local
> repository copies them. `git log --all` and `gitk --all` list the snapshot commits, like
> `refs/stash`. `git gc` keeps the objects of existing refs; pruned refs free theirs at the next
> gc.

## Lifetime

At most 50 snapshots per session; numbers count up and are never reused. Closing a session deletes
all of its refs (`snapshot_forget`, called before `close_session` while the backend still knows
the folder); this also covers the session's own worktree, whose refs live in the shared
repository. Quitting Hangar keeps them: the restored session has the same key. A session that
ends in a crash leaves its refs, bounded by the cap.

## Going back (`snapshot_restore`)

1. The current tree is built as in a snapshot. Equal to the target: `alreadyThere`, nothing is
   written.
2. Otherwise the current state is recorded as an `undo` snapshot first (also when it equals HEAD);
   if that fails (too many files, git error) nothing is written (`CannotSaveCurrent`).
3. `diff-tree -r -z --no-renames --name-status --ignore-submodules=all <current> <target>`:
   every path is checked with `check_path` and `refuse_links` of `git_actions.rs`, so a path
   through or holding a link (symbolic link, junction) refuses the whole jump before any write.
4. Files the target lacks are deleted, then their folders while empty (a folder with an ignored
   file stays). The others are written with `read-tree <target>` into a temporary index and
   `checkout-index --force -z --stdin`, filters and hooks off.

Ignored files, the index, HEAD and the branch stay. The confirmation dialog says this; while the
session's agent is working it adds that what the agent writes afterwards mixes with the old state
(going back is not blocked). The undo
snapshot is listed at the top as "Before going back" and can be restored like any other.

## Commands

| Command | Windows | Answer |
| --- | --- | --- |
| `snapshot_take(id, key, prompt?)` | main | `{kind: created, number, files} \| unchanged \| notARepository \| tooManyFiles(limit)` |
| `snapshot_list(id, key)` | main, panel | `{isRepo, snapshots[] newest first, skipped: null \| {reason: tooManyFiles, limit} \| {reason: failed, message}}` |
| `snapshot_diff(id, key, number)` | main, panel | `GitDiff`, against the previous snapshot, the first against its parent |
| `snapshot_restore(id, key, number)` | main, panel | `{kind: restored, files, undo} \| alreadyThere` |
| `snapshot_forget(id, key)` | main | `null` |

The folder always comes from the session manager; the key must be a UUID (lower-cased, the only
text that becomes part of a ref name). Calls of one session are serialized by its own lock
(`Snapshots`), so a slow snapshot of one session holds up neither another session's snapshot nor
its closing; `Snapshots` also remembers in memory why a session's newest step was not saved. Every change emits
`snapshots-changed` with the key to all windows; the panel window learns the active key through
`panel-context` (`activeKey`).
