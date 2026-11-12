---
title: "Measurement: Codex CLI 0.167.2 live run with Hangar's hook recipe"
type: source
tags: [hooks, gate, codex, windows, measurement, live]
sources: []
raw: raw/2026-11-12-codex-gate-live-measurement.md
url:
retrieved: 2026-11-12
created: 2026-11-12
updated: 2026-11-12
---

# Measurement: Codex CLI 0.167.2 live run with Hangar's hook recipe

Three live `codex exec` turns on Windows 11 (Codex 0.167.2 through npm's `codex.cmd`, the
account's default model, low reasoning effort), approved by the owner, in a throwaway git
repository that was deleted afterwards. The start used exactly Hangar's argv recipe
(`--dangerously-bypass-hook-trust -c hooks.PreToolUse=[...]` with literal strings and a bare
path); the hook command was a logging probe instead of `hangar gate-hook`. Record:
`raw/2026-11-12-codex-gate-live-measurement.md`. Answers section 7, question 1 of
[[agent-hooks-per-session-research]] for [[agent-gate-adapters]].

## Key points

- **The hook fires** on every `apply_patch` of a live turn (twice in run 1, for an add and an
  update), with `permission_mode` `bypassPermissions`, through `cmd.exe /C`.
- **`HANGAR_GATE` arrives** in the hook's environment unchanged, despite Codex starting hooks with
  a cleared environment plus its session snapshot.
- **Patch text:** `tool_input.command` holds the whole patch with LF line ends and paths relative
  to `cwd`, for example `*** Begin Patch\n*** Add File: hello.txt\n+hi\n*** End Patch`.
- **Only a JSON deny with exit 0 blocks.** The probe's
  `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny",...}}` with
  exit 0 stopped the patch (`hook: PreToolUse Blocked`, the file was not written, the reason
  reached the model). The same JSON with the reason on stderr and exit 2 showed
  `hook: PreToolUse Failed` and the patch went ahead.

> [!warning] Contradiction
> [[agent-hooks-per-session-research]] (from the docs) lists "exit 2 with the reason on stderr"
> as a block. Measured with 0.167.2: exit 2 together with JSON on stdout counts as a failed hook
> and fails open. Exit 2 with stderr alone was not run.

## Significance for Hangar

- Codex CLI is team-capable from 0.167.2 (`measured = true`, `min_version = "0.167.2"` in the
  catalog).
- The helper answers Codex with the JSON deny and exit 0, nothing on stderr
  (`src-tauri/src/gate_hook.rs`).
