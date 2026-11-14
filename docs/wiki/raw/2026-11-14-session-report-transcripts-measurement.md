# Measurement: transcript fields for the session report (2026-11-14)

Platform: Windows 11 Pro 10.0.26300. Installed: Claude Code 2.1.325, codex-cli 0.169.0. Python
scripts (`py -3 -I`) counted record types, key names, block types, tag names at the start of user
texts and `toolUseResult` key sets in local Claude Code main transcripts
(`<config>/projects/<enc>/<session uuid>.jsonl`, the newest 60 plus two picked by size); the
records were written by Claude Code 2.1.275 to 2.1.325 (most by 2.1.319 and 2.1.322). No message
content was printed or copied.

## Claude Code main transcripts
- Record types of one 1 000-line transcript: `assistant` 253, `user` 182, `attachment` 109,
  `queue-operation` 56, `system` 50, `api-request-blob` 46, `api-request` 45, `ai-title`,
  `agent-name`, `mode`, `permission-mode`, `atis-latch` 34 each, `last-prompt` 33,
  `file-history-snapshot` 10, `api-request-shape` 1, `cost-state` 1.
- `user` and `assistant` records all carry `timestamp` (RFC 3339 UTC with milliseconds),
  `gitBranch`, `cwd`, `isSidechain`, `sessionId`, `uuid`, `parentUuid`, `version`.
- `assistant` content blocks: `tool_use` (`caller`, `id`, `input`, `name`, `type`), `thinking`
  (`signature`, `thinking`), `text`. One block per record.
- `user` content: a string, or blocks of `tool_result` (`content`, `tool_use_id`, optional
  `is_error`) and `text`.
- `user` string records by marker:
  - `origin: {"kind": "human"}` with `promptSource` `typed`, `queued` or `suggestion_accepted`:
    what the person typed.
  - `origin.kind` `task-notification` (text starts with `<task-notification>`) and `peer` (with
    `isMeta: true`, messages from other agents): not typed by the person.
  - No `origin`: texts starting with `<command-name>` (a slash command, followed by
    `<command-message>` and `<command-args>`), `<local-command-stdout>`, `<bash-input>`,
    `<bash-stdout>`, and `<local-command-caveat>` (with `isMeta: true`); one record with
    `isCompactSummary: true` and `isVisibleInTranscriptOnly: true` (the summary after compacting).
- `toolUseResult` of an `Agent` call: a finished run has `agentId`, `agentType`, `status:
  "completed"`, `totalTokens`, `totalToolUseCount`, `totalDurationMs`, `usage`, `content` and
  more (6 records); a background run has `agentId`, `status: "async_launched"`, `isAsync`,
  `outputFile`, `description`, `prompt` and no token count (550 records). Some results have no
  `toolUseResult` (43) or a string (2).
- `Agent` inputs: `description`, `prompt`, `subagent_type`, `run_in_background`.
- Tool names seen in one transcript: `Bash` 117, `Write` 85, `Edit` 19, `Read` 14,
  `PowerShell` 7, `AskUserQuestion` 6, `Agent` 4, `ToolSearch` 2, `TaskStop` 2, `Skill`, `Monitor`,
  `SendMessage` 1 each.

## Codex CLI 0.169.0
- `codex --help` lists no option that sets the thread or session id at launch (options: `-c`,
  `--enable`, `--disable`, `--remote`, `-i`, `-m`, `--oss`, `-p`, `-s`, `-C`, `--worktree`,
  `--add-dir`, `-a`, `--search`, `--no-alt-screen`, `--no-daemon` and others); `resume` and
  `fork` take an existing id.
- A new subcommand `migrate-rollouts` "Inspect or migrate legacy local sessions to paginated
  thread history": the storage of sessions is being changed.
- `~/.codex/sessions/2026/11/14/` holds two rollouts named
  `rollout-2026-11-14T19-14-31-<id>.jsonl`, started in the same second: start time cannot tell
  two parallel sessions apart.
