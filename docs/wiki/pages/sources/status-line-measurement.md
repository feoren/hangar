---
title: "Measurement: status line commands and Codex tui.status_line"
type: source
tags: [measurement, status-line, codex, claude-code, windows]
sources: []
raw: raw/2026-11-27-status-line-measurement.md
url: https://github.com/openai/codex/tree/rust-v0.172.0/codex-rs
retrieved: 2026-11-27
created: 2026-11-27
updated: 2026-11-27
---

# Measurement: status line commands and Codex tui.status_line

Windows 11, Codex CLI 0.172.0, Claude Code 2.1.332; Qwen Code not installed.

## Key points
- Codex CLI 0.172.0 parses `-c tui.status_line=[...]` in its bootstrap configuration: a number
  fails with "expected a sequence in `tui.status_line`", a list of ids is taken (unknown ids too,
  at this stage). The rendered TUI line was not looked at.
- Every id ADR 0023 uses is present in the installed binary: `model-with-reasoning`,
  `current-dir`, `git-branch`, `branch-changes`, `pull-request-number`, `context-used`,
  `used-tokens`, `five-hour-limit`, `weekly-limit`.
- Hangar's `status-line` renderer: 385 ms with a fresh git reading, 26 ms from its 5 s cache
  (debug build); garbage on stdin prints nothing and exits 0; `COLUMNS=60` drops segments until
  both lines fit.
- The system drive keeps 8.3 short names (`C:\PROGRA~1\...`); another drive here does not.

## Significance for Hangar
Evidence for [[0023-hangar-status-line]]: Codex is offered its built-in items, Claude Code's
command falls back to the short name for a path with blanks and leaves the status line out when
there is none.
