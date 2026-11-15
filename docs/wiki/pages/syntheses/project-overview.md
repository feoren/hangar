---
title: Project overview (home view)
type: synthesis
tags: [home-view, git, team, dev-server, contract]
sources: ["[[git-worktree-commondir-measurement]]"]
created: 2026-11-14
updated: 2026-11-14
---

# Project overview (home view)

The home view (house button in the sidebar, View > "Project overview", or the same entry in quick
access) shows one card per project above the activity calendar. Code: `src/projectCards.ts`
(pure rules, tests in `src/projectCards.test.ts`), `src/ProjectOverview.tsx` (cards, tests in
`src/ProjectOverview.test.tsx`), backend command `git_repo_places` in `src-tauri/src/git.rs`.

## Why the home view

The home view already is the place for "what is going on across my work" (the calendar of the
user's own commits). The overview is the live half of that: it sits above the calendar, the
sessions keep running while it shows (`putAside`), and a click on a session row brings the
sessions back with that one active. No new shortcut: the home shortcut already opens the view,
and the menu entry makes it findable in quick access.

## Projects

A project is the repository's main folder, or the plain folder for a session outside git.

| Session | Project | Checkout |
|---|---|---|
| own worktree (team or private, `worktree.ts`) | `worktree.repoRoot` | `worktree.path` |
| folder inside a repository (root or subfolder) | `root` from `git_repo_places` | `workTree` from `git_repo_places` |
| linked worktree made by hand | the repository it belongs to (`commondir`) | the worktree folder |
| submodule | the submodule itself | the submodule |
| no repository | the folder | none |

Update sessions run in the home folder on purpose and are left out. Folders compare by
`folderKey` (case and separators on Windows). A folder without an answer yet counts as a plain
folder until the answer arrives.

## `git_repo_places`

`paths: string[]` (at most 64) to `Record<string, { root, workTree } | null>`. Read from the files
under `.git` without a process, like `git_branches`: the nearest `.git` folder makes its parent
both `root` and `workTree`; a `.git` file names the git folder (`gitdir:`), whose `commondir`
names the shared `.git` folder, and that one's parent is `root`. Without `commondir` (submodule)
or when the shared folder is no `.git` folder (bare repository) the checkout is its own root. The
same rule as for branches keeps a local folder from leading to a network share
(`gitdir_allowed`). The file layout was measured with git 2.56.0.windows.4
([[git-worktree-commondir-measurement]]).

The existing commands did not fit: `list_session_folder` names the repository root but lists the
folder, runs `git check-ignore` and holds a per-session claim the Files tab uses; `git_log` and
`git_changed_count` start git and do not name the root; a linked worktree's root is its own
checkout there, not the repository.

## A card

- Name (folder name of the project) and full path.
- Sessions with a dot in the state's color and the state in words (the overview tiles' text), so
  color is never the only signal. A click activates the session.
- Open changes: `git_changed_count` per checkout, summed. A click opens the Changes tab of the
  first session in the first checkout with changes. Hidden when git answered for no checkout.
- The newest 3 commits of the project's main folder (`git_log`, see [[commit-history]]), with
  their age; hidden when there are none.
- Team: the teammates in the same repository (`peersOf`, `repoTeammates`) and the reservations of
  the team projects the card's sessions are shared with, at most 3 and a count. Hidden without a
  team or without anything to show.
- Dev servers of the card's sessions, each address once; the globe opens it like the globe on a
  session row (`open_local_url`).

## When git is asked

Only while the home view shows, and per card only once it has been in view (IntersectionObserver):
when the view opens, on "Refresh projects" and when one of the card's sessions finished a stretch
of work the user asked for (the same moment as the done notice). Never on a timer and never per
output chunk. All cards share one queue of at most 3 git calls at a time (`createLimiter`).
`git_repo_places` is asked when the view opens, when the set of folders changes and on refresh.

## Empty state

Without a session in a folder: "No project open yet" with a hint and, when a program can start,
"New session" (the same action as the new session shortcut).
