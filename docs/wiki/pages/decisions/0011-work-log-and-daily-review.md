---
title: "ADR 0011: Work log for the daily review"
type: decision
status: accepted
superseded_by:
tags: [daily-review, persistence, privacy, git]
sources: []
created: 2026-11-14
updated: 2026-11-20
---

# ADR 0011: Work log for the daily review

## Context
The daily review shows one day: which sessions ran, how long they worked, in which projects,
which commits came out of it and which files those commits touched. Sessions end and are closed;
`workspace.json` ([[session-restore]]) only knows the sessions of the current window, so the
review needs a record of its own. The record names folders and session titles of this machine
and must never become a copy of the terminal.

## Options
1. **Derive everything from git.** Commits and files, but no sessions, no times, nothing for
   folders that are no repository.
2. **Read the agents' transcripts.** Only some agents keep them ([[subagent-transcripts]]), they
   hold prompts and output, and shells have none.
3. **A small append-only log written by the backend**, with working phases reported by the
   window, which already knows when a session works (the attention state).

## Decision
Option 3, a work log in `app_local_data_dir()/activity/` (Windows `%LOCALAPPDATA%\<identifier>`),
local like the notes and the scrollback: it names this machine's folders. Folder and files are
created for the user only (0700 and 0600 on Unix, like the notes; on Windows the profile's ACL).

- **Files:** one JSON Lines file per UTC day, `YYYY-MM-DD.jsonl`, append only. UTC names keep time
  zones out of the backend: the window computes its local day as `[start, end)` in Unix ms
  (23 or 25 hours on a daylight saving change), and the backend reads the files of that window
  plus three days before, so a session started earlier keeps its start.
- **Records:** `v` (format 1), `event` (`start`, `phase`, `end`), `key` (`<app run>-<session id>`),
  `at` (Unix ms; a phase's end), `from` (a phase's start), `title` (control characters removed,
  at most 200 characters), `program` (display name), `cwd`, `repo` (main work tree root; for a
  linked worktree the repository it belongs to, found from the `.git` entries without running
  git; the `.git` file is read as a regular file of at most 4 KiB, and a `gitdir:` on another
  network share or device than the folder is not followed, the folder then counts as its own
  repository), `worktree`. Every record carries all details, so a day file stands on its own.
- **Validation:** `log_work_phase` takes only a known session id and two times; the phase must
  be positive, at most 48 hours long, end no later than a minute from now and no earlier than
  48 hours ago, so the window can neither write text nor reach an old day file.
- **Writers:** the backend writes `start` when a session's process starts (also after a restart),
  `end` when the process exits, the session closes or the app quits. The window reports a
  working phase once, when it ends (`log_work_phase`: session id and two times; the backend adds
  the details), never per output chunk. The phase ends at the program's last output, not when
  the quiet time noticed it. Update sessions are not logged.
- **Messages:** a cross-project message ([[0012-cross-project-messages]]) adds `message` with
  the target's `toKey` and `toCwd`; a message delivered inside a link group
  ([[0014-session-links]]) adds `linkMessage` with the same fields and the sender's `member`
  name. Neither carries the text, and the review counts neither as work. A `linkMessage` is
  checked like a phase: a valid member name and a time no later than a minute from now and no
  earlier than 48 hours ago.
- **Never stored:** terminal output, prompts, environment variables, secrets, file contents.
- **Titles are kept, on purpose.** Without them the review cannot say which session did what.
  A title Hangar derives from the first prompt ([[agent-workflow-features]]) is the start of
  its first line, at most 48 characters (`PROMPT_TITLE_MAX` in `promptTitle.ts`), and the same
  title is already stored in `workspace.json`. What the log adds is retention: the title stays
  for 90 days after the session closed. A title the user typed is the user's own name for the
  session. The log cuts any title to 200 characters and removes control characters; the
  prompt itself is never stored. Leaving titles out was rejected: a list of "Claude Code" rows
  is no review.
- **Limits:** a day file stops growing at 4 MiB, longer lines than 16 KiB are skipped when read,
  paths over 4096 bytes are not logged. Retention 90 days: older day files are deleted at startup
  on a thread of their own; other files in the folder stay.

## Review (`daily_review`)
- The window sends the local day; it must be 26 hours at most and lie within the kept days.
- Per session: run time is the union of its process runs within the day, active time the union
  of its working phases. An open run lasts until now for a session that is still running, and
  until its last record otherwise (a crash). Per project and for the day: unions again, so two
  agents working side by side for an hour count one hour.
- A project is the repository root, or the working folder outside a repository.
- git (hardened runner, 20 s for the whole review, at most 24 repositories): the day's commits
  by author time from `git log --branches --tags --remotes --no-merges --name-only`, restricted to
  the repository's `user.email` (fixed string, case-insensitive, whole address); names only,
  no content, so no diff driver, textconv or filter runs. A repository without `user.email`
  shows no commits, like the activity calendar: `--remotes` would otherwise list everybody
  else's fetched work as the user's day. At most 200 commits and 300 files per project. For today only, the
  uncommitted changes (`git status`, filters switched off) of each work tree the sessions ran in.
  A logged repository on a network share is asked only when one of the project's sessions
  worked on that same share, so an older record can never make Hangar reach out to a host
  (on Windows each access sends the user's NTLM credentials).
- "Add to notes" creates a note in the General area ([[0004-notes-per-working-folder]]): a day
  spans several projects, and a note per project would scatter one review over many lists.
  "Copy" puts the same Markdown on the clipboard.

## Consequences
- Active time exists only where the attention state exists: with the attention setting off, no
  phases are recorded and only run times show.
- A process that exits before its start returned its id, or an app that crashes, leaves an open
  run; it ends at its last record.
- Not built: offering the review automatically in the evening. It needs a schedule, a setting and
  a notice that must not compete with the attention notices; left out until asked for.

## Evidence
- Tests with synthetic records under `tests/.tmp/`: writing, size cap, pruning, broken lines,
  UTC day files, merging runs and phases across midnight, restarts, crashes and live sessions
  (`src-tauri/src/work_log.rs`, `src-tauri/src/daily_review.rs`).
- A real test repository (git on PATH, Windows 11): `--since` and `--until` with
  `YYYY-MM-DDTHH:MM:SSZ` select by commit time, the author filter and the author time window
  keep only the user's commits of the day, `--name-only` lists their files, `git status` the
  uncommitted ones (`commits_of_the_day_with_their_files_and_uncommitted_changes`).
- Vitest: local day windows in Europe/Berlin (23, 24, 25 hours), the Markdown, the dialog
  (`src/dailyReview.test.ts`, `src/DailyReviewDialog.test.tsx`).
