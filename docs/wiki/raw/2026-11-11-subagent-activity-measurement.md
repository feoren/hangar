# Measurement: what Claude Code stores about a running sub-agent (2026-11-11)

Platform: Windows 11 Pro 10.0.26300. Claude Code 2.1.322 (the `version` field of the records).
Python scripts read record type names, key names, value types and numbers from the transcripts of
about 160 sub-agents of one project; no message content was copied.

Question: where does Claude Code get the line it shows per background agent
(`<type>  <activity>  4m 12s · ↓ 119.9k tokens`) and the tool use count of its agent view?

Observations:
- Sub-agent transcript record types and their share of bytes (12.4 MB example): `user` 8.2 MB,
  `api-request-blob` 3.2 MB, `assistant` 0.56 MB, `attachment` 0.26 MB, `api-request` 0.12 MB,
  `api-request-shape` 0.07 MB. `api-request*` records start with `{"type":"api-request`; `user`,
  `assistant` and `attachment` records start with `{"parentUuid":`. Longest line seen: 1.21 MB.
  Transcript sizes seen: 0.5 to 12.4 MB.
- `assistant` record: `message.usage` with `input_tokens`, `cache_creation_input_tokens`,
  `cache_read_input_tokens`, `output_tokens`, `output_tokens_details.thinking_tokens`,
  `server_tool_use`, `service_tier`, `cache_creation`, `iterations`, `speed`. One API response can
  span several records (same `message.id`).
- `user` record with a tool result carries `toolUseResult` beside `message`. Shapes by tool:
  - Read: `type` (`text` or `image`), `file.{filePath, content, numLines, startLine, totalLines}`;
    images `file.{base64, type, originalSize, dimensions}`.
  - Write: `type` (`create` or `update`), `filePath`, `content`, `structuredPatch`, `originalFile`.
  - Edit: `filePath`, `oldString`, `newString`, `originalFile`, `structuredPatch[]` with
    `oldStart`, `oldLines`, `newStart`, `newLines`, `lines` (prefixed ` `, `-`, `+`), `replaceAll`.
  - Grep: `mode` (`files_with_matches`, `content`, `count`), `filenames`, `numFiles`, and
    `content` with `numLines`, or `numMatches`.
  - Bash, PowerShell: `stdout`, `stderr`, `interrupted`, `isImage`, optional `backgroundTaskId`,
    `bashEditDiff`, `gitOperation`, `persistedOutputPath`.
  - WebFetch: `bytes`, `code`, `durationMs`, `result`, `url`. WebSearch: `query`, `results`.
  - MCP tools: an array of `{type, text}`. Errors (`is_error: true`): a plain string.
  - Glob: no sample in the measured transcripts.
- Parent transcript: the async `Agent` tool result holds `agentId`, `description`, `prompt`,
  `status`, `outputFile`, `isAsync`. On completion an `attachment` of type `queued_command`
  arrives with `usage.{totalTokens, toolUses, durationMs}` and a `<task-notification>` prompt.
- For 4 of 4 sub-agents that ran once, `totalTokens` equals the sum `input_tokens +
  cache_creation_input_tokens + cache_read_input_tokens + output_tokens` of the last assistant
  record of the sub-agent transcript, and `toolUses` equals the number of `tool_use` blocks.
  Continued sub-agents report the numbers of the last run only.
- The live activity text (for example "Reading commands.rs to add the command") was searched in
  every file under the config folder: it is stored nowhere except where the text was quoted in a
  prompt. It is generated in memory.
