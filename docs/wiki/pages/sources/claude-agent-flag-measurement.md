---
title: Claude Code --agent option (help text)
type: source
tags: [agents, claude-code, sub-agents]
sources: []
raw: raw/2026-11-15-claude-agent-flag-help.md
url: ""
retrieved: 2026-11-15
created: 2026-11-15
updated: 2026-11-15
---

# Claude Code --agent option (help text)

`claude --help` of Claude Code 2.1.325 on Windows 11.

## Key points

- `--agent <agent>`: "Agent for the current session. Overrides the 'agent' setting."
- `--agents <json-or-file>` defines agents inline; Hangar does not use it.
- The binary names no rule for sub-agent names; the rules found are for plugins, marketplaces,
  hosts and topics.

> [!question] Unverified
> - Whether `--agent` together with `--resume <id>` or `--continue` keeps the agent for the resumed
>   conversation. Hangar passes it on every process start of the session.
> - The name rule of definitions (lowercase letters and hyphens, as Claude Code's documentation
>   is remembered to say) and how a project definition and a global one of the same name rank
>   (project first is assumed).

## Significance for Hangar

The catalog's `agent_args = ["--agent", "{agent}"]` for Claude Code starts a session as a sub-agent
definition ([[sub-agent-library]]).
