---
title: Sub-agent transcripts of agent CLIs
type: source
tags: [agents, claude-code, codex, opencode, gemini, sub-agents, transcripts]
sources: ["[[subagent-activity-measurement]]"]
raw: raw/2026-11-08-subagent-transcripts-measurement.md
url: https://code.claude.com/docs/en/hooks
retrieved: 2026-11-08
created: 2026-11-08
updated: 2026-11-11
---

# Sub-agent transcripts of agent CLIs

Research for feature 23 "Show sub-agents" ([[agent-workflow-features]]): a read-only live view per
sub-agent of a running session. Measured on Windows 11 with Claude Code 2.1.319, Codex 0.167.0 and
OpenCode 1.15.7; Gemini CLI not installed. Only file names, record type names and key names were
read, no message content.

## Claude Code (measured 2.1.319)
- Project folder: the cwd with every non-alphanumeric character replaced by `-`
  (`D:\Work\Repository\hangar` becomes `D--Work-Repository-hangar`). Honors
  `CLAUDE_CONFIG_DIR`.
> [!question] Unverified
> Truncation or hashing of very long paths. Hangar should locate `<uuid>.jsonl` by scanning
> `projects/*/` instead of computing the folder name.
- Main transcript `~/.claude/projects/<enc>/<sessionId>.jsonl` holds no sidechain records; the
  `Agent` tool result holds `agentId` (`a` plus 16 hex), `description`, `status`, `outputFile`.
- Sub-agents: `<sessionId>/subagents/agent-<agentId>.jsonl` plus `agent-<agentId>.meta.json`
  (agentType, isBuiltIn, description, toolUseId, spawnDepth, `finished: true` when done). Records
  carry `isSidechain: true`, `agentId` and the parent `sessionId`; no end marker in the file.
> [!question] Unverified
> Whether a missing `finished` always means running; when the `subagents/` layout was introduced.
- Still the same layout in 2.1.322. Token count, tool use count and structured tool results
  (`toolUseResult`) per record, and that the live activity line is not stored:
  [[subagent-activity-measurement]].
- Flags: `--session-id <uuid>` (valid UUID), `--resume [id]`, `--continue`, `--fork-session`.
  Combining `--session-id` with resume or continue requires `--fork-session` (changelog 2.0.73).
- Hooks: `SubagentStart` (2.0.43); `SubagentStop` with `agent_id`, `agent_transcript_path` (2.0.42),
  `last_assistant_message` (2.1.47); `agent_id` and `agent_type` on all hook events (2.1.69).
  `--settings` ranks above user, project and local settings and list keys merge.
> [!question] Unverified
> That hooks passed through `--settings` merge with the user's own hooks.

## Codex CLI (0.167.0)
- Sub-agents exist (`/subagents`, `[agents]` config). Each thread is a rollout
  `~/.codex/sessions/YYYY/MM/DD/rollout-<time>-<threadId>.jsonl`; `session_meta` carries
  `parent_thread_id`, `source`, `thread_source`. No flag to fix the thread id at launch.

## OpenCode (1.15.7)
- Child sessions with `parent_id` in `~/.local/share/opencode/opencode.db`; HTTP API
  `GET /session/:id/children` and SSE `GET /event`; server port via `--port`.

## Gemini CLI
- Sub-agents are documented; where their chats are stored is unverified.

## Significance for Hangar
- Claude first: give every Claude session a UUID (`--session-id` on a fresh start, `--resume <uuid>`
  on restore), which also lifts the "two sessions in one folder" limit of [[resume]]; watch
  `<uuid>/subagents/`. OpenCode and Codex later, Gemini last.
- Privacy: extract a whitelist only (agent id, description, type, status, current tool name, a short
  excerpt of the last assistant text), in memory only, never persisted. With excerpts on, the
  newest call's short target (file name, pattern, `Bash` description) and the token count join it.
- Tailing on Windows: open with read, write and delete sharing, keep a byte offset, buffer partial
  lines, read only the tail of large files for the snapshot, skip unparsable lines, validate ids
  before building paths. The view of one sub-agent reads its whole transcript (bounded at 64 MB).

## Sources
- https://code.claude.com/docs/en/hooks
- https://code.claude.com/docs/en/settings
- https://code.claude.com/docs/en/cli-reference
- https://raw.githubusercontent.com/anthropics/claude-code/main/CHANGELOG.md
- https://learn.chatgpt.com/docs/agent-configuration/subagents
- https://opencode.ai/docs/agents/
- https://docs.opencode.ai/docs/server/
- https://github.com/google-gemini/gemini-cli/blob/main/docs/core/subagents.md
