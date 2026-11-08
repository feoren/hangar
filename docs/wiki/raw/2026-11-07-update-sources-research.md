# Research: version sources per agent (as of 2026-11-07, queried the same day)

Technology: reqwest 0.13.5 in Cargo.lock (via tauri-plugin-updater 2.14.1), TLS rustls 0.23.46, no native-tls. Features json/rustls for our own call [UNVERIFIED] → `cargo tree -e features -i reqwest`.
Rate limits: GitHub unauthenticated 60/h per IP (User-Agent mandatory). npm/PyPI fine without auth. Set our own User-Agent `Hangar/<version>`. Prefer npm/PyPI over GitHub, cache 6 to 24 h.
Endpoints: npm `https://registry.npmjs.org/<pkg>/latest` (version; only /latest, never dist-tags), PyPI `https://pypi.org/pypi/<pkg>/json` (info.version), GitHub `https://api.github.com/repos/<o>/<r>/releases/latest` (tag_name; never /releases).

| ID | latest source | Update command | Notes |
|---|---|---|---|
| claude | npm @anthropic-ai/claude-code | `claude update` (npm: `npm i -g @anthropic-ai/claude-code@latest`) | auto-update with the native installer; --version "2.1.319 (Claude Code)" |
| codex | npm @openai/codex | rerun installer / `npm i -g @openai/codex@latest` | GitHub tag `rust-v…`; local 0.156.1 vs 0.167.0 (test case) |
| gemini | npm @google/gemini-cli | `npm install -g @google/gemini-cli@latest` | preview/nightly with suffix → only /latest |
| antigravity | none | rerun installer | |
| copilot | npm @github/copilot (GitHub github/copilot-cli, v prefix) | Win `winget upgrade GitHub.Copilot`; Unix installer / npm | |
| cursor | none | rerun installer | own updater [UNVERIFIED] |
| kimi | PyPI kimi-cli | rerun installer / `uv tool upgrade kimi-cli` | |
| qwen | npm @qwen-code/qwen-code | installer / `npm i -g @qwen-code/qwen-code@latest` | package name not evidenced in the catalog |
| kiro | none | rerun installer | |
| opencode | npm opencode-ai (GitHub anomalyco/opencode, v) | `opencode upgrade` [UNVERIFIED] | catalog names @opencode/cli (2.x line) ≠ opencode-ai (1.x) → clarify; local 1.15.7 vs 1.19.5 (test case) |
| aider | PyPI aider-chat | `uv tool upgrade aider-chat` / `pipx upgrade aider-chat` [UNVERIFIED route] | installer aider-install |
| goose | GitHub aaif-goose/goose (v) | rerun installer | |
| crush | npm @charmland/crush (GitHub charmbracelet/crush, v) | Win `winget upgrade charmbracelet.crush`; Unix npm | |
| pi | npm @earendil-works/pi-coding-agent (GitHub earendil-works/pi, v) | rerun installer (pi.dev/install) | |
| omp | npm @oh-my-pi/pi-coding-agent | rerun installer | |
| forge | npm forgecode | rerun installer | mapping to the installer unverified |
| kode | npm @shareai-lab/kode | `npm i -g @shareai-lab/kode@latest` | |
| codebuff | npm codebuff | `npm i -g codebuff@latest` | |
| cline | npm cline | `npm i -g cline@latest` | |
| vibe | PyPI mistral-vibe | `uv tool upgrade mistral-vibe` | |
| droid | none | n/a | |
| amp | npm @sourcegraph/amp | `npm i -g @sourcegraph/amp@latest` | version 0.0.<unix>-g<hash> → compare="equal" or exclude |
| pwsh | GitHub PowerShell/PowerShell /releases/latest (v) | Win `winget upgrade Microsoft.PowerShell`; mac `brew upgrade --cask powershell` | preview not via /latest |
| powershell, cmd | part of Windows | n/a | no query |

Catalog field proposal: `latest = { npm = "…" } | { pypi = "…" } | { github = "o/r", tag_prefix = "v" }`, optional `compare = "equal"`; `update_hint = { windows = "…", unix = "…" }`; latest missing → show nothing; update_hint missing → install_* as the hint.

# Research resume (resume_args): handed over to the backend worktree agent, measured there.
