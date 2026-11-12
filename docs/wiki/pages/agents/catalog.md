---
title: Agent catalog
type: agent
tags: [agents, catalog, installation]
sources: ["[[agent-clis-research]]", "[[agent-icons]]"]
created: 2026-11-07
updated: 2026-11-12
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

## Icons
Every catalog entry has a bundled icon, `src/assets/agent-icons/<id>.svg`: a rounded 24 × 24 tile
in the product's color with its mark in white (10 marks from Simple Icons) or a monogram drawn for
Hangar (12 products without a permissively licensed mark). Sources, licenses and trademark notes
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
