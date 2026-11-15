---
title: Crew column
type: synthesis
tags: [agents, claude-code, sub-agents, layout, figures]
sources: [[sub-agent-library]], [[agent-workflow-features]]
created: 2026-11-16
updated: 2026-11-16
---

# Crew column

While an agent session has sub-agents running, they stand as whole figures in one line, one below
the other, in a column right of that session's terminal. Each figure says in a speech bubble what
it does right now and stands on a plinth whose color shows its state. The figures do not move.
Data comes from the existing sub-agent watch ([[agent-workflow-features]], item 23); nothing in
the backend changes.

## Layout

- **Column:** a flex child of the pane next to the terminal (never an overlay). The terminal fits to
  the remaining width through its existing resize observer and the PTY gets the new size as usual.
  Width 148 px, not resizable and not remembered (it holds an 88 px high figure, its bubble and plinth).
- **Shown** only when the session supports sub-agents (`SessionInfo.subAgents`), the switch is on
  and at least one sub-agent is running, quiet, or finished less than `DONE_LINGER_MS` (8 s) ago.
  A quiet one older than `QUIET_SHOWN_MS` (10 min) is not shown: it is history, not crew.
  Sub-agents the user dismissed in the sidebar tree stay hidden here too.
- **Fallback, crew strip:** a pane narrower than `CREW_COLUMN_MIN_PANE` (560 px) gets a strip
  between header and terminal instead: one chip per sub-agent with the face, a one-line text and a
  colored bottom edge. Chips wrap, no horizontal scrollbar.
- **Switch:** "Hide crew" on the column or strip itself, and "Show sub-agents next to the terminal"
  in Settings (`crewColumn`, default on). Off means no column and no watch for it.
- **Split view:** the watch is one per window, so only the active pane shows its crew.
- **Many sub-agents:** at most `MAX_CREW` (6) figures; ranking waiting, working, quiet, done, then
  newer first; the rest is a "+N more" line. Order on screen stays the start order.
- **Click** on a figure opens the sub-agent view for that sub-agent (same as a row in the tree).

## States, colors, texts

The plinth is a low round pedestal: an elliptic top face in the state color for the figure's feet
and a front face tinted with the same color mixed into the panel color and shaded towards the
bottom, so its label (text color) keeps 4.5:1 in both themes. The label is the sub-agent's name
(the user's choice): on screen only the color tells the state. The state word stays in the tooltip
and the accessible name. The color changes with a short transition (color only, allowed under
reduced motion).

| State | When | Token | State word (tooltip, label) | Bubble |
| ----- | ---- | ----- | --------------------------- | ------ |
| working | status `running` | `--accent` | Working | tool and target, e.g. "Reading terminal.ts" |
| waiting | the session has a pending gate decision (reason `gate`, never `bell`) and this is the unfinished sub-agent whose last write is before the decision's creation and newest of those; one that wrote since is not waiting | `--warn` (amber in both themes), stronger tint and a ring | Waiting for you | "I need your OK" |
| done | status `finished`, hidden after 8 s | `--ok` | Done | "Done" |
| quiet | status `stale` (no writes for a minute: cancelled, crashed or waiting long) | `--muted` | Quiet | "Quiet for over a minute" |

No failed state: the transcript does not say whether a sub-agent failed or was cancelled, so
nothing is claimed. `stale` stays honest as "quiet". The waiting state is inferred from the
session, not read from the sub-agent; with several candidates the one that wrote last before the
decision is marked. `bell` does not count: Claude Code also rings it for the main agent's idle
notice ([[agent-prompt-shapes]]).

> [!question] Unverified
> Claude Code's hook reference says hooks carry `agent_id` for sub-agent tool calls
> ([[subagent-transcripts]], [[claude-http-hook-measurement]]), which would name the waiting
> sub-agent exactly. The gate does not pass it on today (`PendingDecision` has no such field) and
> the field was never measured, so the guess above stays. Follow-up: the backend reads `agent_id`
> from the gate hook input and sends it with the decision; this needs a backend rebuild.

Bubble text from the newest tool call (`crewText.ts`): known tools get a verb with the target
("Reading terminal.ts", "Searching for trackTyped", "Editing SessionLook.tsx", "Running cargo test"),
paths shortened to the file name, commands and patterns cut at 28 characters on the first line.
Without a target (excerpts switched off) the verb stands alone ("Reading a file"). Unknown tools:
"Using {tool}" with the tool name cut to 24 characters (MCP names to their last segment). No
tool yet: "Starting …".

## Throttle

A bubble changes at most once per `BUBBLE_MIN_MS` (1 s) per figure; the newest text wins when the
second is over. A change of state (waiting, done, quiet) shows at once. The column re-renders on
a 500 ms clock, which also carries the linger of finished figures; the clock only runs while the
session has sub-agents.

## Data flow and privacy

- One shared watch store (`useSubAgents.ts`, `subAgentStore.ts`) serves the sidebar tree, the
  sub-agent view and the column; the backend keeps one watch per session, so no second
  subscription exists. `watchedSession` now also returns the active session while the column is
  possible, which keeps the watch running without the tree.
- The watch runs only when the session supports sub-agents and the column is not switched off.
- Excerpts (tool targets) flow while the column is possible and Settings "Show sub-agent text" is
  on; off means no target and no assistant text is read.
- Used: tool name, target, call note (unknown tools only), status, type. Never used: assistant
  excerpts. Texts live only in memory and are cut to short lengths; the backend already redacts.

## Figures

Whole figure (88 px high, 59 px wide, [[sub-agent-library]] "Whole figure and head") of the sub-agent type (`agentType`) through `runningFigure` with the session's folder, like the
tree: a project definition that overrides a global one gets the neutral figure, but only in
sessions started in that project. Unknown types get a role figure
from the name or the initial. No animation anywhere: the old breathing and bobbing of figure
heads (`figure-breathe`, `figure-bob`, `busy`) is removed in the tree, the pane header and the
session look as well.
