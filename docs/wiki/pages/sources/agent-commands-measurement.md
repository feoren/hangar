---
title: "Measurement: slash commands of Claude Code, Codex CLI and Gemini CLI"
type: source
tags: [agents, claude-code, codex, gemini, slash-commands, measurement, windows]
sources: []
raw: raw/2026-11-14-agent-commands-measurement.md
url: https://raw.githubusercontent.com/google-gemini/gemini-cli/main/docs/reference/commands.md
retrieved: 2026-11-14
created: 2026-11-14
updated: 2026-11-14
---

# Measurement: slash commands of Claude Code, Codex CLI and Gemini CLI

Own measurement on Windows 11 (build 26300): strings of the installed Claude Code and Codex
executables, plus Gemini CLI's command reference (not installed here). No session was started and
no command was typed.

## Key points

- **Claude Code 2.1.325** (native install): the bundle's command table has `compact` ("Free up
  context by summarizing the conversation so far", off with `DISABLE_COMPACT`), `clear` (aliases
  `reset`, `new`; "previous session stays on disk (resumable with /resume)"), `usage` (aliases
  `cost`, `stats`), `context` and `model`. `/cost` is only an alias in this version.
- **Codex CLI 0.169.0** (npm install): the slash command name table holds `new`, `compact`,
  `clear`, `usage` and more; `/status` and `/model` appear in the built-in help, `/new` in two
  hints. The popup descriptions include "summarize conversation to prevent hitting the context
  limit" and "start a new chat during a conversation".
- **While the agent works** (strings only): Codex 0.169.0 contains "'/…' is disabled while a
  task is in progress." and refuses such commands during a turn; Claude Code 2.1.325 queues slash
  commands and prompts typed while it works ("Fate of a queued command (slash command or queued
  user prompt)").
- **Gemini CLI** (docs only, main branch on 2026-11-14): `/compress` replaces the context with a
  summary, `/clear` clears the context and starts a new session, `/stats`, `/model`.

## Significance for Hangar

These are the built-in `agent_commands` of the catalog ([[agent-commands]]): Claude Code
`/compact`, `/context`, `/usage`, `/model`, `/clear`; Codex `/compact`, `/status`, `/model`,
`/new`; Gemini `/compress`, `/stats`, `/model`, `/clear`. The compact button sends `/compact`
or `/compress`.

> [!question] Unverified
> Gemini CLI's commands were not read from an installed version; a spot check with the first
> installed version is open. Whether each CLI accepts a slash command while a turn runs (queued,
> refused or interrupting) was not measured; Hangar's compact button therefore waits for an idle
> agent.
