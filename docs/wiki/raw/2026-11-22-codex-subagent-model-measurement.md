# Measurement record: Codex CLI 0.171.0 sub-agent default model and reasoning efforts (2026-11-22)

Platform: Windows 11 Pro 10.0.26300. `codex --version`: `codex-cli 0.171.0`. Signed in with a
ChatGPT account. Every run headless with `codex exec`, in an empty folder under
`tests/.tmp/codex-subagent-model/` (not a git repository, hence `--skip-git-repo-check`), sandbox
`read-only`, stdin from `/dev/null`. The user's `config.toml` sets `model = "gpt-6.1-sol"` and
`model_reasoning_effort = "high"` and has no `[agents]` table, so every sub-agent value below can
only come from the `-c` overrides. Evidence for the models: the `turn_context` lines
(`payload.model`, `payload.effort`) and the `session_meta` line (`payload.source`) of the rollout
files under `~/.codex/sessions/2026/11/22/`.

## Question 1: does `-c agents.default_subagent_model` reach a sub-agent?

```
codex exec --skip-git-repo-check -s read-only -m gpt-6-luna -c model_reasoning_effort=low \
  -c agents.default_subagent_model=gpt-5.6-luna -c agents.default_subagent_reasoning_effort=medium \
  "Use your spawn_agent tool exactly once, with no agent type, to start one sub-agent whose task is: Reply with the single word PONG. Wait for it, then reply with what it answered. Do not run any shell command."
```

Output (excerpt):

```
model: gpt-6-luna
reasoning effort: low
collab: Wait
codex
PONG
```

Main rollout: `session_meta.source = "exec"`, `turn_context` `gpt-6-luna low`, then a
`function_call` `spawn_agent` (`task_name: "pong"`, `fork_turns: "all"`), then `wait_agent`.

Second rollout written by the same run:

```
session_meta.source = {"subagent": {"thread_spawn": {"parent_thread_id": "<main session id>", "depth": 1, "agent_path": "/root/pong", "agent_nickname": "Noether", "agent_role": null}}}
turn_context: model gpt-5.6-luna, effort medium
```

The same with `agents.default_subagent_reasoning_effort=max`: the sub-agent's `turn_context` is
`gpt-5.6-luna max`, it answered `PONG`.

## Question 2: which efforts does a model take?

### Main session (`-c model_reasoning_effort=<effort>`, prompt "Reply with the single word OK.")

| Model | Effort | Result |
| --- | --- | --- |
| gpt-6-luna | minimal | rejected by the API (below) |
| gpt-6-luna | none | `OK` |
| gpt-6-luna | ultra | `OK`, `turn_context` effort `ultra` |
| gpt-6.1-sol | minimal | rejected by the API |
| gpt-6.1-sol | none | rejected by the API |
| gpt-6.1-sol | bogus | rejected by the API, enum error |

Verbatim errors:

```
ERROR: {"type":"error","error":{"message":"Unsupported value: 'minimal' is not supported with the 'gpt-6-luna' model. Supported values are: 'none', 'low', 'medium', 'high', 'xhigh', and 'max'.","type":"invalid_request_error","param":"reasoning.effort","code":"unsupported_value"},"status":400}
ERROR: {"type":"error","error":{"message":"Unsupported value: 'minimal' is not supported with the 'gpt-6.1-sol' model. Supported values are: 'low', 'medium', 'high', 'xhigh', and 'max'.","type":"invalid_request_error","param":"reasoning.effort","code":"unsupported_value"},"status":400}
ERROR: {"type":"error","error":{"message":"Unsupported value: 'none' is not supported with the 'gpt-6.1-sol' model. Supported values are: 'low', 'medium', 'high', 'xhigh', and 'max'.","type":"invalid_request_error","param":"reasoning.effort","code":"unsupported_value"},"status":400}
"message": "[ReasoningEffortParam] [reasoning.effort] [invalid_enum_value] Invalid value: 'bogus'. Supported values are: 'none', 'minimal', 'low', 'medium', 'high', 'xhigh', and 'max'.",
```

Codex starts the session with any string as the effort (the banner prints it); only the request
fails.

### Sub-agent (`-c agents.default_subagent_reasoning_effort=<effort>`, sub-agent model gpt-5.6-luna)

| Effort | Result |
| --- | --- |
| medium | started, `PONG` |
| max | started, `PONG` |
| minimal | not started |
| none | not started |
| ultra | not started |

Verbatim errors (Codex checks before the sub-agent starts; the main agent reported the text):

```
ERROR codex_core::tools::router: error=Reasoning effort `minimal` is not supported for model `gpt-5.6-luna`. Supported reasoning efforts: low, medium, high, xhigh, max
ERROR codex_core::tools::router: error=Reasoning effort `ultra` is not supported for model `gpt-5.6-luna`. Supported reasoning efforts: low, medium, high, xhigh, max
ERROR codex_core::tools::router: error=Reasoning effort `none` is not supported for model `gpt-5.6-luna`. Supported reasoning efforts: low, medium, high, xhigh, max
```

## Model catalog of the account

`~/.codex/models_cache.json` (written by Codex, `supported_reasoning_levels[].effort` and
`default_reasoning_level` per model):

```
gpt-6.2-sol     low     low medium high xhigh max ultra
gpt-6.2-astra   low     low medium high xhigh max ultra
gpt-6.1-terra   low     low medium high xhigh max ultra
gpt-6.1-sol     low     low medium high xhigh max ultra
gpt-6-astra     low     low medium high xhigh max ultra
gpt-6-sol       medium  low medium high xhigh max ultra
gpt-6-luna      medium  low medium high xhigh max
gpt-5.6-sol     low     low medium high xhigh max ultra
gpt-5.6-terra   medium  low medium high xhigh max ultra
gpt-5.6-luna    medium  low medium high xhigh max
(hidden) gpt-reserve, codex-auto-review: low medium high xhigh max
```

No model lists `minimal` or `none`. The sub-agent check matches this list (its error names the
same five for gpt-5.6-luna). The main session's `none` for gpt-6-luna and `ultra` for gpt-6-luna
went through although the catalog does not list them for that model.

## Cost

Eleven `codex exec` runs, each about 24 000 tokens (mostly the fixed system prompt).
