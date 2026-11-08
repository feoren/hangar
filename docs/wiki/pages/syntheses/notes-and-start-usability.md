---
title: "Usability test: notes panel and starting a session"
type: synthesis
tags: [notes, usability, new-session, folders, worktree, right-side]
sources: []
created: 2026-11-08
updated: 2026-11-08
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
