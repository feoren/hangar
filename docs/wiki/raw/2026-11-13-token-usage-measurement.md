# Measurement: usage fields in Claude Code transcripts (2026-11-13)

Platform: Windows 11 Pro 10.0.26300, Claude Code 2.1.322.

Method: a script read the newest main transcript of one project folder under
`%USERPROFILE%\.claude\projects\` and printed only key names and counts, never values or text
(`py -3 -I shape.py <projects folder>`; the script parses every `assistant` record and groups
`message.id`).

Output (verbatim):

```
version 2.1.322 ts len 24 sep T end Z
assistant lines 1345 distinct ids 771 no id 0 identical usage 771
diff samples (count, output_tokens) []
record keys ['advisorModel', 'apiBlockIndex', 'attributionSkill', 'cwd', 'effort', 'entrypoint', 'gitBranch', 'isSidechain', 'message', 'parentUuid', 'perTurnEffort', 'promptCacheRequested', 'requestId', 'requestRef', 'requestedModel', 'serverClassifierRequest', 'sessionId', 'session_id', 'slug', 'thinkingDisplay', 'thinkingDurationMs', 'timestamp', 'toolInputStreamMs', 'type', 'userType', 'uuid', 'version', 'wireIngestContext', 'wireToolInputs']
message keys ['container', 'content', 'context_management', 'diagnostics', 'id', 'input_transformations', 'model', 'role', 'stop_details', 'stop_reason', 'stop_sequence', 'type', 'usage']
usage keys ['cache_creation', 'cache_creation.ephemeral_1h_input_tokens', 'cache_creation.ephemeral_5m_input_tokens', 'cache_creation_input_tokens', 'cache_read_input_tokens', 'fallback_credit', 'inference_geo', 'input_tokens', 'iterations', 'output_tokens', 'output_tokens_details', 'server_tool_use', 'service_tier', 'speed']
```

Reading: 1345 assistant records carry 771 distinct `message.id` values; every record of the same
id has the same four token counts. The `timestamp` is RFC 3339 in UTC with milliseconds
(`YYYY-MM-DDTHH:MM:SS.mmmZ`, 24 characters).

Prices used for the built-in table: the Claude API model table and pricing notes as cached on
2026-11-03 in the Claude API reference that Claude Code ships (input and output per million
tokens for Fable 5.2, 5.1, 5, Mythos 5.2, 5.1, 5, Opus 5.5, 5, 4.8, 4.7, 4.6, Sonnet 5.5, 5, 4.6,
Haiku 5.5, 4.5; cache reads $0.20 Fable/Mythos 5.2, $0.25 Fable/Mythos 5.1, $1 Fable/Mythos 5,
$0.20 Opus 5.5, $0.10 Sonnet 5.5, otherwise 0.1x input; cache writes 1.25x input for 5 minutes and
2x input for 1 hour; fast mode $8/$40 Opus 5.5 and $16/$80 Fable/Mythos 5.2; Haiku 5.5 $0.50/$2.50
above 100K prompt tokens). Live page: https://platform.claude.com/docs/en/about-claude/pricing
