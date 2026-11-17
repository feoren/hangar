---
title: "ADR 0012: Messages between sessions of partner projects"
type: decision
status: accepted
superseded_by:
tags: [sessions, projects, agents, hooks, security, pty]
sources: ["[[cross-project-paste-measurement]]"]
created: 2026-11-17
updated: 2026-11-17
---

# ADR 0012: Messages between sessions of partner projects

## Context
The owner works on several projects that belong together but live in different folders, for
example a desktop app, its product website and a relay server. Each one runs its own agent session
in Hangar. Work in one project often needs a change in another: the app starts to show a server
text, so the website has to send it. Today the person copies an instruction from one session,
switches to the other one and pastes it.

Claude Code can already message another Claude Code session on the same machine. That only
works between Claude Code sessions, and Hangar does not see it. Hangar hosts every session itself
and knows its program, working folder and state. It can therefore offer the same thing for every
agent CLI.

What already exists, from the code:

- **Typing into a running session.** Agent commands and "Send to several" send text as bracketed
  paste and then Enter as a separate write (`src-tauri/src/agent_commands.rs`). The handover
  (`src/handover.ts`) pastes a prompt without Enter and first waits until the agent has turned
  bracketed paste on and its output has paused.
- **A local endpoint that agents can reach.** The agent gate runs an HTTP endpoint on 127.0.0.1
  (`src-tauri/src/gate_service.rs`, `src-tauri/src/hook_http.rs`). Every session that has the gate
  gets its own random token, only through the PTY child's environment (`HANGAR_HOOK_TOKEN`). A
  request without a known token is refused.
- **Session states.** A session row knows whether its program is working, paused or waiting for
  the person (glossary: attention, waiting (for you)).
- **Project folders.** The sidebar groups sessions by project folder (glossary: project group),
  and the project overview has one card per project.

Requirements from the owner:

1. From a session in project A, send an instruction to the session of project B.
2. It works with every agent CLI, not just Claude Code.
3. Projects in different folders can be linked, so that this works without searching.

## Options

| Option | Advantage | Disadvantage |
| --- | --- | --- |
| A. Paste into the target terminal | Works with every CLI, the person sees what arrives | Hangar has to wait until the target agent is ready for input |
| B. Hangar MCP server with `send` and `inbox` tools | Clean tool for agents, answers possible | Not every CLI supports MCP in the same way; the target agent only reads its inbox when it decides to |
| C. Mailbox file in the project folder | Very simple | The target agent does not notice new mail; files land in the repository |
| D. Rely on Claude Code's own session messages | Nothing to build | Claude Code only, invisible to Hangar, no link between projects |

## Decision
Accepted on 2026-11-17: option A, with a command line client as the sending side for agents. Option B can
follow later for answers.

**1. Partner projects.** A project can name other projects as partners, each with a short name and
a folder. A partner pair is set up once by the person, in a project's settings or on its project
card. Messages only go between partners, never to any session. The list is stored like other
per-project settings in the app data folder, not in the repository, so no path reaches a public
repository.

**2. Two ways to send.**

- **By hand:** In a session's menu, the person chooses "Send to partner", picks the partner and
  writes the text.
- **By the agent:** Hangar puts a small client on the session's `PATH`, called as
  `hangar-send <partner> <text>` or with the text on stdin. It reads the session's token from its
  environment and posts to a new route of the existing endpoint on 127.0.0.1. The route checks the
  token, resolves which session it belongs to and checks that `<partner>` is a partner of that
  session's project. Arguments go as an argv array, never through a shell string.

**3. Delivery.**

- If the partner has exactly one running agent session, the message goes there. With several, the
  person chooses. With none, Hangar offers to start one with the partner's last profile in its
  folder.
- The text is pasted as bracketed paste with a header line naming the sending project, for
  example `[Message from project "app"]`. Like the handover, Hangar waits until bracketed paste is
  on and the output has paused. A target session that is working keeps the message queued until it
  pauses.

**4. The person decides.** A message sent by an agent is never pasted silently. The target session
row shows it as a waiting message with the sender and the text, and the buttons "Paste" and
"Discard". Only the person's "Paste" types it into the terminal, and Hangar presses Enter only if
the person chooses "Paste and send". Without this step, one project's agent could steer another
project's agent without the person seeing it. A message sent by hand is pasted right away,
because the person wrote it.

**5. Limits.**

- A message is plain text, capped like the handover prompt.
- Control characters except newline and tab are removed before the paste, so a message cannot
  end bracketed paste early or send keys.
- At most a few queued messages per target session; more are refused with a clear error for the
  sender.
- Nothing is stored after it is pasted or discarded, apart from a line in the work log
  ([[0011-work-log-and-daily-review]]) with sender, target and time, not the text.

## Consequences

- **New domain terms:** partner project and cross-project message (glossary).
- **Endpoint without the gate.** The hook endpoint today only serves sessions with the gate, that
  is team sessions ([[0008-private-and-team-sessions]]). Sending must also work from private
  sessions. So the endpoint must also start for sessions that only have the send route, and the
  token must say which routes it allows.
- **A client on `PATH`.** It needs its own small binary, or a mode of the Hangar binary, for
  Windows, macOS and Linux. On macOS the login shell's `PATH` rules apply as for every child
  process.
- **Answers.** Option A has no reply channel. The target agent can answer with the same client,
  because the sender's project is its partner too. Whether that is enough or whether option B's
  inbox is needed is open.
- **Not a remote feature.** Messages stay on one machine and one Hangar instance; sessions of
  teammates are out of scope (non-goal: remote multiplexing).

## Evidence
- Code paths named in Context, read on 2026-11-17 at commit `d7e66f1`.
- [[cross-project-paste-measurement]] (Windows, Claude Code 2.1.326, Codex CLI 0.169.0, Pi 1.5.2,
  Aider 0.86.2): a bracketed paste with a header line arrives as one prompt, the line break inside
  it does not submit, Enter as a separate write does. Claude Code and Codex turn bracketed paste on
  while their folder trust dialog is open, so readiness signals cannot tell that dialog from the
  prompt: Hangar presses Enter only after the person chose "Paste and send" (decision point 4).
- Same measurement: Codex `workspace-write` on Windows reaches 127.0.0.1 without network access
  turned on; Claude Code under default permissions asks before the call. `hangar-send` is therefore
  an executable with its own name, which no shell alias can shadow.

> [!question] Unverified
> OpenCode, Gemini CLI and the remaining catalog CLIs (not installed or broken on the measuring
> machine); the sandboxes of Codex and Claude Code on Linux and macOS; whether Pi and Aider run
> commands without a sandbox of their own.
