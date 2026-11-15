---
title: "Measurement: instruction and first prompt options of Claude Code, Codex CLI and Aider"
type: source
tags: [agents, claude-code, codex, aider, gemini, instructions, measurement, windows]
sources: []
raw: raw/2026-11-13-agent-instruction-flags.md
url:
retrieved: 2026-11-13
created: 2026-11-13
updated: 2026-11-13
---

# Measurement: instruction and first prompt options of Claude Code, Codex CLI and Aider

Own measurement on Windows 11 (build 26300): `--help` of the installed CLIs and the strings of the
Codex executable. No session was started and no model was called.

## Key points

- **Claude Code 2.1.323:** `--append-system-prompt <prompt>` appends to the default system prompt;
  `claude [options] [command] [prompt]` starts an interactive session with that prompt. A file
  variant (`--append-system-prompt-file`) is only mentioned in the `--bare` description, not listed
  as an option.
- **Codex CLI 0.169.0:** `-c key=value` overrides any `config.toml` value, the value parsed as TOML
  (raw string when that fails); `codex [OPTIONS] [PROMPT]`, the prompt being an "optional user
  prompt to start the session". `developer_instructions` is a top-level key of the config struct
  in the 0.169.0 binary, next to `model_instructions_file`.
- **Aider 0.86.2:** `--read FILE` adds a read-only file; `--message` sends one message and exits
  ("disables chat mode").
- **Gemini CLI:** not installed, nothing measured.
- On this machine the npm shim `codex.cmd` comes before the native `codex.exe` on the PATH.
- Follow-up (raw/2026-11-13-codex-npm-shim-and-claude-equals-form.md): `codex.cmd` runs
  `node codex.js`, which resolves the platform package's `codex.exe` and sets
  `CODEX_MANAGED_PACKAGE_ROOT` and `CODEX_MANAGED_BY_*` before spawning it. Claude Code 2.1.323
  accepts `--append-system-prompt=<value>`, also for a value starting with `-`.

## Significance for Hangar

- Claude Code takes `HANGAR.md` as `--append-system-prompt <text>` and the worktree check as a
  positional prompt; Codex CLI as `-c developer_instructions=<TOML string>` and a positional
  prompt; Aider as `--read <path>` and no first prompt ([[agent-instructions]]).
- A text with line breaks or quotes cannot pass a cmd.exe launcher ([[cmd-launcher]]): for a
  `codex.cmd` or `claude.cmd` the instructions are left out, so on this machine Codex does not get
  them unless the native `codex.exe` comes first on the PATH.

> [!question] Unverified
> That Codex really sends `developer_instructions` given through `-c` to the model, and that Claude
> Code keeps the appended prompt on `--resume`: read from help texts and strings only, no session
> run.
