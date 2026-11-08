---
title: Agent catalog
type: agent
tags: [agents, catalog, installation]
sources: ["[[agent-clis-research]]"]
created: 2026-11-07
updated: 2026-11-07
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

> [!question] Unverified
> For many agents there is no evidence yet that they run natively on Windows (Gemini, ForgeCode, Codebuff, Cline,
> Droid, Amp, OpenCode), nor that they start interactively without arguments (Gemini, Kimi, Qwen, OpenCode,
> Crush, Kode, Aider). According to its docs, Goose starts with `goose session`.
