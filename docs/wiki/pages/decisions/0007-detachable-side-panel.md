---
title: "ADR 0007: Detachable side panel"
type: decision
status: accepted
superseded_by:
tags: [ui, windows, tauri, capabilities, notes, changes, files]
sources: []
created: 2026-11-12
updated: 2026-11-12
---

# ADR 0007: Detachable side panel

## Context
The right panel (`src/SidePanel.tsx`) has three tabs: Notes (`NotesPanel`), Changes
(`ChangesPanel` with "Open | Commits" and `CommitHistory`) and Files (`FilesPanel`, which opens
read-only file viewer windows). Until 0.6.0 only the notes could move into a window of their
own (label `notes`, `notes_window.rs`, `capabilities/notes.json`, `NotesWindow.tsx`). The owner
wants the whole right panel detachable, for example on a second monitor next to the terminal.

What the tabs need, and where it lives today:

- Notes: the active session's folder and id, the running sessions as "Send to" targets.
- Changes: the active session's folder, its attention state (a change reads the diff again), its
  claims and overlaps from the local claims snapshot, teammates' overlaps from the team
  activities, session names, and "show that session" from a claims warning.
- Files: the active session's id and folder, the whole claims snapshot (change letters,
  "also in" another session), the team activities (avatars), session names, opening a viewer.

All of it is state of the main window (`App.tsx`): the session list, `useClaims` (event
`claims-changed`, delivered to `main` only), `useTeam` (event `team-changed`, `main` only, plus
commands that change team state), the attention tracker. The backend emits these events to the
main window alone (`src-tauri/src/lib.rs`).

## Options

1. **Notes stay the only detachable tab.** No work; does not meet the request.
2. **A second, independent app instance of the panel**: the panel window subscribes to
   `claims-changed` and `team-changed` itself and gets `claims_snapshot` and the team commands.
   Two sources of truth (the attention state does not exist in the backend at all), the backend
   events must go to two windows, and the panel window needs read access to team state it never
   changes. Rejected: more permissions, more ways to disagree.
3. **One detached window type for the whole panel; the main window stays the single source of
   truth and pushes a compact context.** The panel window renders the same tab components with
   props built from that context; it asks the main window for actions through the existing
   validated-message pattern.
4. **One window per tab.** More windows to place and remember, three capabilities, no benefit
   over a tab strip in one window.

For the push from main to panel in option 3:

- a. a backend relay command (`main_to_panel`) that re-emits a validated payload to `panel`;
- b. `emitTo("panel", …)` from the main window, as the notes window already received its
  commands;
- c. localStorage plus the `storage` event, as the notes window received its context.

## Decision
Option 3 with transport b.

- **One window type.** The notes-only window is replaced by the panel window: label `panel`, URL
  `index.html?window=panel`, created by the backend only (`open_panel_window`), at least
  320 x 360. The commands `open_notes_window`, `set_notes_on_top`, `notes_to_main`, the module
  `notes_window.rs`, `capabilities/notes.json` and `NotesWindow.tsx` are removed;
  `panel_window.rs`, `capabilities/panel.json` and `PanelWindow.tsx` take their place. The
  localStorage prefs move from `hangar.notesWindow` to `hangar.panelWindow`
  (`{ detached, onTop, tab, bounds }`); the old key is read once when the new one is missing.
- **Main to panel: `emitTo`.** The main window already holds `core:event:default` (emit and
  emit-to to any window); a backend relay (a) would add a command and a second validation layer
  for a sender that is trusted anyway, and could not restrict anything the main window cannot
  already do. localStorage (c) would write the claims snapshot synchronously on every change and
  keep a stale copy on disk. The context goes as event `panel-context`, commands as
  `panel-command`. Only the main window can emit (the panel and the viewers hold
  `core:event:allow-listen` only), and the panel window validates every payload
  (`parsePanelContext`, `parsePanelCommand`) and refuses a malformed context as a whole.
- **The context is compact.** Active session id and folder, its attention state, every
  session's id, title and running flag, the claims snapshot, and the team activities trimmed to
  `peers`, `overlaps` and `sessionRepos` (tasks, notices, attempts, linked repositories and other
  devices are not sent). The main window sends only when the JSON changed; the panel's `ready`
  message forces a send, so a window that opens later (or after a restart) gets the context
  within one round trip.
- **Panel to main: `panel_to_main`.** A closed, validated `PanelMessage`: `sendToTerminal`,
  `notesChanged`, `showMain`, `showSession { sessionId }` (the backend brings the main window
  to the front, the main window ignores unknown ids) and `ready {}`. Delivered to `main` only.
- **Opening a file viewer from the panel:** the panel calls `open_file_viewer` directly;
  `check_opener` accepts `main` and `panel`. The command already resolves the session's folder
  in the backend and keeps the path inside it, so relaying it through the main window would add
  a hop without adding a check.
- **Docking.** A "detach" button in the panel header (every tab) moves the panel into its
  window on the current tab; the main window hides its panel and the terminal gets the space.
  The sidebar buttons, menu entries and shortcuts for notes, changes and files then bring the
  window to the front on that tab. The window's "Back into the main window" button and its
  system close button both close the window; the close handler saves pending note input and sets
  `detached: false`, and the main window opens its docked panel on the window's last tab.
- **Restarts.** The detached state is remembered (it was for the notes window already): on quit
  the main window's `flush` command marks the panel as quitting, so closing it does not dock it,
  and the next start opens the window again.
- **Size and place** are remembered by the frontend (`bounds` in `hangar.panelWindow`, logical
  pixels from `screenX`, `screenY`, `innerWidth`, `innerHeight` after move or resize). The
  backend clamps the size and uses the position only while the top left of the title bar lies on
  a monitor (`panel_window::placement`, unit-tested), so an unplugged monitor cannot hide the
  window. No window-state plugin: one window, a pure function, no new dependency.

## Consequences
- One detached window type and one capability instead of a notes-only special case; the
  contract test pins the exact permission list of `panel.json`.
- The panel tabs are the same components as in the docked panel; only the data source differs.
  New tabs need their data added to the context or a read command in `panel.json`.
- Live sync costs one IPC event per context change. The claims snapshot is the largest part; it
  changes at the pace of the claims worker, not per keystroke. Sessions' output does not change
  the context.
- The panel window does not receive gate decisions, team tasks or notices; those stay in the
  main window.
- Data in localStorage from 0.5 (`hangar.notesWindow`, `hangar.notesContext`) is migrated or
  removed on the first write.

## Security
- `capabilities/panel.json` (`windows: ["panel"]`): the note commands, `git_diff`, `git_log`,
  `git_commit_diff`, `list_session_folder`, `open_file_viewer`, `panel_to_main`,
  `set_panel_on_top`, listen and unlisten, close, destroy and focus of itself, clipboard write.
  No session commands (nothing that starts, writes to, resizes or ends sessions); text reaches a
  session only as a validated paste through the main window (`panel_to_main`). No claims, gate,
  team, account, profile, update, export or dialog commands, no `emit`, no `set_always_on_top`.
- New reach compared with the notes window: reading git changes and history of a folder named
  by the caller (`git_diff`, `git_log`, `git_commit_diff` take a path, as for the main window),
  listing a session's folder and opening a viewer for a file inside it (both resolve the folder
  from the session id in the backend). Accepted: the panel renders note Markdown like the notes
  window did, so an injection there would previously have reached all notes; git output of a
  local folder is of the same sensitivity. Narrowing the git commands to a session id is a
  possible follow-up.
- The panel cannot emit events, so it cannot fake `tray-quit`, `panel-message` or any backend
  event; everything it asks goes through `panel_to_main` and is validated twice (serde with
  `deny_unknown_fields` and `validate()` in Rust, `parsePanelMessage` in the main window).
- `open_panel_window` takes only booleans and four numbers; non-finite or absurd bounds fall
  back to the default size.

## Evidence
- Tauri events carry no sender (see [[notes]], section "Detached side panel window").
- `@tauri-apps/api` 2.12 awaits `onCloseRequested` handlers before `destroy()` (as built for
  the notes window, [[notes]]).

> [!question] Unverified
> `window.screenX`/`screenY` in WebView2 and WebKitGTK/WKWebView report the outer window
> position in the same logical pixels that `WebviewWindowBuilder::position` takes; checked by
> reasoning only, not measured on mixed-DPI setups or on macOS and Linux (Wayland does not let
> apps place windows at all).
