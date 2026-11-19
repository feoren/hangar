---
title: "ADR 0014: Linked sessions that work together"
type: decision
status: accepted
superseded_by:
tags: [sessions, agents, hooks, security, pty, sidebar]
sources: ["[[0012-cross-project-messages]]", "[[cross-project-paste-measurement]]"]
created: 2026-11-19
updated: 2026-11-19
---

# ADR 0014: Linked sessions that work together

## Context
[[0012-cross-project-messages]] lets the agent of one project message the agent of a partner
project. A partnership is set up once per pair of projects, is stored, and an agent's message
waits until the person clicks "Paste".

The owner wants something lighter: pick two or more running sessions in the sidebar, link them,
and let their agents talk to each other on their own until the person unlinks them. The linked
sessions must be easy to spot, more than two must be possible, and it must work with every agent
CLI.

## Options

| Option | Advantage | Disadvantage |
| --- | --- | --- |
| A. Link groups of running sessions, in memory, automatic delivery | What the owner asked for; reuses the 0012 send route and paste | Agents steer each other without a click; needs a loop brake |
| B. Extend partner projects to more than two and make them temporary | One concept less | Partners are per project, the owner wants per session; mixes a stored and a temporary concept |
| C. Shared chat panel in Hangar that agents read via MCP | Answers and history in one place | Not every CLI supports MCP the same way; the agent only reads when it decides to |

## Decision
Accepted on 2026-11-19: option A.

**1. Link group.** The person selects two or more running agent sessions and chooses "Link". They
form a link group with its own color from a fixed palette, shown as a colored chain mark on each
session row (and on the tab icon in the collapsed sidebar). A session belongs to at most one group.
Several groups can exist at once. Shell sessions cannot be linked (as in 0012, shells neither send
nor receive).

**2. Names.** Each member gets a short name inside the group, derived from its session title
(lowercase, `[a-z0-9-]`, unique inside the group by a numeric suffix). The person can rename it in
the link dialog. `hangar-send <name> <text>` writes to one member, `hangar-send all <text>` to
every other member.

**3. Lifetime.** A group lives only in memory. It ends when the person chooses "Unlink" or when
fewer than two members remain (a member's session closed or exited). Nothing is stored on disk;
after a restart there are no groups. A member that leaves loses the right to send to the group at
once; its queued messages to others stay deliverable, messages queued for it are dropped.

**4. Sending.** The send route of 0012 resolves the target in this order: member of the sender's
link group, then partner project. Because a link is made while the session runs, its environment
cannot change any more: every agent session therefore gets the send token, the `hangar-send`
client on `PATH` and the `HANGAR_SEND` variable at its start, whether or not it has partners. The
route refuses a sender that is neither linked nor has the named partner, so an unused token grants
nothing. Sessions started before this version have no token and are not offered for linking.

**5. Introduction.** On "Link", Hangar delivers an introduction to every member (same delivery path
as below): the group's member names, each with its program and working folder, and how to use
`hangar-send`. The text says the other members are agents working in parallel and that the agent
should send results or requests only when they help the other side.

**6. Automatic delivery.** Inside a link group, a message is delivered without a click: Hangar
queues it for the target session and pastes it with the header line
`[Message from "<name>" (link group)]` and then Enter once the target is waiting for input (state
"waiting for you" or "paused", output paused, bracketed paste on). This differs from 0012
decision point 4 on purpose: the person created the group and so consented to agents steering each
other inside it. Messages from partner projects keep the 0012 rule.

Guards:

- A toggle per group, "Ask before pasting", switches the group to the 0012 behavior (waiting
  message with "Paste" and "Discard"). Off by default.
- Loop brake: at most 6 automatic deliveries per group per minute. Beyond that, messages wait for
  a click like in 0012 and the group shows a hint; the brake resets after a quiet minute.
- The cleaning, length cap and queue limits of 0012 apply (at most 5 waiting per target).
- Every delivered message leaves a work log line ([[0011-work-log-and-daily-review]]) with sender,
  target and time, not the text.

**7. Agents.** Delivery is a bracketed paste plus Enter, which needs nothing from the CLI. Measured
in [[cross-project-paste-measurement]] for Claude Code, Codex CLI, Pi and Aider. Sending needs the
agent to run a command; Gemini CLI and OpenCode are not measured yet.

## Consequences
- New glossary terms: link group, member name.
- Every agent session gets a send token. The route still refuses everything without a link group
  or partner, so the attack surface for a session without either is unchanged in effect.
- A link group is not a remote feature and stays inside one Hangar instance.

## Evidence
- [[cross-project-paste-measurement]]: a bracketed paste with a header line arrives as one prompt,
  Enter as a separate write submits it (Claude Code, Codex CLI, Pi, Aider on Windows).
- The send route, token scopes and queue limits as built in [[0012-cross-project-messages]].

> [!question] Unverified
> Automatic Enter while an agent shows a permission or trust prompt: the target state must be
> "waiting for you" at the prompt, not at a dialog. The readiness signals of 0012 cannot always tell
> these apart; the loop brake and the per-group toggle are the fallback.
