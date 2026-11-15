# git worktree remove on a folder deleted by hand

Date: 2026-11-14. Windows 11 Pro 10.0.26300, git 2.56.0.windows.4, Git Bash, folders under
`tests/.tmp/probe/`.

Setup: repository `r` with one empty commit; `git worktree add -b hangar/x-1a2b3c ../w HEAD` and
`git worktree add -b other ../o HEAD`; then both folders deleted with `rm -rf ../w ../o`.

`git worktree list --porcelain -z` (NUL shown as line breaks):

```
worktree E:/.../probe/r
HEAD dc18ad9d90e527a7ce97e108628452a8f0bb9691
branch refs/heads/main

worktree E:/.../probe/o
HEAD dc18ad9d90e527a7ce97e108628452a8f0bb9691
branch refs/heads/other
prunable gitdir file points to non-existent location

worktree E:/.../probe/w
HEAD dc18ad9d90e527a7ce97e108628452a8f0bb9691
branch refs/heads/hangar/x-1a2b3c
prunable gitdir file points to non-existent location
```

`git worktree remove ../w` without `--force`: exit 0, no output. Afterwards the list holds `r` and
`o` (still `prunable`); `.git/worktrees/w` is gone. So `worktree remove` takes away the record of
exactly that worktree, while `git worktree prune` would take every prunable record.

A locked worktree prints `locked` (or `locked <reason>`) as its own field. `git branch -D other`
while `o` is only a prunable record: `error: cannot delete branch 'other' used by worktree at
'E:/.../probe/o'`.

Paths are printed with forward slashes on Windows. Records end with an empty field.

Repeated in the Rust tests `worktree::tests::remove_after_the_folder_was_deleted_by_hand` and
`worktree_cleanup::tests::cleanup_with_real_repositories` (`cargo test --manifest-path
src-tauri/Cargo.toml worktree`): ok.
