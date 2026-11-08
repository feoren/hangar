---
title: Newer program versions (latest, packages, self_update, update_hint)
type: agent
tags: [agents, updates, catalog]
sources: ["[[update-sources]]", "[[agent-clis-research]]", "[[update-installers]]"]
created: 2026-11-07
updated: 2026-11-08
---

# Newer program versions (latest, packages, self_update, update_hint)

In the catalog and in profiles, `latest` says where to find a program's newest version. How to
update is decided per machine: Hangar detects from the resolved path how the started copy was
installed and runs that installer's command with the package name from `packages`, or the
program's own `self_update` (rules and order in [[program-updates]], evidence in
[[update-installers]]). `update_hint` is an override for a profile; the catalog no longer sets
one, because a fixed command runs the wrong installer for a copy installed another way. If no
plan can run, Hangar shows `install_*` for copying only. Code: `src-tauri/src/install_source.rs`,
`src-tauri/src/updates.rs`, values: `src-tauri/src/agent_catalog.toml`.

## Values (as of 2026-11-08, [[update-sources]], [[update-installers]])

| id | latest | packages | self_update |
|---|---|---|---|
| claude | npm `@anthropic-ai/claude-code` | npm, winget `Anthropic.ClaudeCode` | `update`, aware |
| codex | npm `@openai/codex` | npm, winget `OpenAI.Codex` | `update`, aware |
| gemini | npm `@google/gemini-cli` | npm | |
| copilot | npm `@github/copilot` | npm, winget `GitHub.Copilot` | |
| kimi | npm `@moonshot-ai/kimi-code` | npm, winget `MoonshotAI.KimiCodeCLI` | |
| qwen | npm `@qwen-code/qwen-code` | npm | |
| opencode | GitHub `anomalyco/opencode` (prefix `v`) | npm `opencode-ai` | `upgrade`, aware |
| aider | PyPI `aider-chat` | PyPI (uv, pipx) | |
| goose | GitHub `aaif-goose/goose`, prefix `v` | | `update`, aware |
| crush | npm `@charmland/crush` | npm, winget `charmbracelet.crush` | |
| pi | npm `@earendil-works/pi-coding-agent` | npm | `update`, aware |
| omp | npm `@oh-my-pi/pi-coding-agent` | npm | |
| kode | npm `@shareai-lab/kode` | npm | |
| codebuff | npm `codebuff` | npm | |
| cline | npm `cline` | npm | |
| vibe | PyPI `mistral-vibe` | PyPI (uv, pipx) | |
| amp | npm `@sourcegraph/amp`, `compare = "equal"` | npm, winget `Sourcegraph.Amp` | |
| pwsh (default profile) | GitHub `PowerShell/PowerShell`, prefix `v` | winget `Microsoft.PowerShell` | |
| Git Bash (built in) | GitHub `git-for-windows/git`, prefix `v` | winget `Git.Git` | |

"npm" in the packages column means the same name as in `latest`. Left out on purpose: winget for
opencode (`SST.opencode` and `Anomaly.OpenCode` both exist, which one belongs to this line is
open), `aider --upgrade` (pip inside its own environment; uv and pipx keep their receipt right with
their own upgrade), `vibe --check-upgrade` (only checks), and the look-alikes winget
`Pressly.Goose`, `Google.Antigravity`, `Anysphere.Cursor` and npm `cursor-agent`. kimi moved from
the archived PyPI `kimi-cli` to npm `@moonshot-ai/kimi-code` ([[update-installers]]).

No source: antigravity, cursor, kiro, droid (none documented), forge (mapping from package to installer
unverified), powershell and cmd (part of Windows).

## Decisions and limits
- **Run from a plan per machine:** decision of 2026-11-08 (supersedes "run only from
  `update_hint`" of the same day). The catalog carries `packages` and `self_update` instead of
  fixed commands; a test checks names, look-alikes and that the catalog sets no `update_hint`.
  An invalid `update_hint` in a user profile does not prevent loading; it yields `plan: "copy"`
  with `reason: "invalid-hint"`.
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
  So that older files still get the source, Hangar adds it (with the winget package) for profiles
  that run the program `pwsh` without a `latest` of their own (`updates::builtin_for_command`).
  Older files may still hold `update_hint = { windows = "winget upgrade Microsoft.PowerShell" }`;
  it stays an override and keeps working.
- **Git Bash:** a profile whose program resolves to the `bash.exe` of Git for Windows gets the
  Git for Windows source and winget package (`updates::builtin_for_program`); its version is that
  of `git.exe`.
- **Comparison:** numeric over the first up to four groups of digits (as in version detection,
  [[0003-program-versions]]), with `X.Y.Z.windows.N` read as `X.Y.Z.N`; not comparable means
  "no update".
- **Installation channel:** the same version can arrive through different channels (installer, npm, winget).
  The source only says which version is the newest, not how it gets onto this machine.

> [!question] Unverified
> qwen: the standalone installer and the npm package `@qwen-code/qwen-code` presumably share the same
> version sequence. kimi, vibe: whether `uv tool upgrade` also applies to installations through the vendor script.
