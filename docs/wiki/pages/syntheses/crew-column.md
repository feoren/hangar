---
title: Crew column
type: synthesis
tags: [agents, claude-code, sub-agents, layout, figures]
sources: [[sub-agent-library]], [[agent-workflow-features]], [[claude-hook-agent-id-measurement]]
created: 2026-11-16
updated: 2026-11-22
---

# Crew column

While an agent session has sub-agents running, they stand as whole figures in one line, one below
the other, in a column right of that session's terminal. Each figure says in a speech bubble what
it does right now and stands on a plinth whose color shows its state. The figures do not move.
Data comes from the existing sub-agent watch ([[agent-workflow-features]], item 23) and, for
who waits, from the gate's pending decisions (see "Who waits" below).

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
- **Switch:** "Hide crew" on the column or strip itself; "Show crew" / "Hide crew" in the session
  menu (below the sub-agents entry) and in quick access for a session that has sub-agents; and
  "Show sub-agents next to the terminal" in Settings (`crewColumn`, default on, one setting for
  all three). Off means no column and no watch for it.
- **Split view:** the watch is one per window, so only the active pane shows its crew.
- **Many sub-agents:** at most `MAX_CREW` (6) figures; ranking waiting, working, quiet, done, then
  newer first; the rest is a "+N more" line. Order on screen stays the start order.
- **Click** on a figure opens the sub-agent view for that sub-agent (same as a row in the tree).

## States, colors, texts

The plinth is a low block seen slightly from above: a trapezoid top face in the state color, the
figure standing in its middle, and a front face tinted with the same color mixed into the panel color and shaded towards the
bottom, so its label (text color) keeps 4.5:1 in both themes. The label is the sub-agent's name
(the user's choice): on screen only the color tells the state. The state word stays in the tooltip
and the accessible name. The color changes with a short transition (color only, allowed under
reduced motion).

| State | When | Token | State word (tooltip, label) | Bubble |
| ----- | ---- | ----- | --------------------------- | ------ |
| working | status `running` | `--accent` | Working | tool and target, e.g. "Reading terminal.ts" |
| waiting | the session has a pending gate decision (reason `gate`, never `bell`) whose hook named this sub-agent (`agent_id`); for a decision that names nobody, the fallback below | `--warn` (amber in both themes), stronger tint and a ring | Waiting for you | "I need your OK" |
| done | status `finished`, hidden after 8 s | `--ok` | Done | "Done" |
| quiet | status `stale` (no writes for a minute: cancelled, crashed or waiting long) | `--muted` | Quiet | "Quiet for over a minute" |

No failed state: the transcript does not say whether a sub-agent failed or was cancelled, so
nothing is claimed. `stale` stays honest as "quiet". `bell` does not count: Claude Code also
rings it for the main agent's idle notice ([[agent-prompt-shapes]]).

**Who waits.** Claude Code 2.1.330 sends `agent_id` and `agent_type` in the `PreToolUse` input of
a sub-agent's tool call, over command and http hooks, and leaves both out for the main agent;
`agent_id` is the id of the sub-agent's transcript, which the watch uses
([[claude-hook-agent-id-measurement]]). The gate keeps them only in the expected shape (an id of
1 to 64 letters, digits, `_` or `-`; a type of up to 128 such characters plus `.` and `:`; a type
without an id is dropped, since `--agent` sends it for a main agent too) and never refuses a
request over them (`src-tauri/src/gate.rs`, `sub_agent_ref`). The helper `hangar gate-hook`
passes them on from any agent that sends them. Each pending decision lists the sub-agents of the
requests still waiting on it (`PendingDecision.agents`, at most four, one per request); a request
that stops waiting takes its name along. The column marks every named sub-agent as waiting, so
several can wait at once; a name the watch does not know marks nobody (`src/crew.ts`,
`waitingSubAgents`).

**A decision that names nobody** is the main agent's; nothing is guessed. Claude Code sends
`agent_id` on a sub-agent's tool calls since 2.1.69 ([[subagent-transcripts]]), and the column
exists only for Claude Code (`SubAgentKind` has only `Claude`). An earlier version guessed the
sub-agent that wrote last before the decision, which marked a quiet sub-agent as "Waiting for
you" while the main agent was the one asking.

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
