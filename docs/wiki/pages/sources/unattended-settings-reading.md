---
title: Settings files that turn agent prompts off
type: source
tags: [agents, claude-code, codex, aider, gemini, copilot, security, links]
sources: []
raw: raw/2026-11-20-unattended-settings-reading.md
url: https://code.claude.com/docs/en/permission-modes
retrieved: 2026-11-20
created: 2026-11-20
updated: 2026-11-20
---

# Settings files that turn agent prompts off

Reading of the official docs of Claude Code, Codex CLI, Gemini CLI, Aider and GitHub Copilot CLI
on which settings file and key start the agent without asking before tools. Nothing was run.

## Key points
- **Claude Code:** `permissions.defaultMode` `bypassPermissions` in the user file
  (`CLAUDE_CONFIG_DIR` or `~/.claude/settings.json`), `--settings` or the managed file. Since
  v2.1.257 project files may no longer set it; older versions took it from there. `dontAsk` does
  not ask but denies what would have asked.
- **Codex CLI:** `approval_policy = "never"` in `$CODEX_HOME/config.toml`, a profile file
  `<name>.config.toml` or a trusted project's `.codex/config.toml`. `--yolo` adds
  `sandbox_mode = "danger-full-access"`.
- **Aider:** `yes-always: true` in `.aider.conf.yml` (home, git root, working folder, or only the
  `--config` file), or `AIDER_YES_ALWAYS` in the environment or a `.env` file.
- **Gemini CLI and Copilot CLI:** no file key; only start arguments run them without prompts.

## Significance for Hangar
Used by `src-tauri/src/unattended.rs` for "Agents without prompts" in [[0014-session-links]]. Hangar
reads every place and counts any one that says so, without working out precedence, so a newer or
older CLI version that reads a file differently cannot hide the mode.

> [!question] Unverified
> The older Codex `[profiles.x]` table for current versions; the precedence between Aider's
> arguments, environment and YAML; whether Copilot CLI reads `COPILOT_ALLOW_ALL`.
