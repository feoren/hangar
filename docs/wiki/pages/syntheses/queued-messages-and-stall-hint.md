---
title: Queued messages and the stall hint
type: synthesis
tags: [attention, sidebar, frontend]
sources: ["[[agent-prompt-strings-reading]]"]
created: 2026-11-14
updated: 2026-11-15
---

# Queued messages and the stall hint

Two features on top of the attention states (`src/attention.ts`, `src/sessionChip.ts`). Both exist
only for agent sessions (`runsAgent`, no plain shell) while attention is on.

## Queued message

The user writes the next instruction while an agent session works; Hangar sends it once the
session is done. State machine in `src/messageQueue.ts`, wired in `src/App.tsx` (attention
listener), editor in `src/QueueDialog.tsx`.

- One message per session, keyed by the session's stable key (`SessionTerminal.key`, the
  workspace key), so it survives a process restart and an id change of a placeholder.
- How a working phase ends comes from `AttentionTracker.endedBy`: "quiet", "bell" or "exit" are
  known also while the session is watched, where the state only turns "idle" without a reason.
  `phaseEnd` maps it: quiet is "done", a bell or a pending gate decision is "asks", an exit or a
  stop by the user is "other". Ctrl+C or a lone Escape typed while the program works (and "Send
  Ctrl+C" from the menu) calls `AttentionTracker.interrupted`: the quiet that follows then has
  `endedBy` null, so an interrupted turn is "other" and never sends the message.
- Question guard: a permission dialog of Claude Code or Codex usually rings no bell
  ([[agent-prompt-shapes]]), so it ends quietly and counts as "done". Before a message goes out,
  `looksLikeQuestion` (`src/promptGuard.ts`) reads the newest screen lines; a choice list with a
  pointer or a question phrase turns "done" into "asks" (`guardedEnd`). Enter after the pasted
  text would otherwise confirm the preselected answer. The same check runs when the editor saves
  while the session is done (`saveMessage`). A held message is marked `waiting`; the peek says it
  waits for the answer, and the next work after the user's input clears the mark.
- Aider's confirmation prompt (`Run shell command? (Y)es/(N)o [Yes]:`) is a question phrase
  too (`/\(y\)es\/\(n\)o/iu`). The format comes from the security audit of 2026-11-15, not from
  a measurement: [UNVERIFIED] until an Aider screen has been read.
- "done" sends an armed message exactly once (it leaves the queue as it is handed out). "asks"
  keeps it; the peek says it waits for the answer. "other" keeps it disarmed.
- Armed: queued or edited in this run. Disarmed: restored after a restart, or after an exit. A
  disarmed message arms again when a working phase starts that follows the user's own input
  (`SessionTerminal.hasTyped`), so the start-up screen of a resumed program never sends it.
- Saving the editor while the session is already done sends right away.
- Sending reuses the broadcast path: variables filled (`fillVariables`), then `sendToTargets`
  (bracketed paste, Enter as a separate write after `BROADCAST_ENTER_DELAY_MS`). A missing
  variable sends nothing; the message stays queued with a notice.
- A filled variable (`{{diff}}`, `{{selection}}`) can bring text the user did not read. When the
  program has no bracketed paste on and the text has a line break or invisible characters
  (`blockedFor` in `src/broadcast.ts`), nothing is sent: the message stays queued and a notice
  says why. "Send to several" and agent commands follow the same rule.
- Text that is blank once control characters are stripped (`sanitizePaste`, for example only
  `ESC ESC`) sends nothing at all: the paste would be empty and the Enter alone would submit
  whatever the program had in its input. "Send to several" stays disabled for such text
  (`canBroadcast`).
- When a write to a session stalls (`InputStalled` after 30 s), every write of that session
  queued before the stall was known is dropped too (`inputQueue` in `src/sessionInput.ts`), with
  one terminal notice. Otherwise a key could be lost while the Enter typed after it arrives on
  its own. Writes queued after the stall go out as usual. The end of a bracketed paste is sent
  after a stall only when its start had reached the program.
- The "done" that sends the message raises no chime, flash, desktop notification, phone push or
  done notice: the session goes on working.
- Persistence: `localStorage` key `hangar.queuedMessages`, an object of session key to text,
  written once the restore ran, only for sessions that exist (or were remembered before a UI
  reload), cleared when "Remember sessions" is off.

> [!note] Why not in `workspace.json`
> The backend's `Workspace` and `SavedSession` use `deny_unknown_fields`
> (`src-tauri/src/persist.rs`), so a new field would make every save fail until the backend
> knows it. A `queuedMessage: Option<String>` on `SavedSession` is the cleaner home; until then the
> frontend keeps the texts beside the workspace, keyed the same way.

## Stall hint

A working agent never stops printing: its spinner and elapsed time redraw every second, and the
tracker turns "working" into "done" after only a few quiet seconds anyway. So the hint does not
look at raw output but at progress on the screen (`src/stall.ts`):

- Every `STALL_SAMPLE_MS` (10 s; 30 s while saving energy on battery, [[power-detection]])
  `StallMonitor` reads the visible rows of each working agent
  session (`SessionTerminal.screenLines`), removes elapsed times and every character that is no
  letter or digit (spinner glyphs, arrows, separators) and compares the rest with the last sample.
  Token counts and new lines stay, so they count as progress.
- With resource readings on, a CPU reading of the process tree at or above 1 % counts as busy
  (`observeResources`, which never starts polling by itself). A long build prints nothing and is
  not stuck.
- `stallSince` reports the hint once nothing changed for the setting's minutes (default 5, 0 is
  off) and the tree was not busy during that time. The row's status line then reads "nothing new
  for 7 min" in the muted color, never an alarm color; no sound, no notification.
- The hint goes with the next change on screen (within one sample), at once on any state change,
  and after "Send Ctrl+C" from the session menu (`api.write(id, "\x03")`).
- The session menu then offers "Show last lines" (the hover peek opened at once; only for a
  session not on screen) and "Send Ctrl+C".

> [!question] Unverified
> Whether every agent CLI keeps its spinner text stable within one turn. A CLI that rotates its
> spinner words would never show the hint (a missed hint, never a false one).
