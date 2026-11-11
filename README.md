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

AI coding agents are fast, but running several of them means a pile of terminal windows, questions
you notice too late and agents that overwrite each other's work. **Hangar is the control room for
that.** It runs Claude Code, Codex CLI, Gemini CLI, Aider and any other command-line tool in one
native desktop app, shows you at a glance which agent is working and which one needs you, and keeps
them out of each other's way.

Hangar brings no model and makes no API calls of its own: it runs the tools you already use, with
your own accounts, on your own machine. Open source under the MIT license.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-light.png">
  <img alt="Hangar with Claude Code and its sub-agents next to Codex CLI in its own worktree; the sidebar shows Aider waiting for an answer and two teammates online" src="docs/images/hangar-dark.png">
</picture>

## Features

- **All your agents in one window.** Claude Code, Codex CLI, Gemini CLI, Aider and 18 more are
  found automatically, and every session keeps running in the background.
- **Never miss a question.** Hangar shows when an agent waits for you, with an optional sound and
  notification. Each state gets its own color and pulse.
- **Side by side.** Two or four sessions at once, arranged by dragging.
- **Watch sub-agents work.** The sub-agents of a Claude Code session appear under it in the
  sidebar; one click shows a sub-agent's task, commands, paths and results beside the terminal.
- **Agents that don't collide.** Each session can get its own worktree, and Hangar warns when two
  sessions touch the same file.
- **Notes, shared if you like.** Notes next to the terminal, private on your computer or shared
  encrypted in the repository, so they travel with your commits.
- **Teams.** The Team window shows what your teammates' agents work on and who holds which task,
  end-to-end encrypted.
- **Your look.** Six terminal color schemes in light and dark, and four bundled coding fonts.
- **Small things that add up.** Paste screenshots straight into an agent, one-click updates for
  Hangar and your agents, sessions that come back after a restart, and a single Hangar window that
  comes to the front when you start it again.

## Screenshots

<table>
  <tr>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-subagent-light.png">
        <img alt="A Claude Code session with the activity log of its sub-agent 'Write retry tests' beside the terminal: the task, file reads, a failing test run and the fix" src="docs/images/hangar-subagent-dark.png">
      </picture>
      <br><sub>Follow a sub-agent's task, tool calls and results beside its session.</sub>
    </td>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-team-light.png">
        <img alt="The Team window over the terminals, listing two teammates with their sessions and states, and the reserved tasks of the project" src="docs/images/hangar-team-dark.png">
      </picture>
      <br><sub>See who on your team works on what, and reserve tasks.</sub>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-notes-light.png">
        <img alt="The notes panel beside a terminal, with encrypted notes shared in the repository above notes kept only on this computer" src="docs/images/hangar-notes-dark.png">
      </picture>
      <br><sub>Keep notes private or share them encrypted with the repository.</sub>
    </td>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-settings-light.png">
        <img alt="Settings, Appearance: a live terminal preview, six color schemes shown in their own colors and the bundled terminal fonts" src="docs/images/hangar-settings-dark.png">
      </picture>
      <br><sub>Pick a color scheme and font, previewed live.</sub>
    </td>
  </tr>
</table>

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
