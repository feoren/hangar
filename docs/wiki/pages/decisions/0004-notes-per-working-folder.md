---
title: "ADR 0004: Notes per working folder"
type: decision
status: accepted
superseded_by:
tags: [notes, persistence, markdown]
sources: []
created: 2026-11-07
updated: 2026-11-08
---

# ADR 0004: Notes per working folder

## Context
The user wants a notes panel with tabs like Windows Notepad, with Markdown content. Notes
must not disappear when a session is closed. Sessions are short lived (processes do not survive
a restart, [[session-restore]]); a session's working folder, on the other hand, is the
project the notes are about.

## Options
1. **On the session** (session key from `workspace.json`). Once the user closes the session, the
   notes are gone or orphaned; two sessions in the same project would have separate notes.
2. **In the project** (`<project>/.hangar/notes`). Visible to Git and collaborators, easily ends up in
   commits, needs write access to the project, changes other people's repositories and runs into sync conflicts
   (Dropbox, OneDrive).
3. **localStorage in the WebView.** No real file access, no Markdown in Explorer, lost when the
   WebView data is cleared, size limit, cannot be checked by the backend.
4. **Per working folder, stored in the app data folder** as real `.md` files, plus a General area.

## Decision
Option 4. Stored under `app_local_data_dir()/notes/<key>/` (Windows `%LOCALAPPDATA%\<identifier>`):
- Key `allgemein` for notes without a folder (renamed to `general` on 2026-11-08, see Addendum), otherwise
  the first 32 hex digits (128 bits) of SHA-256 over the normalized path. A hash instead of the path in the
  folder name avoids length problems and special characters and does not reveal paths in file names;
  `folder.json` keeps the original path for display.
- Normalization: separators unified, `.` and doubled separators removed, no trailing separator,
  `\\?\C:\` treated as `C:\`, `\\?\UNC\s\x` as `\\s\x`. On Windows also lowercased, because NTFS
  is case insensitive by default and the folder dialog, the shell and typed input spell the
  same folder differently. No resolution of `..` and no `canonicalize`: the folder
  may be missing, sit on a slow network drive or be reached through symbolic links.
- File name from the title (readable in Explorer), ID (UUID v4) only in `folder.json`, so the ID stays
  stable across renames. Safe names: no path components or forbidden characters, no trailing dot or space,
  no leading dot, reserved device names (CON, NUL, COM1 …) get a prefix, at most 80
  characters, duplicates get " (2)".
- Deleting moves a note to `.papierkorb/` (renamed to `.trash/` on 2026-11-08, see Addendum). `.md` files
  added from outside are picked up, missing files drop out of the list; a corrupt `folder.json` is set
  aside and rebuilt.
- Permissions as for the scrollback (Unix 0700/0600, Windows default ACL of the profile); contents are never logged.

## Consequences
- Notes survive sessions and restarts and belong to the project, not to the session.
- If a project is moved or renamed, its notes stay with the old path (new key).
  Possible remedy later: "Take over notes" in the frontend.
- macOS and Linux are case sensitive here; on a case insensitive APFS volume
  `~/Repo` and `~/repo` give two scopes. Accepted, because the path usually comes from the same dialog.
- The frontend does not need read access to the files; `reveal_notes_folder` opens the folder
  in the file manager (Windows `explorer.exe`, macOS `open`, Linux `xdg-open`, argv without a shell, no plugin).
- Contract: [[notes]].

## Addendum (2026-11-08): English folder names
The stored folder names are now English: the key for notes without a folder is `general` (was
`allgemein`), and the trash folder is `.trash` (was `.papierkorb`). On startup a one-time migration renames
existing folders, so that notes written by earlier versions remain available.

## Evidence
- Our own tests in `src-tauri/src/notes.rs` (normalization, file names, reconciliation, trash).
- Tauri `app_local_data_dir` = `dirs::data_local_dir()/<identifier>`, checked in tauri 2.12.5
  `src/path/desktop.rs`.
