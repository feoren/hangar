---
title: Local models per agent (Ollama, LM Studio)
type: synthesis
tags: [agents, catalog, models, ollama, lm-studio, local-llm]
sources: ["[[0022-models-and-local-llm-servers]]"]
created: 2026-11-27
updated: 2026-11-27
---

# Local models per agent (Ollama, LM Studio)

Which catalog agents Hangar starts against a local Ollama (`{base}` = `http://127.0.0.1:11434`)
or LM Studio (`{base}` = `http://127.0.0.1:1234`) server, through the `local` field of
`src-tauri/src/agent_catalog.toml` ([[0022-models-and-local-llm-servers]], [[catalog]]).

## Rule
An agent is offered for a server only when its official docs, README or source show that the
interactive session starts against that server with a chosen model from command line arguments
and environment variables alone: no edit of the user's config files, no prior interactive setup
(login, `/connect`, onboarding), no file written by Hangar. Partial evidence means not offered.
The catalog requires `{model}` in `local.*.args` or in at least one env value
([[0022-models-and-local-llm-servers]]), so a program that takes the model only from a variable
can be offered too.

All sources were read on 2026-11-27. Source code links name the commit or tag that was read.
Nothing on this page was run against a live server.

## Table
Args are what Hangar appends after the catalog's `args`; env is set for the session.

| Agent | Ollama | LM Studio | Sources |
| --- | --- | --- | --- |
| Claude Code (`claude`) | offered: `--model {model}`; `ANTHROPIC_BASE_URL={base}`, `ANTHROPIC_AUTH_TOKEN=ollama`, `ANTHROPIC_API_KEY=` (empty) | offered: `--model {model}`; `ANTHROPIC_BASE_URL={base}`, `ANTHROPIC_AUTH_TOKEN=lmstudio`, `CLAUDE_CODE_ATTRIBUTION_HEADER=0` | [[0022-models-and-local-llm-servers]] |
| Codex CLI (`codex`) | offered: `--oss --local-provider ollama -m {model}` | offered: `--oss --local-provider lmstudio -m {model}` | [[0022-models-and-local-llm-servers]] |
| Aider (`aider`) | offered: `--model ollama_chat/{model}`; `OLLAMA_API_BASE={base}` | offered: `--model lm_studio/{model}`; `LM_STUDIO_API_BASE={base}/v1`, `LM_STUDIO_API_KEY=dummy-api-key` | [[0022-models-and-local-llm-servers]] |
| Hermes Agent (`hermes`) | offered: `chat --cli --provider ollama -m {model}`; `CUSTOM_BASE_URL={base}/v1` | offered: `chat --provider lmstudio -m {model}` | 1, 2 |
| GitHub Copilot CLI (`copilot`) | offered: `--model {model}`; `COPILOT_PROVIDER_BASE_URL={base}/v1`, `COPILOT_PROVIDER_API_KEY=` (empty), `COPILOT_PROVIDER_WIRE_API=responses` | offered: the same; GitHub takes any OpenAI-compatible server with its `/v1` URL, LM Studio serves `/v1/responses` (no page names both together) | 3, 4, 5, 25 |
| Qwen Code (`qwen`) | offered: `--auth-type openai --model {model}`; `OPENAI_BASE_URL={base}/v1`, `OPENAI_API_KEY=ollama`, `OPENAI_MODEL={model}` | offered: same args; `OPENAI_BASE_URL={base}/v1`, `OPENAI_API_KEY=lm-studio`, `OPENAI_MODEL={model}` | 6 |
| Oh My Pi (`omp`) | offered: `--model ollama/{model}`; `OLLAMA_BASE_URL={base}`, `OMP_SKIP_SETUP=1` | offered: `--model lm-studio/{model}`; `LM_STUDIO_BASE_URL={base}/v1`, `OMP_SKIP_SETUP=1` | 7 |
| Gemini CLI (`gemini`) | not offered: `GOOGLE_GEMINI_BASE_URL` only speaks the Gemini API, which neither server serves | not offered: same | 8 |
| Antigravity CLI (`agy`) | not offered: Gemini API only, an API key needs `modelProvider` in its settings file, local servers only in the SDK | not offered: same | 9 |
| Cursor CLI (`cursor-agent`) | not offered: no base URL option, sign-in required, requests go through Cursor's servers | not offered: same | 10 |
| Kimi Code CLI (`kimi`) | offered: no args; `KIMI_MODEL_NAME={model}`, `KIMI_MODEL_API_KEY=ollama`, `KIMI_MODEL_PROVIDER_TYPE=openai`, `KIMI_MODEL_BASE_URL={base}/v1` | offered: the same with `KIMI_MODEL_API_KEY=lm-studio` | 11, 26 |
| Kiro CLI (`kiro-cli`) | not offered: hosted models only, sign-in required | not offered: same | 12 |
| OpenCode (`opencode`) | not offered: 2.x has no `--model` on the full TUI (only `opencode run` and `opencode mini`); 1.x would work with `OPENCODE_CONFIG_CONTENT`, but one catalog entry serves both lines | not offered: same | 13 |
| Goose (`goose session`) | offered: no args; `GOOSE_PROVIDER=ollama`, `GOOSE_MODEL={model}`, `OLLAMA_HOST={base}` | offered: no args; `GOOSE_PROVIDER=lmstudio`, `GOOSE_MODEL={model}`, `LMSTUDIO_HOST={base}` (the provider id and host variable from the source at v1.61.1) | 14 |
| Crush (`crush`) | not offered: local providers only through its config file, no base URL variable, no model flag | not offered: same | 15 |
| Pi (`pi`) | not offered: Ollama and LM Studio only through `~/.pi/agent/models.json` | not offered: same | 16 |
| ForgeCode (`forge`) | not offered: a provider needs an entry in its credentials file; env variables are migrated into that file once | not offered: same | 17 |
| Kode CLI (`kode`) | not offered: models are profiles in `~/.kode.json`; `--model` only reaches print mode | not offered: same | 18 |
| Codebuff (`codebuff`) | not offered: no model flag, account required, own endpoints only through the interactive `/byok` | not offered: same | 19 |
| Cline CLI (`cline`) | not offered: `-P ollama -m <model>` works on the default address only (no base flag at the root) and Cline saves provider and model as its new default | not offered: same with `-P lmstudio` | 20 |
| Mistral Vibe (`vibe`) | not offered: local models need `[[providers]]` and `[[models]]` in `config.toml`; `--model` picks a configured alias | not offered: same | 21 |
| Droid (`droid`) | not offered: own models only in `customModels` of `~/.factory/settings.json` | not offered: same | 22 |
| Amp (`amp`) | not offered: hosted models only | not offered: same | 23 |
| OpenClaw (`openclaw tui --local`) | not offered: `openclaw tui` has no model or provider flag; Ollama needs `openclaw.json` | not offered: LM Studio only through `openclaw onboard`, which writes the config | 24 |

## Notes on the offered entries
- **Hermes Agent, Ollama:** local Ollama is Hermes' custom endpoint ("base URL
  `http://localhost:11434/v1`, no key"). The first entry used `--provider custom --base-url`,
  which the owner saw fail with "provider 'custom' resolved without credentials (no endpoint or
  API key configured)": in Hermes at `bb834a2cb329b27d67958036039d3e4fe9749a54` (2026-10-18)
  `chat` parses `--base-url` and `--api-key` but the classic chat never receives them
  (`_CHAT_PASSTHROUGH` and `cmd_chat` in `hermes_cli/main.py`), so the bare `custom` provider is
  left without endpoint and key (`_raise_for_credentialless_bare_custom` in
  `hermes_cli/runtime_provider.py`). The entry now uses the alias `ollama`, which resolves to the
  custom endpoint, with the endpoint from `CUSTOM_BASE_URL` (an endpoint source the resolver
  names, `_raise_if_local_alias_missing_endpoint`). Measured on Windows 11 on 2026-11-27 with
  that Hermes and Ollama serving `gemma4:12b`: `CUSTOM_BASE_URL=http://127.0.0.1:11434/v1 hermes
  chat --cli --provider ollama -m gemma4:12b -Q -q "Antworte nur mit OK"` answered `OK`; the same
  with `--provider custom` failed with the message above, with and without `--api-key ollama`.
  `--cli` keeps the classic REPL (`_resolve_use_tui`).
- **Hermes Agent, both servers:** on an install where no provider is configured at all, the
  classic chat shows its first-run prompt ("Run setup now?") before argv is used
  (`_has_any_provider_configured` and `_first_run_setup_guard` in `hermes_cli/main.py`). The
  official installer starts the setup wizard right away (source 2), so a set up Hermes is the
  normal case; Hangar offers the entries for that case.
- **Copilot CLI:** the variables are the ones Ollama's page gives; `--model` feeds the provider's
  model id before `COPILOT_MODEL` (changelog 1.0.109). GitHub calls the `COPILOT_PROVIDER_*`
  variables legacy next to `providers.json`; they take precedence over a saved `providers.json`
  unless the user's own `COPILOT_PROVIDERS_CONFIG` names a file.
- **Qwen Code:** all three `OPENAI_*` variables must be set for env auth, so the key is a dummy.
  `--auth-type openai` because a saved `security.auth.selectedType` (for example Qwen OAuth) wins
  over the environment. The interactive UI opens its auth dialog at startup only when no auth
  type is resolved (`unAuthenticated` in `packages/cli/src/ui/auth/useAuth.ts`), and
  `argv.authType` comes first in that resolution (`packages/cli/src/config/config.ts`).
- **Goose:** `GOOSE_PROVIDER` and `GOOSE_MODEL` "override the config file for that process";
  `goose session` stops with "No provider configured" only when no source has one
  (`resolve_provider_and_model` in `crates/goose-cli/src/session/builder.rs`). The documented
  `goose session` options list no `--provider` or `--model` (the source at v1.61.1 has them,
  `ModelOptions` in `crates/goose-cli/src/cli.rs`); Hangar uses the documented variables. On a
  resume, the provider and model saved with the session come before the variables (same
  function), so a resumed Goose session keeps the model it was started with.
- **Oh My Pi:** Ollama and LM Studio are built-in keyless providers discovered without
  `models.yml`; a user's own `ollama` or `lm-studio` provider in `models.yml`, or one in
  `disabledProviders`, takes precedence. Implicit Ollama uses `openai-responses`.

> [!question] Unverified
> No entry was run live. In particular: whether Oh My Pi resolves `--model ollama/...` before
> discovery ends; the lowest Ollama version with `/v1/responses` (Copilot, Oh My Pi).

- **Kimi Code CLI:** `KIMI_MODEL_*` build a provider in memory that wins over `default_model`
  and writes nothing (env-vars.md). The TUI asks for `/login` only while no model is resolved
  (`welcome.ts`), and the env overlay sets one (`envOverlay.ts`), so the first launch needs no
  login; this is read from the code, the docs do not say it. The defaults for capabilities and
  context size (`image_in,thinking`, 262144) rarely fit a local model.
- **Goose, LM Studio:** the declarative provider definition names `lmstudio`, reads
  `LMSTUDIO_HOST` (without `/v1`, it appends `/v1/chat/completions`) and needs no key; the
  environment wins over the config file.

## Open
- OpenCode 2.x could be offered with `--standalone` and `OPENCODE_CONFIG_CONTENT` (in the source
  at v2.4.2, not in the docs); it needs a version switch (1.x has another schema) and must be
  merged with the JSON Hangar's gate already sets for OpenCode.
- Mistral Vibe could take `VIBE_PROVIDERS`, `VIBE_MODELS` and `VIBE_ACTIVE_MODEL` as JSON in the
  environment (tests and source at v3.1.5); whether the environment reaches its app server is not
  verified.

## Sources
1. Hermes Agent at `28ef6e4e7d1df3015bf786dea6b0d355baf5c211`:
   https://github.com/NousResearch/hermes-agent/blob/28ef6e4e7d1df3015bf786dea6b0d355baf5c211/website/docs/reference/cli-commands.md,
   https://github.com/NousResearch/hermes-agent/blob/28ef6e4e7d1df3015bf786dea6b0d355baf5c211/website/docs/integrations/providers.md,
   https://github.com/NousResearch/hermes-agent/blob/28ef6e4e7d1df3015bf786dea6b0d355baf5c211/hermes_cli/_parser.py,
   https://github.com/NousResearch/hermes-agent/blob/28ef6e4e7d1df3015bf786dea6b0d355baf5c211/hermes_cli/main.py,
   https://github.com/NousResearch/hermes-agent/blob/28ef6e4e7d1df3015bf786dea6b0d355baf5c211/hermes_cli/main_tui_launch.py
2. Ollama, Hermes: https://docs.ollama.com/integrations/hermes
3. Ollama, Copilot CLI: https://docs.ollama.com/integrations/copilot-cli
4. Copilot CLI changelog (read at 1.0.116): https://raw.githubusercontent.com/github/copilot-cli/main/changelog.md
5. GitHub docs: https://docs.github.com/en/copilot/how-tos/copilot-cli/customize-copilot/use-byok-models,
   https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/authenticate-copilot-cli,
   https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference
6. Qwen Code at `1a5c8bb49fa388b81e4df8dcf7b37f47a3d8ca07`:
   https://github.com/QwenLM/qwen-code/blob/1a5c8bb49fa388b81e4df8dcf7b37f47a3d8ca07/packages/cli/src/utils/modelConfigUtils.ts,
   https://github.com/QwenLM/qwen-code/blob/1a5c8bb49fa388b81e4df8dcf7b37f47a3d8ca07/packages/cli/src/config/top-level-options.ts,
   https://github.com/QwenLM/qwen-code/blob/1a5c8bb49fa388b81e4df8dcf7b37f47a3d8ca07/packages/cli/src/config/config.ts,
   https://github.com/QwenLM/qwen-code/blob/1a5c8bb49fa388b81e4df8dcf7b37f47a3d8ca07/packages/cli/src/ui/auth/useAuth.ts,
   https://github.com/QwenLM/qwen-code/blob/1a5c8bb49fa388b81e4df8dcf7b37f47a3d8ca07/docs/users/configuration/auth.md,
   https://github.com/QwenLM/qwen-code/blob/1a5c8bb49fa388b81e4df8dcf7b37f47a3d8ca07/docs/users/configuration/model-providers.md
7. Oh My Pi at `deddfd6134fbe856ef01cc306fb716fe6b91daa4`:
   https://github.com/can1357/oh-my-pi/blob/deddfd6134fbe856ef01cc306fb716fe6b91daa4/docs/providers.md,
   https://github.com/can1357/oh-my-pi/blob/deddfd6134fbe856ef01cc306fb716fe6b91daa4/docs/cli-reference.md,
   https://github.com/can1357/oh-my-pi/blob/deddfd6134fbe856ef01cc306fb716fe6b91daa4/docs/environment-variables.md
8. Gemini CLI: https://raw.githubusercontent.com/google-gemini/gemini-cli/main/docs/reference/configuration.md;
   Ollama API: https://docs.ollama.com/api/openai; LM Studio API: https://lmstudio.ai/docs/developer
9. Antigravity: https://antigravity.google/docs/install?tab=cli, https://antigravity.google/docs/cli/reference/,
   https://antigravity.google/docs/sdk/local-models/
10. Cursor CLI: https://cursor.com/docs/cli/reference/parameters
11. Kimi Code at `9304e3b26dc67cecbcdccc5c6af584f26713cf4c`:
    https://github.com/MoonshotAI/kimi-code/blob/9304e3b26dc67cecbcdccc5c6af584f26713cf4c/docs/en/configuration/env-vars.md,
    https://github.com/MoonshotAI/kimi-code/blob/9304e3b26dc67cecbcdccc5c6af584f26713cf4c/docs/en/guides/getting-started.md
    (section "First launch")
12. Kiro: https://kiro.dev/docs/cli/chat/model-selection/, https://kiro.dev/docs/cli/authentication
13. OpenCode at `v2.4.2`: https://github.com/anomalyco/opencode/blob/v2.4.2/packages/cli/src/commands/commands.ts,
    https://github.com/anomalyco/opencode/blob/v2.4.2/services/site/src/docs/v1/content/cli.mdx,
    https://opencode.ai/docs/providers/
14. Goose at `v1.61.1`: https://github.com/aaif-goose/goose/blob/v1.61.1/documentation/docs/guides/config-files.md,
    https://github.com/aaif-goose/goose/blob/v1.61.1/documentation/docs/guides/goose-cli-commands.md,
    https://github.com/aaif-goose/goose/blob/v1.61.1/documentation/docs/guides/custom-distributions.md,
    https://github.com/aaif-goose/goose/blob/v1.61.1/documentation/docs/getting-started/providers.md,
    https://github.com/aaif-goose/goose/blob/v1.61.1/crates/goose-cli/src/session/builder.rs,
    https://github.com/aaif-goose/goose/blob/v1.61.1/crates/goose-cli/src/cli.rs,
    https://github.com/aaif-goose/goose/blob/v1.61.1/crates/oi-kit-providers/src/declarative/definitions/lmstudio.json
15. Crush: https://github.com/charmbracelet/crush/blob/main/README.md
16. Pi at `315e4c78c051392ba3598fc2b9d1e10904100b58`:
    https://github.com/earendil-works/pi/blob/315e4c78c051392ba3598fc2b9d1e10904100b58/packages/coding-agent/docs/models.md
17. ForgeCode at `a22b49288856d5b3f3ca57e8532683d54a662c8c`:
    https://github.com/tailcallhq/forgecode/blob/a22b49288856d5b3f3ca57e8532683d54a662c8c/crates/forge_repo/src/provider/provider_repo.rs
18. Kode CLI at `2b8a532ce9a7d4ca287306eceed446373fb79002`:
    https://github.com/shareAI-lab/Kode-CLI/blob/2b8a532ce9a7d4ca287306eceed446373fb79002/apps/cli/src/entrypoints/cli/cliParser/rootAction.ts
19. Codebuff (repository CodebuffAI/freebuff at `d1ffb2e390da0068957f946dad64f3da8c7afd45`):
    https://github.com/CodebuffAI/freebuff/blob/d1ffb2e390da0068957f946dad64f3da8c7afd45/cli/src/cli-args.ts,
    https://github.com/CodebuffAI/freebuff/blob/d1ffb2e390da0068957f946dad64f3da8c7afd45/cli/src/commands/byok.ts
20. Cline CLI: https://docs.cline.bot/cli/cli-reference.md; source `apps/cli/src/main.ts` in
    https://github.com/cline/cline at `cbc7d14`
21. Mistral Vibe: https://docs.mistral.ai/vibe/code/cli/work-with-cli, https://docs.mistral.ai/vibe/code/cli/offline-models
22. Droid: https://docs.factory.com/cli/byok/overview, https://docs.factory.com/reference/cli-reference
23. Amp: https://ampcode.com/manual, https://ampcode.com/models
24. OpenClaw: https://docs.openclaw.ai/cli/tui, https://docs.openclaw.ai/providers/ollama,
    https://docs.openclaw.ai/providers/lmstudio, https://lmstudio.ai/docs/integrations/openclaw
25. LM Studio, OpenAI compatible endpoints: https://lmstudio.ai/docs/developer/openai-compat
26. Kimi Code at `62402f173c5a838a1c1f8fa263b637a22486fa39`:
    https://github.com/MoonshotAI/kimi-code/blob/62402f173c5a838a1c1f8fa263b637a22486fa39/docs/en/configuration/env-vars.md,
    https://github.com/MoonshotAI/kimi-code/blob/62402f173c5a838a1c1f8fa263b637a22486fa39/packages/agent-core-v2/src/app/kosongConfig/envOverlay.ts,
    https://github.com/MoonshotAI/kimi-code/blob/62402f173c5a838a1c1f8fa263b637a22486fa39/apps/kimi-code/src/tui/components/chrome/welcome.ts
