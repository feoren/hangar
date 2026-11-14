---
title: git worktree remove on a deleted folder
type: source
tags: [git, worktree, measurement]
sources: []
created: 2026-11-14
updated: 2026-11-14
raw: raw/2026-11-14-worktree-remove-missing-measurement.md
url: ""
retrieved: 2026-11-14
---

# git worktree remove on a deleted folder

Own measurement with git 2.56.0.windows.4 on Windows 11; record in
[[2026-11-14-worktree-remove-missing-measurement]].

## Key points

- `git worktree remove <path>` without `--force` succeeds when the folder was deleted by hand and
  takes away only that worktree's record (`.git/worktrees/<name>`).
- `git worktree prune` would take every prunable record, also those of worktrees the user made.
- `git worktree list --porcelain -z`: fields `worktree`, `HEAD`, `branch`, `locked`, `prunable`,
  `detached`, each ended by NUL, an empty field ends a record; paths with `/` on Windows.
- A branch still named by a prunable record cannot be deleted with `git branch -D`.

> [!question] Unverified
> Older git versions than 2.56 were not measured; whether they accept a missing folder in
> `worktree remove` is open. Hangar then reports git's message and keeps the record.

## Significance for Hangar

Hangar removes a worktree whose folder is gone with `worktree remove`, never with `prune`, so the
user's own records stay. See [[agent-workflow-features]], "Worktree cleanup".
