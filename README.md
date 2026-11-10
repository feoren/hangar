<p align="center">
  <img src="docs/images/logo.svg" alt="Hangar logo" width="96" height="96">
</p>

<h1 align="center">Hangar</h1>

<p align="center"><strong>One window for all your terminal sessions and AI coding agents.</strong></p>

<p align="center">
  <a href="https://hangar.feoren.com">Website</a> ·
  <a href="https://github.com/feoren/hangar/releases/latest">Download</a> ·
  <a href="CONTRIBUTING.md">Contributing</a>
</p>

<p align="center">
  <a href="https://github.com/feoren/hangar/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/feoren/hangar/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/feoren/hangar/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/feoren/hangar"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-blue"></a>
</p>

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-light.png">
  <img alt="Hangar with Claude Code and Codex CLI side by side, and Aider waiting for an answer in the sidebar" src="docs/images/hangar-dark.png">
</picture>

## Features

- **All your agents in one window.** Claude Code, Codex CLI, Gemini CLI, Aider and 18 more are
  found automatically, and every session keeps running in the background.
- **Never miss a question.** Hangar shows when an agent waits for you, with an optional sound and
  notification.
- **Side by side.** Two or four sessions at once, arranged by dragging.
- **Agents that don't collide.** Each session can get its own copy of the project, and Hangar warns
  when two sessions touch the same file.
- **Notes, updates, restore.** Notes next to the terminal, one-click updates for Hangar and your
  agents, and sessions that come back after a restart.
- **Teams (coming soon).** See what your teammates' agents are working on, end-to-end encrypted.

## Install

Download **`Hangar-Setup.exe`** from the [latest release](https://github.com/feoren/hangar/releases/latest)
and run it; no administrator rights needed. Hangar then updates itself, and every update is signed.

Windows 10 1809+ and 11 (x64) are supported today; macOS and Linux are planned.

## Build from source

Requires [Rust](https://rustup.rs), [Node.js](https://nodejs.org) 22+, [pnpm](https://pnpm.io) and
the [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/).

```sh
pnpm install
pnpm tauri dev
```

The check suite and the guidelines are in [CONTRIBUTING.md](CONTRIBUTING.md), design decisions in
[`docs/wiki`](docs/wiki/index.md). Security issues: see [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE)
