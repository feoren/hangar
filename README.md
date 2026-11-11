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

### Sessions and agents

- **Every agent in one window.** Claude Code, Codex CLI, Gemini CLI, Aider and 18 more command-line
  tools are detected automatically and start with one click. PowerShell, Command Prompt, WSL and any
  other command work the same way.
- **Sessions that keep running.** Every session runs in the background while you work in another
  one, and comes back with its history after Hangar restarts.
- **Side by side.** Show two or four sessions at once and arrange them by dragging.
- **Quick access.** A command palette finds sessions, programs and actions by name; every action has
  a keyboard shortcut that never collides with the shells and agents themselves.
- **Profiles as files.** Programs, arguments, working folders and environment variables are plain
  configuration. Secret variables are stored in the operating system's credential store, never in
  a file.

### Staying in control

- **Never miss a question.** The sidebar shows at a glance which agent is working, which one is done
  and which one waits for you, with an optional sound, a desktop notification and a badge on the
  taskbar icon.
- **Your colors for every state.** Working, done, waiting and paused each get their own color, pulse
  strength and pulse duration, previewed live in the settings.
- **Watch sub-agents work.** The sub-agents of a Claude Code session appear as a tree below it. One
  click shows a sub-agent's task, text, tool calls and results beside the terminal as they happen;
  finished ones can be cleared away. Recognizable keys and passwords are masked.
- **Changes at a glance.** A side panel lists the files a session changed, with the changed lines.

### Parallel work without collisions

- **A worktree per session.** Start an agent in its own Git worktree with one click, so several
  agents can change the same project at once. Worktree sessions get their own group in the sidebar,
  and when such a session ends, Hangar removes its worktree again unless it still holds changes.
- **Overlap warnings.** Hangar notices when two sessions change the same file or even the same
  lines, and can ask you before an agent edits a file another session is working on.

### Notes

- **Notes beside the terminal.** Write notes per project or in general, capture a quick note with one
  shortcut, or send selected terminal text straight into a note.
- **Shared, encrypted notes.** Turn on shared notes for a project and they live encrypted in the
  repository, so they travel with your commits. A recovery code makes sure they can always be
  opened again.

### Teams

- **The Team window.** See which teammates are online, which agents they run and which files they
  change, and reserve tasks so nobody works on the same thing twice. Devices join only after
  confirmation, and everything shared is end-to-end encrypted.

### Terminal and comfort

- **A complete terminal.** Truecolor, hyperlinks, bracketed paste, search in the history and
  export of a session.
- **Paste screenshots.** Copy a screenshot and press Ctrl+V: Hangar saves the image and pastes its
  path, so the agent can attach it.
- **Snippets and broadcast.** Reusable text snippets, and one message sent to several sessions at
  once.
- **Your look.** Six terminal color schemes, each in light and dark, and four bundled coding fonts,
  every one shown in its own colors and typeface. The app follows the system theme or your choice.
- **Twelve languages.** The interface speaks English, German, Spanish, French, Italian, Portuguese,
  Polish, Russian, Turkish, Greek, Japanese and Chinese.
- **Always up to date.** Hangar updates itself with signed updates and offers updates for your agents
  as well. Starting it a second time brings the running window to the front.

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
