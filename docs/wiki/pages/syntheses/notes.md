---
title: Notes (contract)
type: synthesis
tags: [notes, contract]
sources: []
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
| `delete_note` | `folder`, `id` | `null`; the file moves to `.trash/` |
| `reorder_notes` | `folder`, `ids: string[]` | `null`; the given IDs move to the front in that order, the rest follow |
| `reveal_notes_folder` | `folder` | `null`; opens the folder in the file manager (creates it) |

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
