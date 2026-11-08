# Measurement: install locations and update paths of the catalog programs

- Date: 2026-11-08
- Platform: Windows 11 Pro 10.0.26300, x64, elevated shell (so UAC behavior was not tested)
- Read-only commands only. Nothing was installed, updated or removed. Home paths are
  written as %USERPROFILE%, %APPDATA%, %LOCALAPPDATA%.

## Resolved executables (where.exe, first hit is what Hangar starts)

| command | resolved path |
|---|---|
| claude | %USERPROFILE%\.local\bin\claude.exe |
| codex | %APPDATA%\npm\codex.cmd (first); %LOCALAPPDATA%\Programs\OpenAI\Codex\bin\codex.exe (later on PATH) |
| copilot | %LOCALAPPDATA%\Programs\Microsoft VS Code\bin\copilot-shim\copilot.exe (VS Code launcher, CLI itself missing) |
| opencode, pi | %APPDATA%\npm\opencode.cmd, %APPDATA%\npm\pi.cmd |
| aider | %USERPROFILE%\.local\bin\aider.exe |
| pwsh | C:\Program Files\PowerShell\7\pwsh.exe |
| git, bash | C:\Program Files\Git\cmd\git.exe, C:\Program Files\Git\usr\bin\bash.exe (WindowsApps bash.exe also on PATH) |
| npm, pnpm | %APPDATA%\npm\npm.cmd and C:\Program Files\nodejs\npm.cmd; pnpm.cmd in %APPDATA%\npm |
| winget | %LOCALAPPDATA%\Microsoft\WindowsApps\winget.exe (v1.29.380) |
| uv | %LOCALAPPDATA%\<other app>\bin\uv.exe (uv 0.12.1, bundled by another program) |
| not found | gemini, agy, cursor-agent, kimi, qwen, kiro-cli, goose, crush, omp, forge, kode, codebuff, cline, vibe, droid, amp, bun, scoop, choco, pipx, brew |

## npm (npm 12.0.2)
npm prefix -g -> %APPDATA%\npm. npm ls -g --depth=0 --json -> @earendil-works/pi-coding-agent 0.79.9,
@openai/codex 0.167.0, opencode-ai 1.15.7, pnpm 12.18.0. The shim folder is the prefix;
node_modules\<package> sits next to it.

## uv tool (uv 0.12.1)
uv tool dir -> %APPDATA%\uv\tools; uv tool dir --bin -> %USERPROFILE%\.local\bin.
%APPDATA%\uv\tools\aider-chat\uv-receipt.toml lists install-path %USERPROFILE%/.local/bin/aider.exe.

## winget (search and list only)
winget list -> Anthropic.ClaudeCode 2.1.317 (portable, not on PATH, WinGet\Links holds only rg.exe),
Git.Git 2.56.0.4, Microsoft.PowerShell 7.6.9.0. Native claude.exe reports 2.1.319.
Registry: HKLM "Git_is1" and "PowerShell 7-x64" (machine scope), HKCU "Claude Code" (user scope).
winget search ids: OpenAI.Codex 0.166.0, SST.opencode 1.18.5, Anomaly.OpenCode 2.2.6, GitHub.Copilot,
charmbracelet.crush, MoonshotAI.KimiCLI 1.52.0, MoonshotAI.KimiCodeCLI 2.5.2, Sourcegraph.Amp,
FactoryAI.Factory 1.6.0. Pressly.Goose is a database tool.

## Timing (ms, two runs, warm)
uv tool dir 47/63; uv tool list 54/59; npm prefix -g 269/241; npm ls -g --depth=0 --json 813/869
(cold 1415); winget list --id X --exact 563/525; claude --version 78/51; codex --version 209/191.

## Self-update commands (help output)
claude update: "package-manager installs update through the package manager"; codex update;
aider --upgrade (from PyPI); pi update [self|--all]; opencode upgrade --method curl|npm|pnpm|bun|brew|choco|scoop.
