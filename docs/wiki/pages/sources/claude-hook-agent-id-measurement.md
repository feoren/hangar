---
title: "Measurement: agent_id and agent_type in Claude Code's PreToolUse input"
type: source
tags: [claude-code, codex, hooks, sub-agents, gate, measurement, windows]
sources: []
raw: raw/2026-11-22-claude-hook-agent-id-measurement.md
url: https://code.claude.com/docs/en/hooks
retrieved: 2026-11-22
created: 2026-11-22
updated: 2026-11-22
---

# Measurement: agent_id and agent_type in Claude Code's PreToolUse input

Two runs of `claude -p` (Claude Code 2.1.330, Windows 11, `--model haiku`), each starting one
sub-agent that writes a file, with a `PreToolUse` hook on every tool: a command hook in run 1, an
http hook in run 2 (`raw/2026-11-22-claude-hook-agent-id-measurement.md`).

## Key points
- A sub-agent's tool call carries `agent_id` and `agent_type` in the `PreToolUse` input, over a
  command hook and over an http hook alike (`"agent_id":"aaafc28bb29131913"`,
  `"agent_type":"general-purpose"`).
- The main agent's calls (`Agent`) carry neither key: they are absent, not `null`.
- `agent_id` is the same id as the sub-agent's transcript name
  (`subagents/agent-aaafc28bb29131913.jsonl`), which is the id Hangar's sub-agent watch uses.
  Both measured ids are 17 characters of `a` plus hex.
- `session_id` and `transcript_path` stay the main session's inside a sub-agent.
- The docs add: `agent_id` also names an in-process teammate; `agent_type` is also present for a
  main agent started with `--agent`, so the type alone does not mean "sub-agent".
- Codex (docs only, no version stated, not run): `PreToolUse` has no `agent_id` or `agent_type`;
  sub-agent hooks carry the parent's `session_id`. Codex sessions have no crew column
  (Hangar watches sub-agents of Claude Code only), so nothing depends on it.

## Significance for Hangar
- The gate reads both fields, keeps them only in the expected shape (id as a transcript id, type
  up to 128 name characters, a type without an id is dropped) and names the sub-agents on the
  pending decision; the crew column marks exactly those as waiting
  ([[crew-column]]).
- A decision that names nobody is the main agent's, and the column marks nobody for it: the
  column exists only for Claude Code, which sends `agent_id` since 2.1.69. The earlier guess
  (the sub-agent that wrote last before the decision) is gone.

> [!question] Unverified
> Droid, Copilot, Gemini and Cursor were not checked; the helper passes the fields on from any
> agent that sends them in Claude Code's shape. Claude Code before 2.1.69 (changelog date of the
> field, [[subagent-transcripts]]) was not run.
