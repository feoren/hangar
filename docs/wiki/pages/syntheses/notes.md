---
title: Notes (contract)
type: synthesis
tags: [notes, contract]
sources: ["[[notes-and-start-usability]]", "[[note-gitattributes-measurement]]"]
created: 2026-11-07
updated: 2026-11-10
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
| `set_notes_on_top` | notes | `onTop: boolean` | `null`; keeps the calling window on top or not; any window other than `notes` fails |
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

- Main window (`capabilities/default.json`): `allow-open-notes-window`,
  `core:window:allow-set-focus`; no `core:webview:allow-create-webview-window` (a contract test
  keeps it out).
- `notes` window (`capabilities/notes.json`, `windows: ["notes"]`): the note commands above,
  `allow-notes-to-main`, `allow-set-notes-on-top`, `core:event:allow-listen` and `core:event:allow-unlisten` (listening
  only, never `allow-emit` or `allow-emit-to`), `core:window:allow-close`,
  `core:window:allow-destroy` (the close handler destroys the window once input is saved),
  `core:window:allow-set-focus`, never `core:window:allow-set-always-on-top`, `clipboard-manager:allow-write-text`
  (copy; `navigator.clipboard` fails in WebView2). Nothing else of the app (no sessions,
  profiles, updates, session export). A contract test (`src-tauri/tests/contract.rs`) keeps the
  list this narrow.
- Why the notes window may not emit: Tauri events carry no sender. `Event<T>` in
  `@tauri-apps/api` has only `event`, `id` and `payload`, and `tauri::Event` in Rust (2.12.5) only
  an id and the payload. The main window therefore cannot tell a `tray-quit` from the backend from
  one emitted by another webview; the narrowed capability is the protection. The main window
  should still only act on `tray-quit` and on `notes-message`, whose payload the backend has
  checked.

## Notes UI (frontend, as built)
- `src/NotesPanel.tsx` serves both the right panel and the detached window. List per scope
  (`src/notesScope.ts`), search, trash, one note in the CodeMirror editor (`src/markdownEditor.ts`,
  checkbox widget on `TaskMarker` nodes) or the preview (`src/markdownPreview.ts`, task boxes with
  `data-line`). Text logic in `src/noteText.ts` (progress, toggling, excerpt, word count, captured
  output). Writes are debounced 500 ms and flushed on leaving a note and on quit (`flushNotes`).
- Detached window: the main window writes `hangar.notesWindow` (`{ detached, onTop }`, per
  machine) and `hangar.notesContext` (active session's folder, running sessions) to localStorage,
  which both windows share; the notes window follows them through the `storage` event. The main
  window sends commands as event `notes-command` (`quickNote`, `capture`, `refresh`, `flush`,
  `close`) with `emitTo("notes", …)`; the notes window checks each payload
  (`parseNotesCommand`). The notes window only listens and answers through `notes_to_main`:
  `sendToTerminal` (then `showMain`), and `notesChanged` after a change, which the main window also
  takes as the reply to `flush` on quit (it waits at most the usual quit timeout). The main window
  checks every `notes-message` again (`parseNotesMessage`) before it pastes.
- Sending text from the app (a note's "Send to", a snippet) goes through `src/appPaste.ts`.
  xterm wraps a paste in bracketed paste markers only while the program has DECSET 2004 on
  (`term.modes.bracketedPasteMode`); otherwise every line break becomes CR and each line but the
  last runs at once. In that case, and when the text holds bidi or zero-width characters
  (`hasInvisible`, line breaks, tabs and emoji joiners ignored), the main window shows an inline
  question with Cancel (focused, also Escape) and "Send anyway"; nothing is written before the
  answer, and nothing at all if the session closes meanwhile. Ctrl+V
  and Shift+Insert stay unchanged. The editor marks invisible characters with CodeMirror's
  `highlightSpecialChars`, extended by the bidi embeddings, U+2068 and U+2060 to U+2064.
- The notes window toggles "Always on top" through `set_notes_on_top`, for itself only.
  Closing the notes window (its own close button, or "Bring back") runs its `onCloseRequested`
  handler: `@tauri-apps/api` 2.12 awaits it and then calls `destroy()`, so pending input is saved
  first and `detached` turns false, and the notes are back in the right panel; on quit the
  `flush` command keeps `detached`, and the next start opens the window again. If the window
  vanished without a word, the main window finds out on focus (`Window.getByLabel`), and on quit
  it does not wait for a window that is gone.

## Shared notes (ADR 0006, phase 1)
Decision: [[0006-encrypted-project-notes]]. Code: `src-tauri/src/project_notes.rs` (app),
`crates/hangar-team-crypto/src/notes.rs` (formats), `src/sharedNotes.ts` and
`src/SharedNotesDialog.tsx` (window).

`folder` gains `shared?: boolean`. With `shared: true` the note commands above work on the shared
notes of the repository that contains `path`, in its main worktree's `.hangar/notes/`:
`list_notes` (sorted by last change, `order` 0), `read_note`, `write_note` (writes only when the
text changed), `create_note`, `rename_note`, `delete_note` (the encrypted file moves to
`notes/shared/<vault id>/.trash/` in the app data folder), `list_trash`, `restore_note` (a new id
and a new encryption if the old id is taken), `export_note` (writes the decrypted Markdown where
the person chooses). `reorder_notes` is a no-op; `reveal_note` and `reveal_notes_folder` fail
("Shared notes are encrypted and have no readable file to show"). `search_notes` also searches
the shared notes of every project unlocked in this run, decrypted in memory; such hits carry
`shared: true` and the main worktree as `path`.

| Command | Arguments | Result |
|---|---|---|
| `shared_notes_status` | `folder`, `label`, `untitled` | `SharedStatus` (below); resolves conflicted note files first and stages exactly those files; one call at a time, and `off` without running git when `.hangar/notes` is missing |
| `shared_notes_prepare` | `folder` | `string`: the notes recovery code, shown once; kept in memory until `enable`, `cancel` or 15 minutes, and a second call before that returns the same code |
| `shared_notes_save_code` | `folder`, `heading` | `{ outcome: "saved", path } \| { outcome: "cancelled" \| "insideRepository" }`; the dialog starts in Documents (else Desktop); a place inside the repository or any Git work tree is refused and nothing is written |
| `shared_notes_copy_code` | `folder` | `null`; copies the prepared code marked as secret, clears the clipboard after 60 s if it still holds the code |
| `shared_notes_cancel` | `folder` | `null`; drops the prepared code (the dialog closed) |
| `shared_notes_enable` | `folder`, `label`, `untitled` | `SharedStatus`; stores the key ring in the keychain, writes `.gitattributes`, `README.md`, `vault.json`; stages nothing |
| `shared_notes_unlock` | `folder`, `code`, `label`, `untitled` | `{ outcome: "unlocked", status } \| { outcome: "wrongCode" \| "typo" \| "teamCode" }` |
| `move_note` | `from`, `id`, `to` | `string`: the new id; created in `to`, the source goes to its trash |

```ts
type SharedStatus =
  | { state: "unavailable" }                       // no Git repository: no Shared group
  | { state: "off" }                               // setup line with "Turn on"
  | { state: "locked"; reason: "needsCode" | "team" | "damaged" } // "Enter code" only for needsCode
  | { state: "newerFormat" }                       // no action
  | { state: "unlocked"; uncommitted: number; ignored: boolean; unreadable: number;
      resolved: { title: string; copy: string | null }[] };
```

- `label`: the window's word for "other version" (at most 40 characters, no control characters,
  no `(`, `)` or `,`); a conflict copy is titled `Title (label, Name)`, in solo mode
  `Title (label)`. `untitled`: the window's title for a note whose own title the app would
  refuse (the last one sent is used by every later call).
- Conflicts: `UU`, `AA`, `UD` and `DU` on `<uuid>.note` are resolved when both present sides open:
  the later `updated` keeps the id (ours on a tie), the other becomes a new note; a deleted side
  loses; equal versions need no copy. Hangar runs `git add -- <paths>` for exactly these files and
  never commits. Stages are read as the XY code promises; a stage that cannot be read whole
  leaves the conflict. `DD` is cleared with `git rm --cached`. A `vault.json` conflict is resolved
  only when both sides are of this format and both key rings are on this computer (the id that
  sorts first stays); otherwise it stays for the person.
- `uncommitted`: entries of `git status --porcelain=v1 -- .hangar/notes`; `ignored`:
  `git check-ignore` of `vault.json`; `unreadable`: note files that do not open here.
- The notes window has the same commands (`capabilities/notes.json`).

UI: "This project" (and another opened project) shows two groups, **Shared** (lock icon, "in the
repository", its own "+") and **Only on this computer** (today's notes, its own "+"). The Shared
group shows one line per state with at most one action; quiet hints below say what is not
committed, ignored or unreadable. "All" and search show shared notes with a lock icon. A note's
More menu offers "Move to shared notes" or "Move to this computer". Setup is one dialog: the code,
Copy, Save as file, "I saved the code", and "Turn on" only after that box is ticked.
