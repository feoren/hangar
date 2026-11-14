---
title: Agent commands and compacting from the context meter
type: synthesis
tags: [agents, slash-commands, session-menu, quick-access, context-meter, profiles]
sources: ["[[agent-commands-measurement]]"]
created: 2026-11-14
updated: 2026-11-14
---

# Agent commands and compacting from the context meter

Hangar can type a program's common slash commands into a running session: from the session menu
("Commands …"), from quick access, and, for the compact command, from a button in the session
row's actions. The commands are data, not code.

## Where the commands come from

- `agent_commands` per `[[agent]]` in `src-tauri/src/agent_catalog.toml`, only commands found in the
  installed program or its docs ([[agent-commands-measurement]]). The key is not `commands`: the
  catalog already uses that for the program names.
- `agent_commands` per `[[profile]]` in `profiles.toml`. A profile with a catalog id that starts
  the catalog's program inherits the catalog's commands and adds its own after them; an own
  command with the same text replaces the catalog's in place (relabel it, drop its confirmation).
  A profile for another program has only its own. Same inheritance rule as `capabilities`
  ([[catalog]]).
- One entry: `{ label_key = "compact", text = "/compact", submit = true, confirm = false }`.
  Exactly one of `label_key` (translated by Hangar: `compact`, `clear`, `new`, `model`, `usage`,
  `context`, `status`, `stats`) or `label` (own text, 1 to 40 characters). `text`: one line, 1 to
  200 characters, no leading space (it would turn `/clear` into a plain prompt), each text once
  per program. `submit` (default true): Enter follows. `confirm` (default false): a dialog asks
  first; only for the label keys `clear` and `new` it also says that the context starts empty.
- Checked by `agent_commands::check` when loading (`src-tauri/src/agent_commands.rs`). A broken
  entry fails the file like any other field: the catalog test fails, `profiles.toml` shows the
  reload notice with the profile and the reason, and the previous profiles stay in use.
- The frontend gets the merged list as `agentCommands` on each `Profile` (`list_profiles`); no
  new command or capability.

## Built-in lists

| Program | Commands (label key) | Evidence |
| ------- | -------------------- | -------- |
| Claude Code 2.1.325 | `/compact` (compact), `/context` (context), `/usage` (usage), `/model` (model), `/clear` (clear, confirm) | command table in the binary |
| Codex CLI 0.169.0 | `/compact` (compact), `/status` (status), `/model` (model), `/new` (new, confirm) | name table and help text in the binary |
| Gemini CLI | `/compress` (compact), `/stats` (stats), `/model` (model), `/clear` (clear, confirm) | vendor command reference only |

`/clear` and `/new` start over with an empty context, so they ask first. Claude Code keeps the
previous session on disk and Codex in its history ([[agent-commands-measurement]]); the dialog
does not promise that, because Gemini's reference does not say so.

## Sending

Exactly like "Send to several" (`src/broadcast.ts` `sendToTargets`): the text as a bracketed
paste, and after the paste reached the backend and 80 ms passed, Enter as a separate write, so a
TUI does not take the Enter as part of the paste. `submit = false` sends the paste only. The
session that got the command becomes the active one, so its answer is in view.

## Where they show (only what works)

- Session menu: "Commands …" only for a running session (not starting, ended or an update
  session) whose program has commands; otherwise the entry is left out. It opens a second menu
  at the same place: the session's name as a caption, one entry per command with its text.
- Quick access: one entry per command of the active session in "Commands", label plus
  "text · session", found by label, text, program name and the word "command".
- Compact button: in the row's actions strip (`.session-actions`, shown on hover and keyboard
  focus), not in the facts line, so it takes no column of the row. Shown when the context meter
  reaches its warning share (`HIGH_SHARE`, 80 %), the program has a `compact` command, and the
  agent is not busy (`commandBusy`: working, or waiting after a bell or for a gate decision). A
  second click on the same row within 3 s sends nothing (`compactRepeated`), since the button
  only hides once the agent's answer marks it as working. Icon in the meter's warning color, accessible name "Kontext
  komprimieren", tooltip with the text.

## Decision: hidden while the agent is busy

The button is hidden, not disabled, while the agent is busy. Busy covers a bell and a pending gate
decision as well as a running turn: both usually mean a question on screen, and the Enter after
the paste would answer it (for example approve a tool) instead of sending the command. Reasons: the project offers only what
works right now; a disabled button cannot take keyboard focus, so its reason in a tooltip would
not reach keyboard users; and the strip shows only on hover, where a greyed button would add noise
for a few seconds at a time. The click handler checks the state once more and sends nothing when
the agent started in between. Menu and quick access send while the agent merely works, since the
person picked the command deliberately; after a bell or during a gate decision
(`awaitsAnswer`) they offer no commands until the person has looked at the session, and an open
commands submenu or question closes. Mid-turn behavior, from binary strings only (no session run,
[[agent-commands-measurement]]): Claude Code 2.1.325 queues slash commands typed while it works;
Codex CLI 0.169.0 has the text "'/…' is disabled while a task is in progress." and refuses such
commands during a turn.

## Decision: no command into a question on screen

Permission dialogs of Claude Code and Codex ring no bell in Hangar's terminal
([[agent-prompt-shapes]]), so `awaitsAnswer` alone misses them. Commands therefore use the queued
message's screen check as well (`asksQuestion` in `agentCommands.ts`, built on `looksLikeQuestion`
in `promptGuard.ts`): the session menu leaves out "Commands" and quick access lists no commands
while the newest screen lines show a question or a choice list, and every send checks again
(`withholdCommand` in `App.tsx`). The row's compact button cannot watch the screen, since the
screen is read only where a command is offered or sent, never on every render; a click on it into
a question sends nothing and a notice says to answer the question first. A false match only
withholds a command, which is the safe direction. An Enter sent into such a prompt is not
measured.
