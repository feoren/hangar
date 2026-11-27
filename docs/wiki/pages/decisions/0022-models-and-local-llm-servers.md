---
title: "ADR 0022: Model choice per start, local LLM servers (Ollama, LM Studio), Hermes and OpenClaw"
type: decision
status: accepted
superseded_by:
tags: [agents, catalog, models, ollama, lm-studio, local-llm, sidebar, top-bar]
sources: []
created: 2026-11-27
updated: 2026-11-27
---

# ADR 0022: Model choice per start, local LLM servers (Ollama, LM Studio), Hermes and OpenClaw

## Context
A session always started with the model the agent CLI picks on its own. Choosing another one
meant a `--model` in `profiles.toml` or `/model` in the running session. Local models (Ollama,
LM Studio) could not be used from Hangar at all, and nobody could see whether such a server ran.
Two agent CLIs people ask for were missing: Nous Research's Hermes Agent and OpenClaw.

Facts this decision rests on (checked 2026-11-27 against the official docs; the URLs are in
[[agent-clis-research]] once ingested):
- Ollama listens on port 11434: `GET /api/tags` lists installed models (`models[].name`),
  `GET /api/ps` the loaded ones. It speaks the Anthropic Messages API at `/v1/messages`.
- LM Studio listens on port 1234: `GET /api/v1/models` lists models (`models[].key`,
  `display_name`, `type`, `loaded_instances`, empty when not loaded). It also speaks
  `/v1/messages`. Older servers: `GET /v1/models` (`data[].id`).
- Claude Code: `--model <alias|id>`, aliases `fable`, `opus`, `sonnet`, `haiku`. Against
  Ollama: `ANTHROPIC_BASE_URL=http://127.0.0.1:11434`, `ANTHROPIC_AUTH_TOKEN=ollama`,
  `ANTHROPIC_API_KEY=` (empty). Against LM Studio: `ANTHROPIC_BASE_URL=http://127.0.0.1:1234`,
  `ANTHROPIC_AUTH_TOKEN=lmstudio`, `CLAUDE_CODE_ATTRIBUTION_HEADER=0`.
- Codex CLI: `-m <model>`; current models `gpt-6.2-sol`, `gpt-6.2-astra`, `gpt-6.1-terra`
  (CLI 0.159.0+), `gpt-6-luna`. Local: `--oss --local-provider ollama|lmstudio -m <model>`.
- Gemini CLI: `-m <model>`, aliases `auto`, `pro`, `flash`, `flash-lite`. No local models.
- Aider: `--model ollama_chat/<m>` with `OLLAMA_API_BASE=http://127.0.0.1:11434`;
  `--model lm_studio/<m>` with `LM_STUDIO_API_BASE=http://127.0.0.1:1234/v1` and
  `LM_STUDIO_API_KEY=dummy-api-key` (required, any value).
- Hermes Agent: executable `hermes`, interactive with `hermes`; `hermes chat --provider <id>
  -m <model>`. Ollama needs a custom endpoint in `~/.hermes/config.yaml` (no flag).
- OpenClaw: executable `openclaw`; `openclaw tui --local` runs without a gateway. No model flag:
  models change with `/model` inside the session.

## Decision

### Catalog (`agent_catalog.toml`, `profiles.toml`)
Three new optional fields per agent, also accepted in a user's `[[profile]]` (a profile's value
replaces the catalog's):
- `models = [{ id = "opus", label = "Opus 5.5" }, ...]`: the vendor's models offered in the
  menu, in this order. `id` is what goes into `{model}`.
- `model_args = ["--model", "{model}"]`: how a vendor model goes into the start. Required when
  `models` is set.
- `local = { ollama = { args = [...], env = { ... } }, lmstudio = { args = [...], env = { ... } } }`:
  how the agent runs against a local server. `{model}` and `{base}` (the server's base URL,
  `http://127.0.0.1:11434` or `http://127.0.0.1:1234`) are the only placeholders, in args and
  env values. An env value may be empty (sets the variable empty). `{model}` must appear in the
  args or in at least one env value (amended 2026-11-27: some programs take the model only from a
  variable, such as Goose's `GOOSE_MODEL`); an entry with it in neither is refused.

Values for the built-in agents follow the facts above. Claude Code: `fable` "Fable 5.2", `opus`
"Opus 5.5", `sonnet` "Sonnet 5.5", `haiku` "Haiku 5.5". Codex CLI: `gpt-6.2-sol` "GPT-6.2 Sol",
`gpt-6.2-astra` "GPT-6.2 Astra", `gpt-6.1-terra` "GPT-6.1 Terra", `gpt-6-luna` "GPT-6 Luna".
Gemini CLI: `auto` "Auto", `pro` "Pro", `flash` "Flash", `flash-lite` "Flash Lite". Aider gets
`local` only (its vendor list is open ended). Hermes gets `local.lmstudio` only if the provider
id for LM Studio is verified in its docs, otherwise nothing. OpenClaw gets neither: Hangar does
not offer what it cannot start ([[only-offer-what-works]] in spirit).

New catalog entries:
- `hermes`, name "Hermes Agent", `commands = ["hermes"]`, homepage
  `https://github.com/NousResearch/hermes-agent`, install lines from its README.
- `openclaw`, name "OpenClaw", `commands = ["openclaw"]`, `args = ["tui", "--local"]`, homepage
  `https://github.com/openclaw/openclaw`, install lines from its README.
Both with icon SVG, NOTICE row and `agent.<id>.description` in 12 languages, like every entry.

### The start (`create_session`)
- New optional argument `model: { source: "provider" | "ollama" | "lmstudio", id: string }`.
- Validation in the backend, before anything starts (a new start: error, no fallback):
  - `id`: 1 to 160 characters from `A-Z a-z 0-9 . _ : / @ + - [ ]`, no leading `-`.
  - `provider`: the agent has `model_args` and `id` is one of its `models`.
  - `ollama` / `lmstudio`: the agent has that `local` entry. The server need not run; the agent
    reports its own error then.
  - Expanded arguments pass `batch::argument_ok` like every other argument for `.cmd` launchers.
- Args: `model_args` (provider) or `local.<server>.args` go after the profile's args and before
  the instruction, agent and prompt args. Env: `local.<server>.env` is applied after the
  profile's env and before secrets, so a user's own secret still wins.
- The choice is stored with the session (`persist.rs`, like `sub_agent`) and used again on
  restore and resume. Recent sessions keep it too.
- A stored choice never costs the session: a value this Hangar cannot read in `workspace.json`
  or the recent sessions (bad id, unknown source, extra field) is dropped on reading with a line
  in the log, and the session or entry stays (`start_model::lenient_stored`).
- A restore, a restart of a placeholder or a reopened recent session (`create_session` with
  `resume`) whose stored model no longer fits (the catalog or the profile dropped it, the
  program lost its `local` entry) starts with the program's default and logs why; the session
  then carries no model. Only a new start from the menu is refused.
- On a resume the model's arguments follow `resume_args`. Codex CLI accepts them there:
  `codex resume` flattens the full interactive option set (`SessionTuiCli(TuiCli)`), which holds
  `-m`, `--oss` and `--local-provider`.
- `ProfileView` gains `models: [{ id, label }]` (empty when none) and
  `localSources: ("ollama" | "lmstudio")[]`.

### Local servers (`local_llm.rs`)
- One background thread, as in `power.rs`: probes both servers every 10 s (30 s while power
  saving), emits `local-llm-changed` only on change, command `local_llm_status` returns the
  current state, command `local_llm_refresh` probes at once (the frontend calls it when the
  window gains focus).
- Loopback only (`127.0.0.1`), no proxy, no redirects, 800 ms timeout, body at most 1 MiB, at
  most 200 models per server, names cut to 160 characters and dropped when they fail the `id`
  rule above.
- Ollama: running when `/api/tags` answers with valid JSON; models from `models[].name`,
  `loaded` from `/api/ps`. LM Studio: `/api/v1/models`, only `type == "llm"`, id `key`, label
  `display_name`, `loaded` when `loaded_instances` is not empty; falling back to `/v1/models`
  (`data[].id`, `loaded` unknown = false) when the new endpoint is missing.
- State shape (camelCase): `{ ollama: Server, lmstudio: Server }`,
  `Server = { running: bool, models: [{ id, label, loaded }] }`.
- The CSP stays as it is: the frontend never talks to the servers itself.

### Interface
- **Top bar**, before the coffee button: one small badge per server with its icon and a dot,
  green (`--ok`) while it runs, grey while not; the tooltip and the accessible name say it in
  words ("Ollama runs, 4 models" / "Ollama is off"). Never color alone.
- **Sidebar, "New session"**: each program row is one start button (its click starts the
  session with the CLI's default). A row whose agent has `models` or a `local` entry for a
  running server opens a flyout directly to its right once the pointer rests on it (about
  150 ms), or with Right arrow (Left arrow and Escape go back to the row). The pointer may cross
  the gap and other rows on its way into the flyout (a short grace period and a safe triangle);
  leaving both closes it, and over another row it hands over at once. No pill, plus or arrow:
  the row keeps its hover look while its flyout is open (amended 2026-11-27, twice: the owner
  found a plus and an arrow unclear, then the "Model" pill too, and asked for a flyout on
  hover). Touch screens have no hover: there a tap starts with the default. The flyout is
  fixed to the window: the vendor's models first (heading: the program's name), then "Ollama"
  and "LM Studio" with their models (loaded ones marked), each section only when it has
  entries. A click starts the session with that model.
- A session row's tooltip names the chosen model.

## Consequences
- Models become one click; local LLMs work with Claude Code, Codex CLI and Aider without any
  setup beyond a running server.
- Vendor model names age: the lists live in the catalog (data, not code) and a user's
  `profiles.toml` can replace them.
- Two small HTTP probes every 10 s on loopback; nothing leaves the machine.
- Open: OpenClaw models (no start flag), the collapsed rail and the app menu do not offer models
  yet.
- Update 2026-11-27: Hermes against Ollama is offered after all (`--provider custom --base-url`
  for one launch); Qwen Code, Oh My Pi, Copilot CLI (Ollama) and Goose (Ollama, model in env only)
  joined. The check of every other
  catalog agent is in [[local-models-per-agent]].

## Evidence
Read on 2026-11-27:
- Ollama API: https://docs.ollama.com/api/tags, https://docs.ollama.com/api/ps,
  https://docs.ollama.com/api/anthropic-compatibility, Claude Code setup
  https://docs.ollama.com/integrations/claude-code, Codex setup
  https://docs.ollama.com/integrations/codex.
- LM Studio API: https://lmstudio.ai/docs/developer/rest, https://lmstudio.ai/docs/developer/rest/list,
  Claude Code setup https://lmstudio.ai/docs/integrations/claude-code, Codex setup
  https://lmstudio.ai/docs/integrations/codex.
- Claude Code models: https://code.claude.com/docs/en/model-config.
- Codex CLI flags: `codex-rs/utils/cli/src/shared_options.rs` in https://github.com/openai/codex
  (`--model`/`-m`, `--oss`, `--local-provider`); models: https://learn.chatgpt.com/docs/models.
  `codex resume` takes them too: `codex-rs/cli/src/main.rs`, `struct ResumeCommand` with
  `#[clap(flatten)] config_overrides: SessionTuiCli` (a wrapper of `TuiCli`), merged by
  `finalize_resume_interactive` (read on main at commit `afe555b9`).
- Gemini CLI: https://geminicli.com/docs/cli/cli-reference/.
- Aider: https://aider.chat/docs/llms/ollama.html, https://aider.chat/docs/llms/lm-studio.html.
- Hermes Agent: https://github.com/NousResearch/hermes-agent,
  https://hermes-agent.nousresearch.com/docs/reference/cli-commands,
  https://hermes-agent.nousresearch.com/docs/integrations/providers.
- OpenClaw: https://github.com/openclaw/openclaw, https://docs.openclaw.ai/web/tui,
  https://docs.openclaw.ai/cli/models.

> [!question] Unverified
> The lowest Ollama and LM Studio versions with `/v1/messages`; Hermes' default interface when
> started without `--tui`/`--cli`.
