---
title: "Usability test: notes panel and starting a session"
type: synthesis
tags: [notes, usability, new-session, folders, worktree, right-side]
sources: []
created: 2026-11-08
updated: 2026-11-09
---

# Usability test: notes panel and starting a session

Task-based test as a person would use the app (persona: developer with three agents in three
projects), headless Edge with the real frontend and a mocked backend, state of `main` b8b9428 plus
the 0.3.0 branch where noted. 1280 and 720 px, light and dark, keyboard only. No blocker, nine
major findings. The redesign below is the accepted direction for the 0.3.0 rework.

## Notes: findings
| ID | Severity | Finding | Direction |
| --- | --- | --- | --- |
| N1 | major | Opening puts the cursor at position 0 of the last note; typed text lands before the heading and the auto title renames the note. | Ctrl+Shift+N opens a new quick note; existing notes open with the cursor at the end; auto title only for new notes. |
| N2 | major | With overflowing tabs the active tab is not scrolled into view; a new note is invisible. | Replace tabs with a list. |
| N3 | major | No search, no change time, no view across folders. | Search across all notes, times, sort by last change. |
| N4 | major | `- [ ]` is plain text in editor and preview. | Checkbox widget, real checkboxes in preview, progress n/m. |
| N5 | major | No way from a note to the terminal and back. | Send note or selection to a session (bracketed paste); terminal selection to note. |
| N6 | major | With focus in the terminal, Ctrl+Shift+N closes the open panel instead of focusing it. | Toggle only when focus is already in the panel. |
| N7 | major | Below about 832 px the panel covers the terminal without scrim; at 900 px the terminal keeps about 36 columns. | Real overlay with scrim and click outside below 900 px, or collapse the sidebar to the rail. |
| N8 | major | Notes and changes share the right side but exclude each other without a visible switch; headers differ. | One right-side frame with two tabs and one header. |
| N9 | minor | Undo is one level, the toast pushes the editor down, trash sits next to close. | Overlay toast, "Recently deleted" from the trash folder. |
| N10 | minor | Auto title after 900 ms; quick switching leaves "Note 1 … Note 5". | Title on leaving the note. |
| N11 | minor | Scope select labels long and cut; "Without folder" unclear. | Project name first, path in the tooltip, "General". |
| N12 | minor | Tab leaves the editor to the body; no list indent by keyboard. | Tab/Shift+Tab indent in lists, F6 cycles terminal, list, editor. |
| N13 | minor | Preview toggle is global; no word count. | Per note, live rendering where possible. |
| N14, N15 | polish | Two navigations (sidebar group and select); icon-only header actions. | One navigation; labeled "More" menu. |

## Starting a session: findings
| ID | Severity | Finding | Direction |
| --- | --- | --- | --- |
| S1 | major | "Starts in" is a global folder, independent of the active session, while notes follow the session: two mental models. | Default target is the active session's folder; the target is visible on every start control. |
| S2 | major | The folder menu omits running projects; favorites can only be toggled for the current folder; the star icon is inverted. | Sections Running projects, Favorites, Recent; a star per row. |
| S3 | major | "Own worktree" is sticky, memory only, only in the sidebar; the reason for disabled is a tooltip. | Decide per start ("+ Worktree"), reset after start, visible reason. |
| S4 | major | The rail "+" and the empty state hide folder and worktree. | Target and worktree switch in the rail menu and empty state. |
| S5 | minor | Ctrl+Shift+T and launcher entries do not show or cover running projects. | Show the target briefly; launcher entries for all projects. |
| S6 to S9 | minor, polish | Ambiguous folder names, numbered titles, two-step folder choice, active session cut off. | Middle-elided paths, branch in titles, one step, scroll active into view. |

## Accepted direction
- Notes: chips "This project", "All", "General"; compact list with title, excerpt and time above
  the editor; quick note on Ctrl+Shift+N; send to terminal and terminal selection to note;
  checklists; search; copy and export; detach into its own window (optionally always on top) with
  a `notes` window label and its own capability; shared right-side frame "Notes | Changes".
- Starting: one target, default the active session's folder; menu with running projects,
  favorites and recent; worktree decided per start; target shown in sidebar, rail, empty state,
  Ctrl+Shift+T hint and launcher.

Backend needs: a capability and window configuration for the detached notes window, optional
`search_notes`, restore from trash, export and reveal of a single note.

## As built: starting a session (S1 to S9)
- **One start target** (`src/startTarget.ts`): the active session's project folder (the repository
  for a worktree session, nothing for an update session), without one the most recent start
  (`FolderPrefs.recent[0]`, the old `hangar.lastCwd` only as a fallback), then the home folder. A
  folder chosen in the folder menu or the folder dialog applies to the next start in the target
  only; the sidebar labels it "Next start in" with an accent edge and a reset button. Choosing the
  default itself is no deviation. No setting for the old fixed folder: it would bring back the
  second mental model of S1, and favorites cover the "always this folder" case.
- **Target everywhere:** sidebar group New session (name plus middle-elided parent, full path and
  source in the tooltip), rail "+" menu (first row "Starts in <folder>" opens the folder menu, a note
  when no worktree is possible), empty state ("Starts in <folder> · Change", reset when chosen), a
  2.5 s hint after Ctrl+Shift+T, quick access.
- **Folder menu** (`FolderMenu.tsx`, sections in `src/folderSections.ts`): Running projects,
  Favorites, Recent, each folder once in that order; a star per row (filled = favorite,
  `menuitemcheckbox`, the menu stays open and rows do not move while it is open); "Choose another
  folder …". Same names get parent folders (`acme\web`, `fork\web`), details are parents shortened
  in the middle (`C:\Users\…\clients\acme`). Keyboard: Right and Left reach the star, Shift+Enter
  toggles it from the row (`menuNav.secondaryCommand`).
- **Worktree per start:** "+ Worktree" over the end of each program row on hover or focus, Shift+click
  on a program, "+ Worktree" below each card in the empty state, a worktree button per program in
  the rail menu. Nothing is sticky. One visible line below the target says "Shift+click: own
  worktree" or "Not a git repository, so no own worktree" (nothing while the branch is unknown).
- **Quick access:** "program in folder" for the target, all running projects, favorites and recent
  folders, plus the worktree variant for every place whose branch is known.
- **Sidebar:** the active session scrolls into view; a worktree session is titled
  "<program> · <branch suffix>" (`worktreeTitle`), the full branch stays in the status line.

## As built: one-click program updates
- `src/programUpdates.ts` parses `plan`, `reason`, `needsAdmin`, `command`, `warnings`, `copies`
  and `planId` (older backends: `canRun` and `updateHint` map to `run` or `copy`; no command means
  `none`), and the `program-update-result` payload. All reason and outcome codes are translated.
- `ProgramUpdateOffer.tsx` (sidebar and Settings > Terminals & Agents): `run` shows
  "2.0.31 → 2.0.35 [Update]" and "Asks for administrator rights." when `needsAdmin`; `copy` shows
  the new version, "Copy command" and the reason; `none` the version and the reason. A second copy
  opens a list of all copies on demand. The rail menu lists runnable updates.
- When the program runs in a session, "Update" asks inline first (close its sessions, or "Update
  anyway"); from the rail the sidebar opens to show that question. Otherwise one click starts the
  update session with `expectedPlan = planId`; a refusal because the plan changed shows a notice
  with "Check again".
- After `program-update-result`: a notice with the outcome (updated from X to Y, still running,
  failed with exit code, another copy earlier on PATH with the copies, unchanged), "Show session";
  a good result fades after 12 s, a problem stays. Versions are queried again.

## As built: notes and the right panel (N1 to N15)
- **Quick note (N1, N6, N10):** Ctrl+Shift+N (`quickNote` in `src/shortcuts.ts`) opens the right
  panel with a new quick note and focuses it, from the terminal or anywhere else; pressed while the
  focus is in the notes it closes them and the terminal gets the focus back. A quick note exists
  in the backend only once it has content; one that ends up empty is moved to the trash on leaving.
  Existing notes open with the cursor at the end. Only notes created in this run (or still named
  "Note n") take their title from the first line, and only on leaving the note (back to the list,
  another note, focus out of the panel, closing). A title you chose stays.
- **List instead of tabs (N2, N3, N11, N14):** chips "This project" (follows the active session;
  hidden without one), "All", "General", plus a removable chip for another project opened from
  quick access. Rows show title, an excerpt, checklist progress and relative time, sorted by last
  change; in "All" and in search results the project name comes first, the path is the tooltip.
  The search field searches all notes through `search_notes` (200 ms after the last keystroke).
  Opening a note folds the list into a bar (back, title, progress, More). The sidebar's notes
  group is gone: the panel is the one navigation.
- **Checklists (N4, N12):** `- [ ]` and `- [x]` show as checkboxes in the editor (bullet hidden,
  click toggles the text) and as real checkboxes in the preview (click or Space). Progress "2/5"
  in the bar and the list, with an icon and a screen reader text. Tab and Shift+Tab indent list
  items; elsewhere Tab still leaves the editor. F6 and Shift+F6 cycle terminal, list, editor while
  the notes are open (`focusCycleStep`, `nextFocusZone`); otherwise F6 stays with the program.
- **Send to terminal (N5):** "Send to <session>" in the footer sends the selection or the whole
  note through `SessionTerminal.pasteText` (bracketed paste when the program enabled it, never
  Enter), then shows that session with the focus in it. A chevron picks another running session.
  The terminal context menu has "Add selection to note": into the open note, otherwise into a new
  quick note, as a code block below a line with time and session.
- **One right panel (N7, N8):** `SidePanel.tsx` with the tabs "Notes | Changes", detach and close.
  Below 900 px window width, or when terminal and panel do not fit side by side, it lies over the
  window with a scrim; a click on the scrim or Escape closes it (`placeNotes` in `src/layout.ts`).
- **Delete, trash, More (N9, N15):** delete lives in the labeled "More" menu (Rename, Copy as
  Markdown, Export …, Show in Explorer or Finder, Delete note). Deleting goes to the trash at once;
  an overlay toast offers Undo (`restore_note`). The list's More menu has "Recently deleted"
  (`list_trash` for the scopes shown, Restore per row) and "Open folder".
- **Detach:** "Open in own window" calls `open_notes_window` with the remembered "Always on top";
  `src/main.tsx` renders `NotesWindow.tsx` for `?window=notes`. Details in [[notes]].
- **Preview and footer (N13):** the preview toggle is remembered per note while Hangar runs and
  renders again after each typing pause; the footer shows the word count (`Intl.Segmenter`).
- **Attention follow-ups:** a waiting session scrolled out of the sidebar list shows "1 waiting
  below" or "above" at that edge (IntersectionObserver, `src/waitingHint.ts`); a click scrolls to
  the nearest one. `attentionEdges` in `src/attentionLook.ts` keeps edge, ring and chip at 3 : 1
  against the sidebar in both themes: the dark theme lightens a color that is too dark, the light
  theme darkens as before.
- Checked headless (Edge, mocked backend, 1280 and 720 px, dark and light): the tasks above as a
  person would do them. Not checked in the real window: the detached window (creation, on top,
  closing it docks the notes back), bracketed paste in real shells, Explorer and the save dialog.
