# git linked worktree: `.git` file and `commondir`

Date: 2026-11-14. Windows 11 Pro 10.0.26300, git 2.56.0.windows.4, folder under `tests/.tmp/`.

Commands:

```
git init -q main
cd main
git -c user.name=x -c user.email=x@x commit -q --allow-empty -m init
git worktree add -q ../wt
cat .git/worktrees/wt/commondir
cat ../wt/.git
```

Output (project path shortened to `<project>`):

```
../..
gitdir: E:/<project>/tests/.tmp/m/main/.git/worktrees/wt
```

Result: the worktree's `.git` file names an absolute git folder with forward slashes on Windows,
and that folder's `commondir` names the main repository's `.git` folder relative to itself
(`../..`). The main folder of the repository is the parent of that `.git` folder.
