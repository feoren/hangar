---
title: "Install locations and update paths of the catalog programs"
type: source
tags: [agents, updates, installers, winget, npm, uv, brew]
sources: []
raw: raw/2026-11-08-update-installers-measurement.md
url: https://code.claude.com/docs/en/setup
retrieved: 2026-11-08
created: 2026-11-08
updated: 2026-11-08
---

# Install locations and update paths of the catalog programs

Measurement on Windows 11 plus vendor documentation read the same day. Purpose: run the right
update with one click ([[program-updates]], [[updates]]).

## Key points
- The installer follows from the resolved executable path plus file existence checks, without
  starting a process: npm (shim folder contains `node_modules\<package>`), uv tool
  (`%APPDATA%\uv\tools\<package>\uv-receipt.toml` whose install path matches), winget portable
  (`WinGet\Packages`, `WinGet\Links`), Homebrew (`Cellar` or `Caskroom` in the realpath). A bin
  folder alone never decides: `~/.local/bin` holds native Claude Code, uv tool and pipx shims.
- Process calls (warm): `uv tool dir` about 50 ms, `npm prefix -g` about 250 ms,
  `winget list --id X --exact` about 550 ms, `npm ls -g --json` about 850 ms.
- `winget list` does not tell which copy runs: a stale winget copy of Claude Code (2.1.317) was off
  PATH while the native 2.1.319 ran; Codex had an npm and a native copy at once.
- Machine-scope installs (PowerShell 7 MSI, Git for Windows) show up in HKLM Uninstall keys.
- Installation-aware self-updaters: `claude update`, `codex update`, `opencode upgrade` (refuses a
  major jump), `pi update`, `aider --upgrade`, `goose update`, `vibe --check-upgrade`.
- npm 12 skips postinstall steps by default; `npm install -g` can break packages that need them
  (documented by Claude Code).
- Catalog entry `kimi` uses the archived PyPI package `kimi-cli`; the current CLI is
  `@moonshot-ai/kimi-code` on npm and `MoonshotAI.KimiCodeCLI` on winget.
- Look-alikes to avoid: winget `Pressly.Goose` (database tool), `Google.Antigravity` and
  `Anysphere.Cursor` (IDEs), `cursor-agent` on npm; a VS Code `copilot` launcher shim is not an install.

## Significance for Hangar
Catalog fields `packages` (per installer) and `self_update` (with an `aware` flag), and a pure
detection module that maps the resolved path to an installer and builds argv from catalog data only;
ambiguous or unknown installs get copy text with a reason instead of a run button.

> [!question] Unverified
> UAC behavior of `winget upgrade` for machine installs, winget exit codes, pnpm and pipx folder
> layouts, uv defaults on Unix, `pi update` on npm installs, `opencode upgrade` on Windows npm
> installs, the Git for Windows tag format and `update-git-for-windows` in 2.56.

## Sources
- https://code.claude.com/docs/en/setup
- https://code.claude.com/docs/en/troubleshoot-install.md
- https://github.com/openai/codex
- https://github.com/github/copilot-cli
- https://raw.githubusercontent.com/anomalyco/opencode/v1.19.5/packages/opencode/src/installation/index.ts
- https://github.com/mistralai/mistral-vibe
- https://github.com/MoonshotAI/kimi-cli
- https://aider.chat/docs/install.html
- https://docs.astral.sh/uv/concepts/tools/
- https://github.com/microsoft/winget-cli/blob/master/doc/windows/package-manager/winget/upgrade.md
