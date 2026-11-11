# Measurement: context use and window size in Claude Code main transcripts (2026-11-11)

Platform: Windows 11 Pro 10.0.26300. Claude Code 2.1.322. Python scripts read record types, key
names, model ids and token numbers of the 40 newest main transcripts
(`<config>/projects/<enc>/<session uuid>.jsonl`) of one project; no message content was copied.

Observations:
- Main transcripts hold `assistant` records with the same `message.usage` shape as sub-agent
  transcripts: `input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`,
  `output_tokens`, plus `cache_creation`, `iterations`, `output_tokens_details`,
  `server_tool_use`, `service_tier`, `inference_geo`, `speed`, `fallback_credit`.
- `message.model` values seen: `claude-opus-5-5` (6289 records), `claude-sonnet-5` (898),
  `claude-sonnet-5-5` (25), `<synthetic>` (25, all usage numbers 0), `claude-haiku-5-5` (14). No
  id carried a `[1m]` suffix. `requestedModel` equals the model id or is missing.
- Largest context (input + cache + output of one response) seen per model: `claude-opus-5-5`
  625,834; `claude-sonnet-5` 579,989; `claude-sonnet-5-5` 92,841; `claude-haiku-5-5` 42,229. So
  the window of the first two is larger than 200K, although the `api-request` records' `betas`
  list holds no `context-1m` entry.
- No record holds a window size: the only keys containing "context" are `context_management`,
  `context`, `serverClassifierContext`, `hookAdditionalContext`, `wireIngestContext`,
  `contextRendering`, `contentNotInModelContext`, `contextLabel` (a tool schema field). The
  `api-request` records' `params` hold `model`, `betas`, `max_tokens` (128000), `thinking`,
  `context_management`, `output_config`.
- `~/.claude/settings.json` holds `model` as an alias (`opus`), no window size.
- Claude Code's status line gets the window through the JSON it pipes to the status line command;
  Hangar does not see that JSON.

Window sizes per model come from the model table of the Claude API docs (as bundled with Claude
Code 2.1.322, cached 2026-11-03): 1M for Fable 5 to 5.2, Mythos 5.1 and 5.2, Opus 5.5, Opus 5,
Opus 4.8, 4.7, 4.6, Sonnet 5.5, Sonnet 5, Sonnet 4.6, Haiku 5.5; 200K for Haiku 4.5.
