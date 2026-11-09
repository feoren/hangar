# Measurement: git status and git diff -U0 for the claims rescan (2026-11-09)

Platform: Windows 11 Pro, build 26300, local NTFS drive. git 2.56.0.windows.4. Rust 1.99.1, debug
build of the test (the time is spent in the git processes, not in Rust).

Harness: `src-tauri/tests/claims_measure.rs` (ignored test), run as

```
HANGAR_MEASURE_REPOS="<worktree>;<worktree>/tests/.tmp/measure-50k" \
  cargo test --manifest-path src-tauri/Cargo.toml --test claims_measure -- --ignored --nocapture
```

Every call goes through `hangar_lib::git::Git::run`, so with `core.fsmonitor=false`,
`core.quotepath=false`, `color.ui=false`, `GIT_OPTIONAL_LOCKS=0`, `GIT_TERMINAL_PROMPT=0` and all
inherited `GIT_*` variables removed. Status and diff also carry the `-c filter.<name>.*=` overrides
from `filter_overrides` (the user's global config names one filter driver). Seven rounds per call,
back to back.

Commands:

- `config -z --get-regexp ^filter\..*\.(clean|smudge|process)$`
- `rev-parse --show-toplevel --git-dir --git-common-dir`
- `status --porcelain=v2 -z --branch --no-renames --untracked-files=normal --ignore-submodules=all`
- `diff -U0 --no-color --no-ext-diff --no-textconv --no-renames --ignore-submodules=all HEAD --`
- "whole rescan": filter config, status and diff in a row, as the claims worker runs them.

Synthetic repository: `tests/.tmp/measure-50k`, 250 folders with 200 text files of 40 lines each
(50 000 tracked files), one commit; then 20 files changed in 3 places each and 5 untracked files.
Creating and committing it took 173 s.

## 1. Warm, index up to date

```
git version 2.56.0.windows.4
<worktree> (382 tracked files)
  config filters (filter_overrides)  min    34.5 ms  median    35.7 ms  max    37.0 ms  (118 bytes out)
  rev-parse identity                 min    35.0 ms  median    38.0 ms  max    61.7 ms  (161 bytes out)
  status --porcelain=v2 -z --branch  min    40.2 ms  median    45.0 ms  max    52.5 ms  (392 bytes out)
  diff -U0 HEAD                      min    42.9 ms  median    47.1 ms  max    51.7 ms  (2549 bytes out)
  whole rescan (3 calls)             min   117.1 ms  median   126.4 ms  max   160.8 ms  (0 bytes out)
<worktree>/tests/.tmp/measure-50k (50000 tracked files)
  config filters (filter_overrides)  min    35.2 ms  median    36.1 ms  max    41.3 ms  (118 bytes out)
  rev-parse identity                 min    34.5 ms  median    35.0 ms  max    36.5 ms  (95 bytes out)
  status --porcelain=v2 -z --branch  min    53.6 ms  median    63.2 ms  max    64.5 ms  (2883 bytes out)
  diff -U0 HEAD                      min    48.0 ms  median    49.7 ms  max    51.5 ms  (8160 bytes out)
  whole rescan (3 calls)             min   143.5 ms  median   148.4 ms  max   195.6 ms  (0 bytes out)
```

## 2. Stale stat data: every file's modification time changed, index not refreshed

All 50 000 files got a new modification time (`os.utime`), as after a checkout or a tool that
rewrites files. `GIT_OPTIONAL_LOCKS=0` keeps status from writing the refreshed index back, so
every following call hashes the files again.

```
<worktree>/tests/.tmp/measure-50k (50000 tracked files)
  config filters (filter_overrides)  min    35.5 ms  median    50.7 ms  max    70.8 ms  (118 bytes out)
  rev-parse identity                 min    36.6 ms  median    37.2 ms  max    39.7 ms  (95 bytes out)
  status --porcelain=v2 -z --branch  min  1224.7 ms  median  1294.9 ms  max  1344.8 ms  (2883 bytes out)
  diff -U0 HEAD                      min   301.0 ms  median   350.4 ms  max  5540.8 ms  (8160 bytes out)
  whole rescan (3 calls)             min   951.2 ms  median  1151.9 ms  max  1345.8 ms  (0 bytes out)
```

The 5.5 s maximum of the diff was the first round after the status rounds.

## 3. notify 8.2.2 on Windows, read from the crate source

`notify-8.2.2/src/windows.rs`, `handle_event` (the ReadDirectoryChangesW completion routine):
any error code other than `ERROR_SUCCESS`, `ERROR_OPERATION_ABORTED` and `ERROR_ACCESS_DENIED`
(with the folder still present) is logged through the `log` crate and the folder is unwatched.
No error reaches the event handler and no event carries the rescan flag. The buffer is 16 384
bytes (`BUF_SIZE`).
