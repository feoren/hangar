---
title: Claude Code agent frontmatter fields (binary reading)
type: source
tags: [agents, claude-code, sub-agents, security]
sources: []
raw: raw/2026-11-15-claude-agent-frontmatter-reading.md
url: ""
retrieved: 2026-11-15
created: 2026-11-15
updated: 2026-11-15
---

# Claude Code agent frontmatter fields (binary reading)

Strings read from the installed Claude Code 2.1.325 binary; no session run.

## Key points

- An agent definition's frontmatter `hooks` and `mcpServers` are loaded for agents and for the
  main-thread agent, which `--agent` starts.
- Both are skipped when the folder the definition file came from is not trusted.
- A frontmatter `permissionMode` is ignored where it would widen the inherited mode.

> [!question] Unverified
> The exact trust check (which folders count as trusted) and whether `permissionMode` can narrow
> the mode of a main-thread agent were not measured live.

## Significance for Hangar

A project definition can bring its own hooks and MCP servers. Hangar names these fields for
project definitions wherever it offers them ([[sub-agent-library]]).
