# Measurement: status line commands and Codex `tui.status_line` (2026-11-27)

Platform: Windows 11 Pro (build 26300), Git Bash. Codex CLI 0.172.0 (npm), Claude Code 2.1.332.
Qwen Code not installed.

## Codex CLI accepts `tui.status_line` as a `-c` override

```
$ codex -c 'tui.status_line=42' features list
Error: failed to load bootstrap configuration

Caused by:
    invalid type: integer `42`, expected a sequence
    in `tui.status_line`

$ codex -c "tui.status_line=['model-with-reasoning','current-dir','git-branch','pull-request-number','branch-changes','context-used','five-hour-limit','weekly-limit']" features list
agent_message_board                      under development  false
...   (normal listing, exit 0)
```

An unknown id inside the list (`'bogus-item'`) is also accepted at this stage.

## Item ids in the installed binary

`grep -a -o -F` over `@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe`,
counts per id: `branch-changes` 2, `context-remaining` 5, `context-used` 4,
`context-window-size` 2, `current-dir` 4, `five-hour-limit` 4, `git-branch` 4,
`model-with-reasoning` 4, `project-root` 2, `pull-request-number` 2, `run-state` 5,
`thread-name` 3, `used-tokens` 4, `weekly-limit` 4.

## Hangar's renderer (debug build of this change)

Input: a Claude Code style JSON with model, effort, folder (a git worktree), repository, context
42 %, cost 1.25 USD over 25 min, a 5 h limit. `HANGAR_STATUS_DIR` pointed at `tests/.tmp`.

```
$ time (hangar.exe status-line claude < sample.json)        # no cache file yet
real 0m0.385s
$ time (hangar.exe status-line claude < sample.json)        # cache file younger than 5 s
real 0m0.026s
$ echo garbage | hangar.exe status-line claude; echo $?
0                                                             # nothing printed
$ COLUMNS=60 hangar.exe status-line claude < sample.json     # colors stripped
Opus 4.7 │ agent-...
████░░░░░░ 42% │ $1.25 ($3.00/h) │ ◷ 25m 00s │ 5h 31%
```

The cache file held the branch, ahead and behind, staged, modified and untracked counts and the
account's display name and plan; no e-mail.

## 8.3 short names

`for %I in ("C:\Program Files\Git\bin\bash.exe") do @echo %~sI` prints
`C:\PROGRA~1\Git\bin\bash.exe`; on the data drive `E:` the same loop returns the long path
(no short names there).
