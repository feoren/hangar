---
title: Agent catalog
type: agent
tags: [agents, catalog, installation]
sources: ["[[agent-clis-research]]", "[[agent-icons]]", "[[agent-commands-measurement]]", "[[agent-vendors-licenses]]", "[[0022-models-and-local-llm-servers]]"]
created: 2026-11-07
updated: 2026-11-27
---

# Agent catalog

Hangar ships a built-in catalog of well-known terminal coding agents
(`src-tauri/src/agent_catalog.toml`). For each entry Hangar checks PATH (and PATHEXT on Windows) for
one of the program names, on every list request (at startup and whenever the window regains focus),
so that freshly installed agents appear without a restart.
User profiles in `profiles.toml` with the same `id` replace the catalog entry's name, icon, command, arguments and
position; the homepage and install command still come from the catalog, so that
existing users with the old default profiles also see how to install a missing agent.
Entries that are not installed appear dimmed in the sidebar under "Not installed (n)"; a click
copies the install command, which Hangar itself never runs.

## Inclusion rules
- Only agents that exist as an interactive terminal application and are documented in an official source
  ([[agent-clis-research]]).
- Install commands only if the vendor documentation lists them; otherwise the field stays empty.
- Generic program names only if the vendor uses them. The collision risk is noted here:
  `agent` (Cursor CLI), `kd` (Kode, hence not in the catalog), `q` (old Amazon Q CLI, hence not in the catalog).
- Not in the catalog: Warp (a terminal application), VibePod (container wrapper), Plandex (winding down, WSL only),
  Mini-Kode (learning project), nori-cli (niche).
- `resume_args` (resuming after a Hangar restart) only with evidence; values and sources in [[resume]].
- `latest` and `update_hint` (showing a newer version) only with evidence; values and sources in [[updates]].

## Agent commands
`agent_commands` lists the slash commands the session menu, quick access and the compact button
offer for a running session ([[agent-commands]]). Only commands found in the installed program or
its docs ([[agent-commands-measurement]]):

- Claude Code 2.1.325: `/compact`, `/context`, `/usage` (`/cost` is an alias), `/model`, `/clear`
  (asks first).
- Codex CLI 0.169.0: `/compact`, `/status`, `/model`, `/new` (asks first).
- Gemini CLI (docs only, not installed): `/compress`, `/stats`, `/model`, `/clear` (asks first).

All other catalog agents have none until a measurement; a test requires that.

## Models and local servers
Three optional fields per entry choose the model a session starts with; the decision, the values
and their sources are in [[0022-models-and-local-llm-servers]]. A user profile with the same `id`
and program inherits each of them it does not set itself; its own value replaces the catalog's.

- `models`: the vendor's models the "›" menu of a program row in "New session" offers, in order,
  as `{ id, label }`. Claude Code (Fable, Opus, Sonnet, Haiku), Codex CLI (GPT-6.2 Sol, GPT-6.2
  Astra, GPT-6.1 Terra, GPT-6 Luna) and Gemini CLI (Auto, Pro, Flash, Flash Lite) have them.
- `model_args`: how a vendor model goes into the start, with the placeholder `{model}`; required
  with `models`. Placed after the start arguments (on a resume after `resume_args`; the CLIs
  accept the model flag there, evidence in the decision).
- `local`: how the program runs against Ollama or LM Studio on this machine, `args` with
  `{model}` and `env` with `{model}` and `{base}`. Claude Code, Codex CLI and Aider run against
  both, Hermes Agent against LM Studio only.

The id of a model is checked before anything starts; a new start with a model the program does
not offer is refused, a restored session whose stored model is gone starts with the program's
default.

## Hermes Agent and OpenClaw
- `hermes` (Nous Research): starts with `hermes`; with an LM Studio model as
  `hermes chat --provider lmstudio -m <model>`. Ollama needs a custom endpoint in
  `~/.hermes/config.yaml`, so it is not offered.
- `openclaw`: starts as `openclaw tui --local`, which needs no gateway. It has no model flag at the
  start (models change with `/model` inside the session), so it offers no models.

Both have a monogram icon (no logo under a clear license, `src/assets/agent-icons/NOTICE.md`) and a
description in all 12 languages. No agent commands until a measurement.

## Descriptions
Every catalog entry has a one-line description in all 12 languages (`agent.<id>.description` in
`src/i18n.ts`, shown under "Available" in Settings > Programs). One pattern for all: what it is, the
vendor, and "open source (license)" only where the LICENSE file proves it. Vendors, licenses and
the pattern with its tests: [[agent-vendors-licenses]].

## Icons
Every catalog entry has a bundled icon, `src/assets/agent-icons/<id>.svg`: a rounded 24 × 24 tile
in the product's color with its mark in white (10 marks from Simple Icons) or a monogram drawn for
Hangar (14 products without a permissively licensed mark). Sources, licenses and trademark notes
are in `src/assets/agent-icons/NOTICE.md`, evidence in [[agent-icons]]. The tile carries the color,
so the icon reads the same in the light and dark theme.

"Automatic" in Settings picks, in this order: the bundled icon, the icon of the program file
(Windows .exe), the letters from `icon` on a color (`resolveBadge` in `src/programPrefs.ts`). The
bundled icon wins because the program icon depends on the install method: on Windows only the
native `claude.exe` carries one, npm and uv installs do not ([[agent-icons]]). Shells and the
user's own programs have no bundled icon and keep their exe icon. The user's own icon (letters or
image) wins over all of these, and "Show program icons" off shows letters everywhere. A user
profile with a catalog id is that agent and gets its icon too. A test (`src/agentIcons.test.ts`)
requires an icon for every catalog id and allows only small static SVG (no scripts, styles,
entities or links).

> [!question] Unverified
> For many agents there is no evidence yet that they run natively on Windows (Gemini, ForgeCode, Codebuff, Cline,
> Droid, Amp, OpenCode), nor that they start interactively without arguments (Gemini, Kimi, Qwen, OpenCode,
> Crush, Kode, Aider). According to its docs, Goose starts with `goose session`.
