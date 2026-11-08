# Research: terminal coding agents (as of 2026-11-07)

Research report by the project agent `researcher`, based solely on official READMEs, documentation and package pages.
Commands were not run on a machine. `[UNVERIFIED]` = not evidenced.

## Result
- 19 of the 24 entries on the initial list exist as terminal agents and are included.
- Not included: Plandex (cloud being phased out since 2025-10-03, Windows only via WSL), Mini-Kode (learning project),
  Warp (standalone terminal program), VibePod (`vp`, wrapper for Docker/Podman), nori-cli (niche, Windows not evidenced).
- Additionally found: Mistral Vibe (`vibe`), Droid by Factory (`droid`), Amp (`amp`, installation [UNVERIFIED]).
- Renames:
  - Kimi CLI is archived; its successor Kimi Code CLI is still called `kimi`.
  - Amazon Q Developer CLI is no longer maintained; new: Kiro CLI (`kiro-cli`, closed source).
  - Gemini CLI: free access for individual users moved to Antigravity CLI (`agy`) on 2026-06-18.
  - Pi moved to `earendil-works`.
  - Cursor: the current documentation calls the program `agent`, not `cursor-agent`.

## Program names and installation

| Name | Program | Windows | Installation Windows | Installation macOS/Linux | Source | License |
|---|---|---|---|---|---|---|
| Claude Code | `claude` | native | `irm https://claude.ai/install.ps1 \| iex` | `curl -fsSL https://claude.ai/install.sh \| bash` | github.com/anthropics/claude-code | proprietary |
| Codex CLI | `codex` | native | `powershell -ExecutionPolicy ByPass -c "irm https://chatgpt.com/codex/install.ps1 \| iex"` | `curl -fsSL https://chatgpt.com/codex/install.sh \| sh` | github.com/openai/codex | Apache-2.0 |
| Gemini CLI | `gemini` | [UNVERIFIED] | `npm install -g @google/gemini-cli` | same | github.com/google-gemini/gemini-cli | Apache-2.0 |
| Antigravity CLI | `agy` | native | `irm https://antigravity.google/cli/install.ps1 \| iex` | `curl -fsSL https://antigravity.google/cli/install.sh \| bash` | antigravity.google | proprietary [UNVERIFIED] |
| GitHub Copilot CLI | `copilot` | native (PowerShell 6+) | `winget install GitHub.Copilot` | `curl -fsSL https://gh.io/copilot-install \| bash` | github.com/github/copilot-cli | [UNVERIFIED] |
| Cursor CLI | `agent` (old `cursor-agent` [UNVERIFIED]) | native | `irm 'https://cursor.com/install?win32=true' \| iex` | `curl https://cursor.com/install -fsS \| bash` | cursor.com/cli | proprietary [UNVERIFIED] |
| Kimi Code CLI | `kimi` | native | `irm https://code.kimi.com/kimi-code/install.ps1 \| iex` | `curl -fsSL https://code.kimi.com/kimi-code/install.sh \| bash` | github.com/MoonshotAI/kimi-cli | Apache-2.0 |
| Qwen Code | `qwen` | native | standalone script (see catalog) | standalone script, `npm install -g @qwen-code/qwen-code@latest` | github.com/QwenLM/qwen-code | [UNVERIFIED] |
| Kiro CLI (formerly Amazon Q) | `kiro-cli` (old `q`) | Windows 11 per documentation, command not evidenced | n/a | `curl -fsSL https://cli.kiro.dev/install \| bash` | kiro.dev | closed source |
| OpenCode | `opencode` | ZIP x64/ARM64, native unclear | `npm install -g @opencode/cli` | `curl -fsSL https://opencode.ai/v2/install \| bash` | opencode.ai | MIT |
| Aider | `aider` | native (Python) [UNVERIFIED] | `python -m pip install aider-install && aider-install` | same | github.com/Aider-AI/aider | [UNVERIFIED] |
| Goose | `goose` (start with `goose session`) | native | PowerShell script `download_cli.ps1` | `curl -fsSL …/download_cli.sh \| bash` | github.com/aaif-goose/goose | Apache-2.0 |
| Crush | `crush` | native | `winget install charmbracelet.crush` | `npm install -g @charmland/crush` | github.com/charmbracelet/crush | FSL-1.1-MIT |
| Pi | `pi` | native | `powershell -c "irm https://pi.dev/install.ps1 \| iex"` | `curl -fsSL https://pi.dev/install.sh \| sh` | github.com/earendil-works/pi | MIT |
| Oh My Pi | `omp` | native | `irm https://omp.sh/install.ps1 \| iex` | `curl -fsSL https://omp.sh/install \| sh` | github.com/can1357/oh-my-pi | MIT |
| ForgeCode | `forge` | [UNVERIFIED] | n/a | `curl -fsSL https://forgecode.dev/cli \| sh` | github.com/antinomyhq/forge | [UNVERIFIED] |
| Kode CLI | `kode`, `kwa`, `kd` | native (npm) | `npm install -g @shareai-lab/kode` | same | github.com/shareAI-lab/Kode-CLI | Apache-2.0 |
| Codebuff | `codebuff` | [UNVERIFIED] | `npm install -g codebuff` | same | codebuff.com/docs | [UNVERIFIED] |
| Cline CLI | `cline` | [UNVERIFIED] | `npm i -g cline` | same | docs.cline.bot/cline-cli/overview | [UNVERIFIED] |
| Mistral Vibe | `vibe` | runs, target UNIX | `uv tool install mistral-vibe` | `curl -LsSf https://mistral.ai/vibe/install.sh \| bash` | github.com/mistralai/mistral-vibe | Apache-2.0 |
| Droid (Factory) | `droid` | [UNVERIFIED] | [UNVERIFIED] | [UNVERIFIED] | docs.factory.com/cli | [UNVERIFIED] |
| Amp | `amp` | [UNVERIFIED] | [UNVERIFIED] | [UNVERIFIED] | ampcode.com/manual | [UNVERIFIED] |

## Start without arguments
- Evidenced as interactive: `claude`, `codex`, `copilot`, `omp`, `pi`, `forge`, `codebuff`, `cline`, `agent`, `vibe`, `kiro-cli`.
- Likely, [UNVERIFIED]: `gemini`, `kimi`, `qwen`, `opencode`, `crush`, `kode`, `aider`.
- Goose: per documentation `goose session`. Antigravity: asks for color scheme, rendering mode and workspace trust on first start.

## Collision risks with program names
`agent` (Cursor), `q` (old Amazon Q CLI), `kd` (Kode) are generic and can hit other programs.

## Evidence (retrieved 2026-11-07)
Claude Code: raw.githubusercontent.com/anthropics/claude-code/main/README.md ·
Codex: raw.githubusercontent.com/openai/codex/main/README.md ·
Gemini CLI: raw.githubusercontent.com/google-gemini/gemini-cli/main/README.md ·
Antigravity: antigravity.google/docs/getting-started?tab=cli ·
Copilot CLI: raw.githubusercontent.com/github/copilot-cli/main/README.md ·
Cursor: cursor.com/docs/cli/installation, cursor.com/cli ·
Kimi: raw.githubusercontent.com/MoonshotAI/kimi-cli/main/README.md ·
Qwen Code: raw.githubusercontent.com/QwenLM/qwen-code/main/README.md ·
Kiro/Q: kiro.dev/docs/cli/, kiro.dev/docs/getting-started/installation/, raw.githubusercontent.com/aws/amazon-q-developer-cli/main/README.md ·
OpenCode: opencode.ai/docs/, raw.githubusercontent.com/anomalyco/opencode/dev/LICENSE ·
Aider: raw.githubusercontent.com/Aider-AI/aider/main/README.md ·
Goose: goose-docs.ai/docs/getting-started/installation ·
Crush: raw.githubusercontent.com/charmbracelet/crush/main/README.md ·
Pi: raw.githubusercontent.com/badlogic/pi-mono/main/packages/coding-agent/README.md ·
Oh My Pi: raw.githubusercontent.com/can1357/oh-my-pi/main/README.md ·
ForgeCode: raw.githubusercontent.com/antinomyhq/forge/main/README.md ·
Kode: raw.githubusercontent.com/shareAI-lab/Kode-cli/main/README.md ·
Codebuff: codebuff.com/docs ·
Cline: docs.cline.bot/cline-cli/overview ·
Plandex: raw.githubusercontent.com/plandex-ai/plandex/main/README.md ·
Mini-Kode: github.com/minmaxflow/mini-kode ·
Warp: docs.warp.dev/getting-started/installation-and-setup ·
VibePod: raw.githubusercontent.com/VibePod/vibepod-cli/main/README.md ·
nori-cli: raw.githubusercontent.com/tilework-tech/nori-cli/main/README.md ·
Mistral Vibe: raw.githubusercontent.com/mistralai/mistral-vibe/main/README.md ·
Droid: docs.factory.com/cli/getting-started/quickstart ·
Amp: ampcode.com/manual
