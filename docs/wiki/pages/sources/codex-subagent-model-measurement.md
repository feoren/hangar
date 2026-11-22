---
title: "Measurement: Codex CLI 0.171.0 sub-agent default model and reasoning efforts"
type: source
tags: [agents, codex-cli, sub-agents, models, measurement, live]
sources: []
raw: raw/2026-11-22-codex-subagent-model-measurement.md
url:
retrieved: 2026-11-22
created: 2026-11-22
updated: 2026-11-22
---

# Measurement: Codex CLI 0.171.0 sub-agent default model and reasoning efforts

Live runs with `codex exec` (Codex CLI 0.171.0, Windows 11, ChatGPT account) in an empty test
folder. Proof from the rollout files in `~/.codex/sessions`. Settles the two open questions of
[[sub-agent-models-research]].

## Key points

- **The `-c` overrides reach the sub-agent.** With `-c agents.default_subagent_model=gpt-5.6-luna`
  and `-c agents.default_subagent_reasoning_effort=medium` on a main session running
  `gpt-6-luna low` (and `config.toml` saying `gpt-6.1-sol high`, no `[agents]` table), the
  sub-agent started by `spawn_agent` without an agent type ran `gpt-5.6-luna medium` (its
  rollout's `turn_context`; its `session_meta.source` is `subagent.thread_spawn` with the parent's
  id). Same with effort `max`.
- **Sub-agent efforts are checked against the account's model catalog before the start.** An
  effort the model does not list stops the spawn: `Reasoning effort `minimal` is not supported
  for model `gpt-5.6-luna`. Supported reasoning efforts: low, medium, high, xhigh, max`. The same
  for `none` and `ultra`. The sub-agent does not start at all; the main agent only gets the error.
- **The catalog** (`~/.codex/models_cache.json`): every listed model takes `low`, `medium`,
  `high`, `xhigh` and `max`; most `sol`, `astra` and `terra` models also `ultra`; no model lists
  `minimal` or `none`.
- **Main session:** the API rejects `minimal` for every model tried (`Unsupported value:
  'minimal' is not supported with the 'gpt-6.1-sol' model. Supported values are: 'low',
  'medium', 'high', 'xhigh', and 'max'.`), `none` for gpt-6.1-sol, any unknown word with an
  enum error. gpt-6-luna took `none` and `ultra` in the main session although its catalog entry
  lists neither: the main session is laxer than the sub-agent check.
- Codex starts a session with any effort string; only the first request fails.

> [!question] Unverified
> - The catalog comes from the server per account; another account or a later release may list
>   other efforts.
> - What gpt-6-luna does with `ultra` in the main session (mapped or passed through) is not
>   visible in the rollout.
> - A field `model_reasoning_effort` in an agent TOML file was not run; it is assumed to take the
>   same check as the default.

## Significance for Hangar

- Hangar's `-c agents.default_subagent_model=...` and `-c
  agents.default_subagent_reasoning_effort=...` are the right keys; nothing to change there.
- Hangar offered `minimal`, which no model takes: with it as the default, no Codex sub-agent
  started. Following "only offer what works", the offered efforts are now `low`, `medium`,
  `high`, `xhigh`, `max` (the ones every model lists); a stored `minimal` default is dropped on
  reading. Contract: [[sub-agent-library]].
