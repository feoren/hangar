# Measurement: sub-agent transcripts of agent CLIs (2026-11-08)

Platform: Windows 11 Pro 10.0.26300. Versions: Claude Code 2.1.319, Codex 0.167.0, OpenCode 1.15.7;
Gemini CLI not installed. Only file names, record type names and key names were read; no message
content was opened or copied.

Commands and observations:
- `claude --version` -> 2.1.319; `claude --help` lists `--session-id <uuid>`, `--resume [id]`,
  `--continue`, `--fork-session`, `--settings <file-or-json>`.
- Directory listing of `%USERPROFILE%\.claude\projects\`: the working folder
  `D:\Work\Repository\hangar` maps to `D--Work-Repository-hangar`.
- One session folder contains `<sessionId>.jsonl` and `<sessionId>\subagents\` with
  `agent-<agentId>.jsonl` and `agent-<agentId>.meta.json` per sub-agent (4 sub-agents sampled).
- Record type count in one main file (over 1,800 records): types assistant, user, attachment, system,
  last-prompt, mode, permission-mode, ai-title, queue-operation, file-history-snapshot,
  file-history-delta, tools-used, cost-state, api-request, api-request-blob, api-request-shape,
  session-owner, atis-latch; 0 records with `isSidechain: true`.
- Sub-agent file: all records `isSidechain: true` with `agentId`; same `sessionId` as the parent;
  first record `user` with `parentUuid: null`; no end marker.
- meta.json keys: agentType, isBuiltIn, launchedWithDefaults, description, toolUseId, spawnDepth,
  requestShape, requestNonInteractive, pickedEffortModel, and `finished: true` on 3 of 4 files.
- Codex: first record of rollouts under `%USERPROFILE%\.codex\sessions\2026\11\07\` is
  `session_meta` with keys including id, parent_thread_id, source, thread_source, cwd; one child
  with `source.subagent` (thread_source guardian_review).
- OpenCode: read-only schema query of `%USERPROFILE%\.local\share\opencode\opencode.db`: table
  `session` with `parent_id`, `title`, `agent`, `model`; tables `message` and `part` with JSON `data`.
