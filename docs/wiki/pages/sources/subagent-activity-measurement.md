---
title: What Claude Code stores about a running sub-agent
type: source
tags: [agents, claude-code, sub-agents, transcripts, tokens]
sources: []
raw: raw/2026-11-11-subagent-activity-measurement.md
url: ""
retrieved: 2026-11-11
created: 2026-11-11
updated: 2026-11-11
---

# What Claude Code stores about a running sub-agent

Own measurement on Windows 11 with Claude Code 2.1.322: where the line Claude Code shows per
background agent (`frontend  Editing sessionLabel.ts  4m 12s · ↓ 119.9k tokens`) and its agent
view come from. Key names and numbers only; no content was copied.

## Key points (2.1.322)
- **Tokens:** the number equals `input_tokens + cache_creation_input_tokens +
  cache_read_input_tokens + output_tokens` of the newest `assistant` record's `message.usage` in
  the sub-agent transcript, the context size, not a running sum. Matches `usage.totalTokens` of
  the completion notice (`attachment` of type `queued_command` in the parent transcript) for every
  sub-agent that ran once.
- **Tool uses:** the number of `tool_use` blocks in the sub-agent transcript (`usage.toolUses`).
- **Activity line:** stored nowhere; Claude Code generates it in memory. Hangar derives it from the
  newest tool call: the `description` of a `Bash`/`PowerShell` call, else a verb plus the file name
  or key argument.
- **Result summaries:** each tool result's `user` record carries a structured `toolUseResult`
  beside the message: `file.numLines` (Read), `type: create` plus `content` (Write),
  `structuredPatch[].lines` with `+`/`-` prefixes (Edit, Write over a file), `mode`, `numFiles`,
  `numLines`, `numMatches` (Grep), `stdout`/`stderr` (Bash). MCP tools store an array, errors a
  string. Glob had no sample.
- **Size:** transcripts reach 12 MB; most bytes are `user` records (whole file contents in
  `originalFile`, images as base64) and `api-request-blob` records. `api-request*` records begin
  with their `type`, so a reader can skip them before parsing. Single lines exceed 1 MB.

## Significance for Hangar
- The sidebar row shows type, activity, run time and tokens like Claude Code; the sub-agent view
  shows summaries from `toolUseResult` and ends with tool uses, tokens and run time
  (`src-tauri/src/subagents/claude_log.rs`, `src-tauri/src/subagents/claude.rs`).
- The view reads a transcript from its start up to 64 MB instead of a 1 MB tail, since 2 to 12 MB
  are common and the task stands in the first line.

> [!question] Unverified
> The `toolUseResult` shape of Glob (`filenames`, `numFiles` assumed from Grep's files mode).
