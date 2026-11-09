---
title: "Measurement: cost of git status and git diff -U0 for the claims rescan, notify 8.2 on Windows"
type: source
tags: [git, windows, claims, notify, measurement, performance]
sources: []
raw: raw/2026-11-09-claims-rescan-measurement.md
url:
retrieved: 2026-11-09
created: 2026-11-09
updated: 2026-11-09
---

# Measurement: git status and git diff -U0 for the claims rescan

Own measurement on Windows 11 (build 26300), git 2.56.0.windows.4, through the hardened
`Git::run` (fsmonitor off, `GIT_OPTIONAL_LOCKS=0`), on Hangar's own repository (382 files) and on
a synthetic repository with 50 000 tracked files. Harness: `src-tauri/tests/claims_measure.rs`
(ignored test, run by hand). In addition, the notify 8.2.2 source from the Cargo registry.

## Key points
- Starting git costs about 35 ms per call on Windows, whatever it does. A rescan (filter config,
  status, diff) takes about 125 ms on 382 files and about 150 ms on 50 000 files while the index
  is up to date.
- After every file's modification time changed and nobody refreshed the index, status takes about
  1.3 s and diff about 0.35 s on 50 000 files, and stays that slow on every call: with optional
  locks off, status never writes the refreshed index back. The agent's own git commands or the
  user's refresh it.
- notify 8.2.2 on Windows (`windows.rs`, `handle_event`): an unknown ReadDirectoryChangesW error,
  which includes a buffer overflow, only logs and unwatches the folder. The event handler gets no
  error and no rescan flag, so a watch can end silently. Buffer size 16 384 bytes.

## Significance for Hangar
- Rescan timing in `src-tauri/src/claims.rs`: 750 ms quiet, 3 s maximum wait, a gap after each
  rescan of ten times its cost (at least 1 s, at most 30 s), so git stays below about a tenth of
  one core even in the stale 50 000 file case (about 12 s between rescans). Safety poll 60 s with
  a watcher, 30 s without.
- Because of the silent unwatch, a poll that finds changes no event reported restarts the
  watcher (`claims_service.rs`).
- The repository identity (`rev-parse`) is resolved once per folder, not on every rescan.
- Raw data: `raw/2026-11-09-claims-rescan-measurement.md`. Plan and as-built notes:
  [[team-service-v1]].
