---
title: Notes (contract)
type: synthesis
tags: [notes, contract]
sources: ["[[notes-and-start-usability]]"]
created: 2026-11-07
updated: 2026-11-08
---

# Notes (contract)

Decision and storage: [[0004-notes-per-working-folder]]. Code: `src-tauri/src/notes.rs`.
All commands run in a blocking thread in the backend; errors arrive as readable text (English).

`folder` is always `{ path: string | null }`: the working folder (absolute) or `null` for "General".

| Command | Arguments | Result |
|---|---|---|
| `list_note_folders` | none | `[{ folderKey, path \| null, noteCount, updatedAt \| null }]`; "General" always first, then scopes with notes, most recent first |
| `list_notes` | `folder` | `[{ id, title, updatedAt \| null, order }]`, sorted by `order`; if the scope does not exist: `[]` |
| `read_note` | `folder`, `id` | `string` (Markdown) |
| `write_note` | `folder`, `id`, `content` (at most 1 MiB UTF-8) | `null`; atomic |
| `create_note` | `folder`, `title` | `id` (UUID); creates the scope if needed, the new note goes to the end |
| `rename_note` | `folder`, `id`, `title` | `null`; the file is renamed along with it, `id` stays |
| `delete_note` | `folder`, `id` | `null`; the file moves to `.trash/`, ID and title are kept in `.trash/trash.json` |
| `reorder_notes` | `folder`, `ids: string[]` | `null`; the given IDs move to the front in that order, the rest follow |
| `reveal_notes_folder` | `folder` | `null`; opens the folder in the file manager (creates it) |
| `search_notes` | `query` | `[{ folderKey, path \| null, id, title, excerpt, updatedAt \| null }]`; all scopes, case-insensitive substring in title or content (no regex); title hits first, then newest; at most 50 |
| `list_trash` | `folder` | `[{ id, title, deletedAt \| null }]`, most recently deleted first; `[]` without a trash |
| `restore_note` | `folder`, `id` (from `list_trash`) | `id` of the restored note (the old one unless taken again); goes to the end of the list, file name follows the title |
| `export_note` | `folder`, `id` | `string \| null`: path written, `null` if the save dialog was cancelled; the backend opens the dialog (`.md`, name from the title) |
| `reveal_note` | `folder`, `id` | `null`; selects the file in Explorer (`explorer.exe /select,"<path>"`) or Finder (`open -R`); Linux opens the folder |

- Stored folder names: `general` for the General area and `.trash` for the trash. Older installs
  used `allgemein` and `.papierkorb`; the backend renames them once on startup.
- `updatedAt`: Unix time in ms (modification time of the file).
- Title: not empty after trimming, at most 200 characters, no control characters (no line break).
  The file name is derived from it and may differ; the frontend always shows `title`.
- Errors: invalid title, invalid ID (not a UUID), "note not found", path not absolute,
  too large, not UTF-8, I/O error. Error texts never contain note content.
- Concurrent calls are safe (a lock just for notes); the frontend should still debounce writes
  (e.g. 500 ms after the last keystroke and on tab switch).
- Files edited externally: `read_note` always reads the file; newly added `.md` files
  show up on the next `list_notes` with a new ID.
- Search: `query` is trimmed; empty finds nothing; more than 200 characters or a control character
  is an error ("Invalid search"). Files above 1 MiB or not UTF-8 are skipped, at most 5000 files
  are read per search. Read only: no scope is synced or rewritten. `excerpt`: up to 160
  characters around the first hit in the content, whitespace collapsed, `…` at a cut; the start
  of the content when only the title matches.
- Trash: `.trash/trash.json` (`{ version: 1, items: [{ id, title, file, deletedAt }] }`) records
  each deletion. `.md` files in `.trash/` without a record (deleted before 0.3.0) are adopted on
  `list_trash` with a new ID, title from the file name without the `<millis>-` prefix, and that
  prefix as `deletedAt`. Nothing is ever deleted for good. A record whose `file` is not a plain
  `.md` name inside `.trash/`, or whose ID or title is invalid, is ignored (its file, if there, is
  adopted again), so an edited `trash.json` cannot move a foreign file into the notes.

## Detached notes window
Code: `src-tauri/src/notes_window.rs`. The backend creates the window; the frontend cannot create
webviews. The backend keeps no window geometry.

| Command | Window | Arguments | Result |
|---|---|---|---|
| `open_notes_window` | main | `alwaysOnTop: boolean` | `null`; creates the window with label `notes`, URL `index.html?window=notes`, 440 x 640, at least 320 x 360, title "Hangar"; if it exists, only unminimizes, shows and focuses it |
| `set_notes_on_top` | main | `onTop: boolean` | `boolean`: `false` if the window is not open |
| `notes_to_main` | notes | `message: NotesMessage` | `null`; delivers the validated message as event `notes-message` to the window `main` only |

```ts
type NotesMessage =
  | { kind: "sendToTerminal"; text: string; sessionId?: number | null } // main pastes it (bracketed paste)
  | { kind: "notesChanged"; folderKey: string }                          // main reloads that scope
  | { kind: "showMain" };                                                // backend also brings main to the front
```

- Validation: unknown kinds or extra fields are rejected when the arguments are read.
  `sendToTerminal.text`: not empty, at most 1 MiB, no control characters except tab, CR and LF
  (an ESC could end bracketed paste early). `folderKey`: `general` or 32 hex digits.
  `notes_to_main` called from any window other than `notes` fails ("Only the notes window may
  send notes messages").
- The frontend opens the notes view when the URL has `?window=notes`.

Capabilities:

- Main window (`capabilities/default.json`): `allow-open-notes-window`, `allow-set-notes-on-top`,
  `core:window:allow-set-focus`; no `core:webview:allow-create-webview-window` (a contract test
  keeps it out).
- `notes` window (`capabilities/notes.json`, `windows: ["notes"]`): the note commands above,
  `allow-notes-to-main`, `core:event:allow-listen` and `core:event:allow-unlisten` (listening
  only, never `allow-emit` or `allow-emit-to`), `core:window:allow-set-always-on-top`,
  `core:window:allow-close`, `core:window:allow-set-focus`, `clipboard-manager:allow-write-text`
  (copy; `navigator.clipboard` fails in WebView2). Nothing else of the app (no sessions,
  profiles, updates, session export). A contract test (`src-tauri/tests/contract.rs`) keeps the
  list this narrow.
- Why the notes window may not emit: Tauri events carry no sender. `Event<T>` in
  `@tauri-apps/api` has only `event`, `id` and `payload`, and `tauri::Event` in Rust (2.12.5) only
  an id and the payload. The main window therefore cannot tell a `tray-quit` from the backend from
  one emitted by another webview; the narrowed capability is the protection. The main window
  should still only act on `tray-quit` and on `notes-message`, whose payload the backend has
  checked.
