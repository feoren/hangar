---
title: Newer program versions (latest, update_hint)
type: agent
tags: [agents, updates, catalog]
sources: ["[[update-sources]]", "[[agent-clis-research]]"]
created: 2026-11-07
updated: 2026-11-08
---

# Newer program versions (latest, update_hint)

In the catalog and in profiles, `latest` says where to find a program's newest version; `update_hint` is
the update command. Hangar shows it and, on a click, runs it as a session of its own (argv
without a shell, rules in [[program-updates]]); it therefore has to be a simple command without shell
syntax. If there is none for the platform, Hangar shows `install_*` for copying only and runs nothing. Code:
`src-tauri/src/updates.rs`, values: `src-tauri/src/agent_catalog.toml`. Contract: [[program-updates]].

## Values (as of 2026-11-07, [[update-sources]])

| id | latest | update_hint |
|---|---|---|
| claude | npm `@anthropic-ai/claude-code` | `claude update` |
| codex | npm `@openai/codex` | (install command) |
| gemini | npm `@google/gemini-cli` | `npm install -g @google/gemini-cli@latest` |
| copilot | npm `@github/copilot` | Windows `winget upgrade GitHub.Copilot`, otherwise install command |
| kimi | PyPI `kimi-cli` | (install command) |
| qwen | npm `@qwen-code/qwen-code` | (install command) |
| opencode | GitHub `anomalyco/opencode` (prefix `v`) | `opencode upgrade` |
| aider | PyPI `aider-chat` | (install command) |
| goose | GitHub `aaif-goose/goose`, prefix `v` | (install command) |
| crush | npm `@charmland/crush` | Windows `winget upgrade charmbracelet.crush`, Unix `npm install -g @charmland/crush@latest` |
| pi | npm `@earendil-works/pi-coding-agent` | `pi update` |
| omp | npm `@oh-my-pi/pi-coding-agent` | (install command) |
| kode | npm `@shareai-lab/kode` | `npm install -g @shareai-lab/kode@latest` |
| codebuff | npm `codebuff` | `npm install -g codebuff@latest` |
| cline | npm `cline` | `npm install -g cline@latest` |
| vibe | PyPI `mistral-vibe` | Windows `uv tool upgrade mistral-vibe`, otherwise install command |
| amp | npm `@sourcegraph/amp`, `compare = "equal"` | `npm install -g @sourcegraph/amp@latest` |
| pwsh (default profile) | GitHub `PowerShell/PowerShell`, prefix `v` | Windows `winget upgrade Microsoft.PowerShell` |

No source: antigravity, cursor, kiro, droid (none documented), forge (mapping from package to installer
unverified), powershell and cmd (part of Windows).

## Decisions and limits
- **Run only from `update_hint`:** decision of 2026-11-08. A test checks that every
  `update_hint` in the catalog can be split into argv. An invalid `update_hint` in a user profile does not prevent
  loading; it yields `canRun: false`.
  > [!question] Unverified
  > Whether `winget upgrade` without `--accept-source-agreements` prompts on its first run (the user then
  > answers in the terminal), and whether `claude update`, `pi update` and `opencode upgrade` fail
  > while the same program is running in another session.
- **OpenCode:** two npm packages ship `opencode` (`@opencode/cli` 2.x, `opencode-ai` 1.x). The source is
  therefore the GitHub release of `anomalyco/opencode` (2026-11-07: v1.19.5), matching `opencode-ai`
  and `opencode upgrade`. Decision of 2026-11-07: with npm `@opencode/cli`, 1.x installations would
  permanently see an "update" to 2.x that `opencode upgrade` does not deliver.
  > [!question] Unverified
  > Whether `opencode upgrade` switches from 1.x to the `@opencode/cli` package, and whether both packages can be
  > installed globally side by side (same program name).
- **amp:** versions of the form `0.0.<time>-g<hash>` cannot be ordered meaningfully; `compare = "equal"`
  reports an update on any difference.
- **pwsh in existing profiles.toml files:** the default profiles are only written on first start.
  So that older files still get the source, Hangar adds it for profiles that run the program `pwsh`
  without a `latest` of their own (`updates::builtin_for_command`).
- **Comparison:** numeric over the first up to four groups of digits (as in version detection,
  [[0003-program-versions]]); not comparable means "no update".
- **Installation channel:** the same version can arrive through different channels (installer, npm, winget).
  The source only says which version is the newest, not how it gets onto this machine.

> [!question] Unverified
> qwen: the standalone installer and the npm package `@qwen-code/qwen-code` presumably share the same
> version sequence. kimi, vibe: whether `uv tool upgrade` also applies to installations through the vendor script.
