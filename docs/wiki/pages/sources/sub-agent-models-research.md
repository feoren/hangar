---
title: Sub-agent models and Codex CLI agent files (docs and binary readings)
type: source
tags: [agents, claude-code, codex-cli, sub-agents, models]
sources: []
raw: raw/2026-11-16-sub-agent-models-research.md
url: https://code.claude.com/docs/en/sub-agents
retrieved: 2026-11-16
created: 2026-11-16
updated: 2026-11-22
---

# Sub-agent models and Codex CLI agent files (docs and binary readings)

Documentation of Claude Code and Codex CLI on sub-agents, `codex --help` and strings in the
binaries of Claude Code 2.1.325 and Codex CLI 0.169.0 on Windows 11. No session was run.

## Key points

- **Claude Code 2.1.325:** frontmatter `model` takes `sonnet`, `opus`, `haiku`, `fable`, a full id
  or `inherit`. Order: call parameter, definition's `model`, variable
  `CLAUDE_CODE_SUBAGENT_MODEL`, main conversation's model. The binary confirms that a set field
  comes before the variable and that `inherit` in the variable means "not set".
- **Codex CLI 0.169.0:** agents are TOML files in `~/.codex/agents` and `<project>/.codex/agents`
  with `name`, `description`, `developer_instructions` and optional `model`,
  `model_reasoning_effort`. Defaults: `[agents] default_subagent_model` and
  `default_subagent_reasoning_effort` in `config.toml`; both keys are in the binary. There is no
  option to start a session as an agent.
- `codex -c key=value` overrides one config value; the value is parsed as TOML and taken as a
  raw string when that fails.
- Effort names in the binary: `none minimal low medium high xhigh max ultra`.

Settled by the live run [[codex-subagent-model-measurement]] (0.171.0): the `-c` overrides reach
the sub-agent; no model takes `minimal`, every listed one takes `low` to `max`.

> [!question] Unverified
> - `CLAUDE_CODE_SUBAGENT_EFFORT` and `CLAUDE_CODE_SUBAGENT_MODEL_FORCE`: undocumented, not used.
> - Agent folders and formats of Gemini CLI, Qwen Code, OpenCode and Copilot CLI: not installed
>   here, not read.

## Significance for Hangar

Model per definition and a default per program, and copies of the user's Claude Code definitions
in Codex CLI's folder: [[sub-agent-library]].
