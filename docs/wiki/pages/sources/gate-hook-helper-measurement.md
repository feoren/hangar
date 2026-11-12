---
title: "Measurement: hook helper start on Windows and Codex `-c` hook layer"
type: source
tags: [hooks, gate, codex, gemini, copilot, windows, cmd, powershell, measurement]
sources: []
raw: raw/2026-11-12-gate-hook-helper-measurement.md
url:
retrieved: 2026-11-12
created: 2026-11-12
updated: 2026-11-12
---

# Measurement: hook helper start on Windows and Codex `-c` hook layer

Own measurement on Windows 11 with Codex CLI 0.167.2, Windows PowerShell 5.1, PowerShell 7 and
cmd.exe; record in `raw/2026-11-12-gate-hook-helper-measurement.md`. It answers the parts of
[[agent-hooks-per-session-research]] section 7 that need no live model, for the adapters in
[[agent-gate-adapters]].

## Key points

- **A GUI-subsystem program works as a hook command.** Hangar's release build has no console
  (`windows_subsystem = "windows"`). Started directly (Copilot's `exec`), through
  `cmd.exe /C "<command>"` (Codex) or through `powershell -NoProfile -NonInteractive -Command`
  (Gemini CLI), a probe built that way got its stdin, and its stdout and stderr came back; every
  shell waited for it to end.
- **PowerShell loses the exit code** of such a program: exit 2 arrived as 0, with or without
  `; exit $LASTEXITCODE`. cmd.exe and the direct start pass 2 on. So a deny must also be on
  stdout as JSON, not in the exit code alone.
- **Codex registers a `-c hooks.PreToolUse=[...]` value as a `sessionFlags` hook** with the
  given matcher, command and `timeoutSec` 660 (`hooks/list` of `codex app-server`); its
  `trustStatus` is `untrusted`, so it runs only with `--dangerously-bypass-hook-trust`.
- **Through npm's `codex.cmd`, cmd.exe eats `^`.** The anchored matcher `^apply_patch$` arrived as
  `apply_patch$`. A value without `"`, `^` and other cmd.exe syntax (TOML literal strings, bare
  path, unanchored matcher) arrived unchanged.
- **Console build and UTF-8** (second record, `raw/2026-11-12-gate-hook-powershell-utf8-measurement.md`):
  PowerShell turns exit code 2 of the console (debug) build into 1, and passes UTF-8 stdin and
  stdout (umlauts, CJK, symbols) unchanged. So the Gemini deny goes out as JSON with exit 0.
- Codex accepts the global options before a subcommand (`codex <flags> resume --last`) and refuses
  `--dangerously-bypass-hook-trust` twice.

> [!question] Unverified
> Whether the hook actually fires on a live `apply_patch`, whether `HANGAR_GATE` survives Codex's
> hook environment, and the patch text per operation were not run (no model call).

## Significance for Hangar

- The helper can live in the Hangar executable itself (`hangar gate-hook`); no separate console
  program is needed on Windows.
- The helper prints the deny as JSON for every agent and exits 2 as well
  (`src-tauri/src/gate_hook.rs`).
- The Codex recipe uses literal strings, an unanchored matcher and a bare path, and refuses an
  install path with a space when the program is a cmd.exe launcher
  (`src-tauri/src/hook_recipes.rs`, `gate_service.rs`). Hangar never adds the bypass flag twice.
