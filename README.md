<p align="center">
  <img src="docs/images/logo.svg" alt="Hangar logo" width="96" height="96">
</p>

<h1 align="center">Hangar</h1>

<p align="center"><strong>Run AI coding agents as a team.</strong></p>

<p align="center">
  <a href="https://hangar.feoren.com">Website</a> ·
  <a href="https://github.com/feoren/hangar/releases/latest">Download</a> ·
  <a href="#getting-started">Getting started</a> ·
  <a href="#teams-and-privacy">Teams and privacy</a> ·
  <a href="CONTRIBUTING.md">Contributing</a>
</p>

<p align="center">
  <a href="https://github.com/feoren/hangar/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/feoren/hangar/actions/workflows/ci.yml/badge.svg"></a>
  <a href="https://github.com/feoren/hangar/releases/latest"><img alt="Latest release" src="https://img.shields.io/github/v/release/feoren/hangar"></a>
  <a href="LICENSE"><img alt="License: MIT" src="https://img.shields.io/badge/license-MIT-blue"></a>
</p>

AI coding agents are fast. Put a whole team of people behind them and the hard part is no longer
writing code, it is knowing who changes what: two agents rewrite the same file, a task gets done
twice, a question waits unseen in some terminal. **Hangar is the shared control room for that.**

Every teammate runs Claude Code, Codex CLI, Gemini CLI, Aider or any other command-line tool in
Hangar on their own machine. Hangar shows the whole team, live, which agent works on which task and
which files it changes, lets people reserve tasks, and warns before two agents collide. Everything
shared with the team is end-to-end encrypted, and Hangar works just as well for a single person.

Hangar brings no model and makes no API calls of its own: it runs the tools you already use, with
your own accounts, on your own machine. The desktop app is open source under the MIT license.

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-light.png">
  <img alt="Hangar with Claude Code, Codex CLI in its own worktree and Gemini CLI in resized split panes; the sidebar shows each session's state, Claude Code's sub-agents and context meter, Aider asking a question and two teammates online" src="docs/images/hangar-dark.png">
</picture>

## Features

### Sessions and agents

- **Every agent in one window.** Claude Code, Codex CLI, Gemini CLI, Aider and 18 more command-line
  tools are detected automatically and start with one click, each with its own icon in one
  consistent style. PowerShell, Command Prompt, WSL and any other command work the same way.
- **Sessions that keep running.** Every session runs in the background while you work in another
  one, and comes back with its history after Hangar restarts. The house button in the sidebar puts
  them aside and shows a calm home view with a calendar of your commits over the last year.
- **Sessions you recognize.** The sidebar groups sessions by program or by project folder, and an
  agent session takes its name from your first prompt, so several sessions in one project stay
  apart. Each row shows its state, folder and branch, and its CPU, memory and GPU use. Hovering a
  row shows that session's last lines in their colors without switching to it.
- **A terminal that feels like an editor.** A click in the prompt line moves the cursor there, and
  Shift+Enter starts a new line in an agent's prompt instead of sending it. Web addresses and
  existing files and folders in the output open with one click; with Shift held, each folder of a
  path opens on its own.
- **One instruction file for all agents.** Write your rules once in HANGAR.md and Hangar hands them
  to every agent that accepts extra instructions (Claude Code, Codex CLI, Aider), without touching
  your repository. An agent started in a fresh worktree first checks for new upstream commits and
  asks before pulling them.
- **Side by side.** Show two, three or four sessions at once, arrange them by dragging and resize
  them by dragging the lines between them.
- **Quick access.** Ctrl+Shift+P finds sessions, programs, files, recent commits, settings and every
  menu command; Ctrl+Shift+S searches the output of all sessions and Ctrl+Shift+M shows them all as
  live tiles. No shortcut collides with the shells and agents themselves.
- **Workspace templates.** Save the running sessions as a template and start the whole set again
  with one click.
- **Profiles as files.** Programs, arguments, working folders and environment variables are plain
  configuration. Secret variables are stored in the operating system's credential store, never in
  a file.

### Staying in control

- **Never miss a question.** The sidebar shows at a glance which agent is working, which one is done
  and which one asks you something, with a colored edge and a small icon on every row. An optional
  sound, a desktop notification, a push to your phone (ntfy or Pushover) and a badge on the taskbar
  icon add to it, all of them quiet during the hours you choose.
- **What changed when it is done.** When an agent finishes, a short notice names how many files it
  changed and opens them in one click. A local dev server an agent starts shows as a globe on its
  row that opens it in the browser.
- **Load at a glance.** A one-row bar shows the machine's CPU, memory and GPU load as small live
  graphs with the memory in use, and every session row shows its own CPU, memory and, when it uses
  the GPU, GPU load and memory (GPU numbers on Windows); a mark flags a session that keeps a whole
  core busy or holds a lot of memory.
- **Context at a glance.** Claude Code sessions show how full their context window is right in
  their row, for example `12 % · 123k / 1M`.
- **Your colors for every state.** Working, done, waiting and paused each get their own color, pulse
  strength and pulse duration, previewed live in the settings.
- **Watch sub-agents work.** The sub-agents of a Claude Code session appear as a tree below it, each
  with what it does right now, its run time and its tokens. One click shows a sub-agent beside the
  terminal the way Claude Code itself does, with recognizable keys and passwords masked; drag the
  line between them to give either side more room.

### Changes and files

- **Review and commit in place.** The Changes tab lists the files a session changed, collapsed for an
  overview, and the recent commits. Stage, unstage or discard a file or everything at once, write a
  commit or ask the agent for a message, and click a diff line to send the agent a comment about it.
- **The project as a tree.** The Files tab shows the session's folder like an editor's explorer:
  filter it with ranked, highlighted matches, insert or drag a path into a terminal in the form its
  program reads (an @ mention for agents, a quoted path for PowerShell, cmd, Git Bash and WSL), see
  which files a teammate or another of your sessions is changing, and switch on an activity map of
  where the agents worked.
- **A file viewer that keeps up.** Files open in their own window with syntax colors, a Markdown
  preview, images, a comparison with the last commit, and a live refresh while an agent edits them.

### Parallel work without collisions

- **Private or team sessions.** A private session runs right in your folder. A team session gets its
  own Git worktree, is visible to your team, and Hangar's agent gate asks before the agent edits a
  file another session is working on. Claude Code, Codex CLI, OpenCode and Pi can run as team
  sessions; the gate was measured live for each of them.
- **Remembered answers.** Tell the gate "always" or "never" for a file or folder of a project, and it
  answers for you from then on, with a visible log.
- **Overlap warnings.** Hangar notices when two sessions change the same file or even the same
  lines, also across worktrees of the same repository.

### Notes

- **Notes beside the terminal.** Write notes per project or in general, capture a quick note with one
  shortcut, or send selected terminal text straight into a note. A Private | Shared switch keeps your
  own notes apart from the team's.
- **Shared, encrypted notes.** Turn on shared notes for a project and they live encrypted in the
  repository, so they travel with your commits. A recovery code makes sure they can always be
  opened again.
- **A panel you can detach.** Notes, Changes and Files share a side panel that can live in its own
  window on a second screen.

### Teams

- **The Team window.** See which teammates are online, which agents they run and which files they
  change, and reserve tasks so nobody works on the same thing twice. Devices join only after
  confirmation, and everything shared is end-to-end encrypted. Your profile picture from your
  Hangar account shows in the sidebar.
- **Teammates beside your work.** The file tree shows which files their agents are changing, with
  their avatar and since when.

### Terminal and comfort

- **A complete terminal.** Truecolor, hyperlinks, bracketed paste, search in the history and
  export of a session.
- **Paste screenshots.** Copy a screenshot and press Ctrl+V: Hangar saves the image and pastes its
  path, so the agent can attach it.
- **Snippets and broadcast.** Reusable text snippets with variables such as `{{file}}`,
  `{{branch}}`, `{{selection}}` and `{{diff}}`, saved from selected terminal text with a right click,
  and one message sent to several sessions at once.
- **Your look.** Fourteen terminal color schemes, each in light and dark and readable in both, and
  seven bundled coding fonts, every one shown in its own colors and typeface. Switch between light
  and dark with one click.
- **Twelve languages.** The interface speaks English, German, Spanish, French, Italian, Portuguese,
  Polish, Russian, Turkish, Greek, Japanese and Chinese.
- **Always up to date.** Hangar updates itself with signed updates. It looks for new versions of
  your agents every 30 minutes and whenever you return to the window, and offers the update right
  away; programs from the Microsoft Store are left to the Store.
- **One window.** Starting Hangar a second time brings the running window to the front instead of
  opening another one.

## Screenshots

<table>
  <tr>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-subagent-light.png">
        <img alt="A Claude Code session with its sub-agent 'tester' shown beside the terminal as in Claude Code: the task, its text, tool calls with summed-up results, a failing test run and the fix, and a footer with tool uses, tokens and run time" src="docs/images/hangar-subagent-dark.png">
      </picture>
      <br><sub>Follow a sub-agent beside its session, the way Claude Code shows it.</sub>
    </td>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-files-light.png">
        <img alt="The Files tab beside a terminal: the project folder as a tree with Git letters on changed files, dots on folders and teammates' avatars on the files their agents edit" src="docs/images/hangar-files-dark.png">
      </picture>
      <br><sub>See the project as a tree, with your changes and your teammates' files.</sub>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-team-light.png">
        <img alt="The Team window over the terminals, listing two teammates with their sessions and states and the reserved tasks of the project; the worktree row in the sidebar shows both teammates' avatars" src="docs/images/hangar-team-dark.png">
      </picture>
      <br><sub>See who on your team works on what, and reserve tasks.</sub>
    </td>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-notes-light.png">
        <img alt="The notes panel beside a terminal, with encrypted notes shared in the repository above notes kept only on this computer" src="docs/images/hangar-notes-dark.png">
      </picture>
      <br><sub>Keep notes private or share them encrypted with the repository.</sub>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-settings-light.png">
        <img alt="Settings, Appearance: a live terminal preview, the color schemes shown in their own colors and the bundled terminal fonts" src="docs/images/hangar-settings-dark.png">
      </picture>
      <br><sub>Pick a color scheme and font, previewed live.</sub>
    </td>
    <td width="50%" valign="top"></td>
  </tr>
</table>

## Getting started

1. **Install.** Download **`Hangar-Setup.exe`** from the
   [latest release](https://github.com/feoren/hangar/releases/latest) and run it. It installs for
   your user only, without administrator rights.
2. **Start an agent.** Pick a project folder under _New session_, choose _Private_ or _Team_ and
   click an agent. Hangar lists every agent it finds on your machine, with its version; under
   _Team_ it lists only the agents that can run as team sessions.
3. **Bring in your team** (optional). Click _Sign in_ at the bottom of the sidebar, confirm the code
   in your browser, then _Connect_ the project. Teammates who join the same project see each other
   in the Team window.

Ctrl+Shift+P opens quick access (sessions, files, commits, settings and every menu command),
Ctrl+Shift+S searches the output of all sessions, Ctrl+Shift+M shows all sessions as live tiles,
Ctrl+Shift+H shows the home view, Ctrl+Shift+N opens a quick note and Ctrl+, the settings.

## Teams and privacy

Your code never leaves Git: Hangar shares what the agents are doing, not what they write.

| Your team sees                                            | Your team never sees                        |
| --------------------------------------------------------- | ------------------------------------------- |
| Your display name                                         | File contents and diffs                     |
| For sessions in folders you share: agent, title and state | Your e-mail address                         |
| The files those sessions change, with line numbers        | Your keys, tokens and environment variables |
| The tasks you reserve                                     | Anything from folders you do not share      |

- **End-to-end encrypted.** Everything shared with the team is encrypted on your device. The team
  server stores and forwards it but cannot read it.
- **Devices are confirmed.** A new device joins only after a teammate (or your own other device)
  compares a safety number and confirms it.
- **You choose what is shared.** Sharing is switched on per project and per folder, and Hangar works
  fully without an account.
- **Your own server.** A self-hosted team server is in preparation and not published yet; Hangar
  already accepts its address in _Settings > Account_.
- **Shared notes stay in the repository**, encrypted per file, so the team server is not involved
  at all.

## Requirements

|                  |                                                                                                                                                   |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| Operating system | Windows 10 version 1809 or later, or Windows 11 (x64). macOS and Linux are planned.                                                               |
| Agents           | Install the agent CLIs you want to use (Claude Code, Codex CLI, Gemini CLI, Aider, ...) with your own accounts. Hangar finds them on your `PATH`. |
| Git              | Needed for team sessions (worktrees), change tracking, commits and shared notes.                                                                  |
| Team features    | A Hangar account, created in the browser when you sign in.                                                                                        |

## FAQ

**Does Hangar need its own API key or send my code to a model?**
No. Hangar has no model of its own. Each agent uses its own account and talks to its own provider,
exactly as it does in any other terminal.

**What does it cost?**
The desktop app is free and open source under the MIT license. For team plans, see the
[website](https://hangar.feoren.com).

**Why does Windows show a SmartScreen warning when I install?**
The installer is not yet signed with a code signing certificate, so Windows does not know it. Choose
_More info_ and _Run anyway_. Updates inside Hangar are always signed and verified.

**Can I use Hangar alone?**
Yes. Every feature except the Team window works without an account.

**Where are my settings and notes?**
In your user profile (`%APPDATA%` and `%LOCALAPPDATA%`), except shared notes, which live encrypted
in the project's `.hangar/notes` folder.

## Build from source

You need [Git](https://git-scm.com), [Rust](https://rustup.rs) (stable),
[Node.js](https://nodejs.org) 22 or later, [pnpm](https://pnpm.io) and the
[Tauri 2 prerequisites](https://tauri.app/start/prerequisites/) for Windows (Microsoft C++ Build
Tools and WebView2, which Windows 11 already includes).

```sh
git clone https://github.com/feoren/hangar.git
cd hangar
pnpm install

# Run in development mode with hot reload
pnpm tauri dev

# Build your own installer
pnpm tauri build --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

The installer lands in `src-tauri/target/release/bundle/nsis/`. A build of your own is not signed
with the project's update key, so it does not receive automatic updates; install official releases
to get them. Development builds can run next to an installed Hangar.

## Contributing

Bug reports, ideas and pull requests are welcome. [CONTRIBUTING.md](CONTRIBUTING.md) explains the
check suite and the guidelines, [`docs/wiki`](docs/wiki/index.md) records how things work and why.
Please report security issues privately as described in [SECURITY.md](SECURITY.md).

## License

The desktop app is licensed under the [MIT License](LICENSE). The bundled agent icons name their
sources, licenses and trademark notes in
[`src/assets/agent-icons/NOTICE.md`](src/assets/agent-icons/NOTICE.md); the bundled fonts carry
their licenses next to them in `src/assets/fonts/`. The color schemes carry Hangar's own names;
the palettes they are adapted from, with sources, licenses and trademark notes, are listed in
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
