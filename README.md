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
  <a href="https://buymeacoffee.com/feoren"><img alt="Buy me a coffee" src="https://img.shields.io/badge/buy%20me%20a%20coffee-support-ffdd00?logo=buymeacoffee&logoColor=black"></a>
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
  <img alt="Hangar with a Claude Code session whose sub-agents hangar-backend, hangar-frontend and hangar-tester stand on round stages in the crew column, each with a speech bubble saying what it edits or runs, and the notes panel open on the right; the sidebar groups the sessions by project with a teal bracket labeled Group · 3 around the linked ones, the hangar-security session has waited for 12 minutes, and the Recently opened group lists acme-billing, acme-web and acme-shop with their program and age" src="docs/images/hangar-dark.png">
</picture>

## Features

### Sessions and agents

- **Every agent in one window.** Claude Code, Codex CLI, Gemini CLI, Aider and 18 more command-line
  tools are detected automatically and start with one click, each with its own icon in one
  consistent style. PowerShell, Command Prompt, WSL and any other command work the same way.
- **Recently opened.** The sidebar keeps the sessions you started last, with program, folder, title
  and when you used them, in a group of its own above New session. One click starts one again in
  its folder and resumes its conversation, Ctrl+Shift+O reopens the newest, and what is open
  already stays out of the list.
- **Sessions that keep running.** Every session runs in the background while you work in another
  one, and comes back with its history after Hangar restarts. The house button in the sidebar puts
  them aside and shows a calm home view: one card per repository with its sessions, open changes,
  newest commits and dev servers, and a calendar of your commits over the last year with weekday
  names and a legend.
- **Sessions you recognize.** The sidebar groups sessions by program or by project folder, and an
  agent session takes its name from your first prompt, so several sessions in one project stay
  apart. Each row shows its state, folder and branch, and its CPU, memory and GPU use in aligned
  columns. Hovering a row shows that session's last lines in their colors without switching to it.
- **A terminal that feels like an editor.** A click in the prompt line moves the cursor there without
  taking the agent's grey suggestion, Backspace or Delete removes marked text, also across the rows
  of a long, wrapped prompt, and Shift+Enter starts a new line in an agent's prompt instead of
  sending it. Your last prompt stays visible above an agent's terminal; a click jumps to the line
  you sent it from. Web addresses and existing files and folders in the output open with one click;
  with Shift held, each folder of a path opens on its own.
- **Dictation on your own computer.** Hold the right Ctrl key, speak and let go: the text lands in
  the prompt of the active session. Speech is recognized locally, nothing is sent anywhere. Turn it
  on and pick another key (F8, F9, F10 or Ctrl+Shift+Space) under _Settings > Dictation_.
- **One instruction file for all agents.** Write your rules once in HANGAR.md and Hangar hands them
  to every agent that accepts extra instructions (Claude Code, Codex CLI, Aider), without touching
  your repository. An agent started in a fresh worktree first checks for new upstream commits and
  asks before pulling them.
- **Agent commands in one click.** The session menu and quick access send an agent's own commands
  such as `/compact`, `/clear` or `/model`, and commands that start over ask first. Add your own
  commands to a profile.
- **Queue the next message.** Write your next instruction while an agent works; Hangar sends it as
  soon as the agent is done and holds it back while the agent asks you something.
- **Hand over to another agent.** "Hand over to ..." in the session menu starts another installed
  agent in the same folder and inserts the last request, the changed files and your notes.
- **Partner projects.** Link projects that belong together, such as an app and its website, on
  their project card. "Send to partner" in the session menu pastes a message into the partner
  project's agent session, with a header line naming the sender, and waits until that agent pauses.
- **Agents message each other.** Agents in linked projects can send with `hangar-send <partner>
<text>`. Nothing is pasted on its own: the message waits under the partner's session row until you
  choose Send, Paste only or × to discard it. While the partner still works, the card says so and
  you can cancel.
- **Link sessions.** Pick two or more running agents and link them; a colored bracket holds the
  group together in the sidebar, labeled with its size and a crown on the lead, and a click on the
  label edits the group. They message each other with `hangar-send` until you unlink them. A lead
  hands out tasks and collects the results, the group history shows every message, and a template
  can link its sessions as soon as they start. Messages are only sent with Enter when the target
  agent is idle, never into a question or a permission dialog.
- **One line per session.** From the View menu or a right-click on a group, every session sits on
  one close line, so fifteen sessions fit at a glance. Turning it off brings back your layout.
- **Side by side.** Show two, three or four sessions at once, arrange them by dragging and resize
  them by dragging the lines between them.
- **Quick access.** Ctrl+Shift+P finds sessions, programs, projects, files, the text of your notes,
  recent commits, settings and every menu command; Ctrl+Shift+S searches the output of all sessions and Ctrl+Shift+M shows them all as
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
  the GPU, GPU load and memory (per session on Windows and on Linux with AMD or Intel graphics,
  the machine's GPU only on macOS); a mark flags a session that keeps a whole core busy or holds
  a lot of memory.
- **Context at a glance.** Claude Code sessions show how full their context window is right in
  their row, for example `12 % · 123k / 1M`. From 80 % a compact button on the row sends the
  agent's compact command.
- **Data flow on the tree lines.** If you like, light flows along the sidebar's lines to every
  working session, at the pace of its output, with its rate beside it. Choose the style and how
  much it shows under _Settings > Appearance_.
- **How long it waits.** When a session has waited for you longer than the time you set (10 minutes
  by default), its row says for how long and its dot pulses softly.
- **A hint when nothing moves.** When a working agent has shown nothing new for a while, its row
  says so quietly, and the session menu offers its last lines and Ctrl+C. No sound, no alarm.
- **Your colors for every state.** Working, done, waiting and paused each get their own color, pulse
  strength and pulse duration, previewed live in the settings.
- **Costs at a glance.** Tokens and estimated costs per session and per day for Claude Code, in the
  peek, as a "Cost" element in the row, per link group and under _View > Costs_, with an optional
  daily budget that warns you early (at 80 % unless you choose otherwise) and once when it is reached.
- **Yesterday at a glance.** On the first start of a day a quiet notice sums up yesterday: projects,
  time and commits, with three lines per project on request.
- **Keys stay out of messages and commits.** Messages between agents, commits and pastes that seem to
  hold an API key, a token or a private key stop and ask you first; Hangar names only the place and
  the kind, never the value.
- **Back after a crash.** After a crash Hangar tells you how many sessions are back and which had to
  start without their conversation; the scrollback is saved every 10 seconds.
- **Approvals Hangar can see.** Claude Code's hooks tell Hangar when a session waits for your
  approval, also in private sessions, so the row shows it and nothing presses Enter for you.
- **Watch sub-agents work.** The sub-agents of a Claude Code session appear as a tree below it, each
  with what it does right now, its run time and its tokens. One click shows a sub-agent beside the
  terminal the way Claude Code itself does, with recognizable keys and passwords masked; drag the
  line between them to give either side more room, and close finished ones with one click.
- **Your crew beside the terminal.** Running sub-agents stand as figures in a column next to the
  agent's terminal, each on a round stage whose light ring shows its state (working, waiting for
  you, done), and a speech bubble says what it is doing right now, such as "Reading parser.rs";
  until it gets going, dots type in the bubble. The figures move calmly while you can see them, or
  stand still if you choose so. A narrow window shows a strip of faces instead; the session menu
  shows or hides the crew.

### Sub-agents

- **Fifteen sub-agents ready to work.** Hangar ships its own crew for Claude Code and Codex CLI:
  hangar-backend, hangar-frontend, hangar-tester, hangar-security, hangar-developer,
  hangar-manager, hangar-researcher, hangar-sysadmin, hangar-marketing, hangar-sales,
  hangar-social-media, hangar-finance, hangar-contracts, hangar-n8n and hangar-comfyui. Switch
  them off in the settings if you don't want them.
- **Your sub-agents in one place.** _Settings > Sub-agents_ separates Hangar's sub-agents from your
  own Claude Code agents (from `~/.claude/agents` and your projects) and Codex CLI's agent files,
  shown as cards or as a table; one click opens an agent's model and figure. Add a new one by
  pasting it or from a file; Hangar never replaces a file unasked.
- **A figure for every Hangar agent.** Each of Hangar's sub-agents wears a whole figure: one of 41
  bundled ones or your own image, also animated. Your own agents show their initial. A session
  started as a sub-agent wears its figure in the sidebar, and the crew column shows it while the
  sub-agent works.
- **Start as an agent.** Quick access starts a Claude Code session as one of your sub-agents
  (`claude --agent`).
- **Models per agent and per program.** Pick the model of each sub-agent (Opus, Sonnet, Haiku, Fable
  or a model id for Claude Code; a model and reasoning effort for Codex CLI) and a default model for
  all sub-agents of a program, which Hangar sets for every session it starts.
- **The same crew in every agent.** Hangar provides your Claude Code sub-agents to Codex CLI, GitHub
  Copilot CLI, OpenCode 2 and Gemini CLI as well, so they all work with the same team; switch each
  program off in the settings. A file of yours with the same name is kept as a `.bak`, and in
  projects only files Git does not track are replaced.
- **Session reports.** Export a Claude Code session as a Markdown report with its prompts, the
  agent's answers, one line per tool call, sub-agent runs with their tokens, the open changes and
  the commits made meanwhile.
- **A daily review.** See what your sessions worked on each day, per project, with times, commits
  and changed files, built from a local work log that never stores terminal content or prompts.
  Copy it as Markdown or save it as a note.

### Changes and files

- **Go back in time.** The timeline in the Changes tab quietly saves your files after each agent
  step, outside your repository. Click a step to see its changes and go back to any saved state; the
  current state is saved first.
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
  file another session is working on. Claude Code, Codex CLI, OpenCode, Pi, GitHub Copilot CLI,
  Cursor CLI and OpenCode 2 can run as team sessions. Team sessions are part of the Team plan.
- **Remembered answers.** Tell the gate "always" or "never" for a file or folder of a project, and it
  answers for you from then on, with a visible log.
- **A guard for risky commands.** In team sessions the gate also asks before a shell command such
  as a force push, a hard reset, a forced clean or a recursive delete outside the project. The
  rules live in a plain file you can edit. It is a safety net against mistakes, not a sandbox.
- **Worktree cleanup.** One dialog lists the worktrees Hangar created with their state and size and
  removes the safe ones. A worktree with changes, ignored files or unmerged commits stays.
- **Overlap warnings.** Hangar notices when two sessions change the same file or even the same
  lines, also across worktrees of the same repository.
- **The agent arena.** Give one task to two to four agents, each in its own worktree from the same
  commit. Compare changed files, lines, time and the result of your test command, then take over
  the winner's changes into your project folder, uncommitted.

### Notes

- **Notes beside the terminal.** Write notes per project or in general, capture a quick note with one
  shortcut, or send selected terminal text straight into a note. A Private | Shared switch keeps your
  own notes apart from the team's.
- **Shared, encrypted notes.** Turn on shared notes for a project and they live encrypted in the
  repository, so they travel with your commits. A recovery code makes sure they can always be
  opened again.
- **A panel you can detach.** Notes, Changes and Files share a side panel that one switch in the bar
  above the terminal slides open and closed, and that can live in its own window on a second screen.

### Teams

Team features need the Team plan; the free plan is for one person. See
[pricing](https://hangar.feoren.com/pricing).

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
- **Your own session row.** Under Appearance you arrange the parts of every session row yourself:
  drag them into place, pick a form for each and hide what you don't need. Fifteen templates give
  you a head start, previewed live.
- **Easy on the battery.** On battery Hangar polls less often and holds its animations still; a
  small leaf in the system bar shows that saving is on (Windows and Linux).
- **Twelve languages.** The interface speaks English, German, Spanish, French, Italian, Portuguese,
  Polish, Russian, Turkish, Greek, Japanese and Chinese.
- **Always up to date.** Hangar updates itself with signed updates. It looks for new versions of
  your agents every 30 minutes and whenever you return to the window, and offers the update right
  away; programs from the Microsoft Store are left to the Store.
- **Settings your size.** Drag the settings window larger or smaller by its edges and corners; it
  remembers its size, and every section opens at its top.
- **One window.** Starting Hangar a second time brings the running window to the front instead of
  opening another one.

## Screenshots

<table>
  <tr>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-recently-opened-light.png">
        <img alt="The Recently opened group above New session with five entries, acme-billing with Claude Code 5 min. ago, acme-web with Codex CLI 2 hr. ago, acme-shop with Gemini CLI yesterday, acme-infra with PowerShell 3 days ago and acme-mobile with Claude Code 6 days ago; the hovered acme-web row shows its remove button, beside two terminals working side by side" src="docs/images/hangar-recently-opened-dark.png">
      </picture>
      <br><sub>Recently opened: one click brings a session back with its folder and conversation.</sub>
    </td>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-sub-agents-light.png">
        <img alt="The crew column of a Claude Code session reviewing a change: hangar-manager on a green-ringed stage saying Done, hangar-security searching for apiKey|secret, and hangar-researcher with typing dots in its bubble, beside a Codex CLI session" src="docs/images/hangar-sub-agents-dark.png">
      </picture>
      <br><sub>The crew on its stage: the light ring shows each sub-agent's state.</sub>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-home-light.png">
        <img alt="The home view with the activity calendar of the last year in a frame, with month and weekday names, 737 commits and a legend from Less to More, above project cards for acme-shop and acme-docs with their sessions, open changes, team and dev server" src="docs/images/hangar-home-dark.png">
      </picture>
      <br><sub>The home view: a calendar of your commits and one card per project.</sub>
    </td>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-waiting-message-light.png">
        <img alt="Two waiting messages under the Webhook retries session: one from cart with Send, Paste only and a close button, and one from docs set to send, saying it sends once Webhook retries waits, with Cancel" src="docs/images/hangar-waiting-message-dark.png">
      </picture>
      <br><sub>A message from another agent waits for your click: Send, Paste only or discard.</sub>
    </td>
  </tr>
  <tr>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-link-groups-light.png">
        <img alt="The Edit group dialog of a link group with three members, shop, cart and docs, the lead set to the Webhook retries session and the group's cost today of 2.2M tokens, about $15.18, while a teal bracket labeled Group · 3 holds the linked sessions together in the sidebar" src="docs/images/hangar-link-groups-dark.png">
      </picture>
      <br><sub>Link agents into a group with a lead that hands out tasks and collects the results.</sub>
    </td>
    <td width="50%" valign="top">
      <picture>
        <source media="(prefers-color-scheme: light)" srcset="docs/images/hangar-secret-guard-light.png">
        <img alt="The changes panel stopping a commit: it names src/payments/client.ts with a Stripe secret key and .env.production as a file that usually holds secrets, never the value, with Cancel and Commit anyway above the three staged files" src="docs/images/hangar-secret-guard-dark.png">
      </picture>
      <br><sub>The secret guard stops commits and messages that seem to hold a key, and asks first.</sub>
    </td>
  </tr>
</table>

## Getting started

1. **Install.** Get it from the [latest release](https://github.com/feoren/hangar/releases/latest).
   - **Windows:** download **`Hangar-Setup.exe`** and run it. It installs for your user only,
     without administrator rights.
   - **Linux (x64):** download the **`.AppImage`**, make it executable (`chmod +x Hangar_*.AppImage`)
     and start it; it updates itself. On Debian or Ubuntu you can install the **`.deb`** instead
     (`sudo apt install ./Hangar_*.deb`) and update it the same way by hand.
2. **Start an agent.** Pick a project folder under _New session_, choose _Private_ or _Team_ and
   click an agent. Hangar lists every agent it finds on your machine, with its version; under
   _Team_ it lists only the agents that can run as team sessions.
3. **Bring in your team** (optional, Team plan). Click _Sign in_ at the bottom of the sidebar,
   confirm the code in your browser, then _Connect_ the project. Teammates who join the same project
   see each other in the Team window.

Ctrl+Shift+P opens quick access (sessions, files, commits, settings and every menu command),
Ctrl+Shift+S searches the output of all sessions, Ctrl+Shift+M shows all sessions as live tiles,
Ctrl+Shift+H shows the home view, Ctrl+Shift+N opens a quick note and Ctrl+, the settings.

## Teams and privacy

Your code never leaves Git: Hangar shares what the agents are doing, not what they write.

**Teams need the Team plan.** The app is free and open source, and the free plan is for one person,
for private use. Working together as a team needs the Team plan, or a license on your own server.
Teams that already exist keep their members. See
[pricing](https://hangar.feoren.com/pricing).

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
- **Your own server.** Run the open-source [hangar-server](https://github.com/feoren/hangar-server)
  yourself and enter its address in _Settings > Account_; _Test connection_ checks it step by step.
  Without a license it serves one person; teams on your own server need a license from the
  [pricing](https://hangar.feoren.com/pricing) page.
- **Shared notes stay in the repository**, encrypted per file, so the team server is not involved
  at all.

### Anonymous usage statistics (optional)

Hangar asks once whether it may send a small anonymous report per day. It is **off until you say
yes**, and you can switch it off at any time in _Settings > General > Privacy_; switching it off
deletes what was counted so far.

- **What it contains:** the app version, the operating system and language, which agents and
  shells were started and for how long, how many sessions were private or team sessions, a few
  settings (theme, color scheme, font), how often some features were used, and whether you are
  signed in with how many shared projects. Every value comes from a fixed list. The font size and
  the number of shared projects are only sent in steps (font size 10, 13, 16 or 20; projects 0, 1,
  2, 5 or 10), so an unusual value does not make your installation stand out.
- **What it never contains:** no installation ID, no project or person names, paths, folders, file
  names, commands, prompts, file contents or account data.
- **How:** at most one report per day, sent by the app itself to the official Hangar website only,
  never to a self-hosted team server. Development builds never send.

Every field is listed at [hangar.feoren.com/telemetry](https://hangar.feoren.com/telemetry).

## Requirements

|                  |                                                                                                                                                   |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| Operating system | Windows 10 version 1809 or later, or Windows 11 (x64). Linux x64 with glibc 2.34 or later (Ubuntu 22.04, Debian 12 or newer). macOS is planned.   |
| Agents           | Install the agent CLIs you want to use (Claude Code, Codex CLI, Gemini CLI, Aider, ...) with your own accounts. Hangar finds them on your `PATH`. |
| Git              | Needed for team sessions (worktrees), change tracking, commits and shared notes.                                                                  |
| Team features    | A Hangar account with the Team plan. The account is created in the browser when you sign in; the free plan is for one person.                     |

## FAQ

**Does Hangar need its own API key or send my code to a model?**
No. Hangar has no model of its own. Each agent uses its own account and talks to its own provider,
exactly as it does in any other terminal.

**What does it cost?**
The desktop app is free and open source under the MIT license, and so is everything you do alone.
The free plan is for one person, for private use. Teams need the Team plan, and a self-hosted
hangar-server needs a license for teams. Prices are on the
[pricing page](https://hangar.feoren.com/pricing).

**Why does Windows show a SmartScreen warning when I install?**
The installer is not yet signed with a code signing certificate, so Windows does not know it. Choose
_More info_ and _Run anyway_. Updates inside Hangar are always signed and verified.

**Can I use Hangar alone?**
Yes. Every feature except team sessions and the Team window works without an account and without
a plan.

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

## Support

Hangar is free and made in spare time, without investors, ads or tracking. If it saves you
time, you can [buy me a coffee](https://buymeacoffee.com/feoren). Thank you!

## License

The desktop app is licensed under the [MIT License](LICENSE). The bundled agent icons name their
sources, licenses and trademark notes in
[`src/assets/agent-icons/NOTICE.md`](src/assets/agent-icons/NOTICE.md); the bundled fonts carry
their licenses next to them in `src/assets/fonts/`. The color schemes carry Hangar's own names;
the palettes they are adapted from, with sources, licenses and trademark notes, are listed in
[`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
