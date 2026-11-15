---
title: git linked worktree commondir measurement
type: source
tags: [git, worktree, measurement]
sources: []
created: 2026-11-14
updated: 2026-11-14
raw: raw/2026-11-14-git-worktree-commondir-measurement.md
url: ""
retrieved: 2026-11-14
---

# git linked worktree commondir measurement

Own measurement on Windows 11 10.0.26300 with git 2.56.0.windows.4; record in
[[2026-11-14-git-worktree-commondir-measurement]].

## Key points

- `git worktree add` writes a `.git` file into the new worktree that names an absolute git folder
  with forward slashes on Windows: `gitdir: E:/.../main/.git/worktrees/wt`.
- That git folder holds `commondir` with `../..`, relative to itself: the main repository's
  `.git` folder.
- The parent of that `.git` folder is the repository's main folder.

## Significance for Hangar

`git_repo_places` follows `gitdir:` and `commondir` by text alone to put a linked worktree on the
card of its repository, without starting git. See [[project-overview]].
