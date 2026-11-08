---
title: Restoring sessions after a restart
type: synthesis
tags: [sessions, persistence, resume, contract]
sources: ["[[resume-measurement]]", "[[resume-id-measurement]]"]
created: 2026-11-07
updated: 2026-11-08
---

# Restoring sessions after a restart

Goal: after closing and reopening, the sessions are back, with their previous scrollback and,
where possible, with the agent conversation resumed. Processes do not survive closing (no
background service, a non-goal). Backend: `src-tauri/src/persist.rs`, `src-tauri/src/manager.rs`.

## Flow (proposal for the frontend)
1. On sidebar changes, `save_workspace`; periodically and on close, `save_scrollback` per session
   (the frontend serializes the scrollback, e.g. with the xterm.js serialize addon; trim older content
   so that 2 MiB is enough).
2. On startup, `load_workspace`; for each entry, write `load_scrollback(key)` into the terminal, then
   `create_session(..., resume: true, cliSessionId)` with the entry's stored `cliSessionId`.
   Store `SessionInfo.cliSessionId` (from `create_session`, `restart_session`) and the
   `cliSessionId` of a `resumeFallback` event in the entry on the next `save_workspace`.
3. When a session is closed, `delete_scrollback(key)`; "clear scrollback" = `clear_scrollbacks`.

## Storage
- Folder: Tauri `app_local_data_dir()` = `dirs::data_local_dir()/<identifier>` (verified in tauri 2.12.5,
  `src/path/desktop.rs`). Windows `%LOCALAPPDATA%\<identifier>`; macOS and Linux according to `dirs`
  `~/Library/Application Support/<identifier>` or `$XDG_DATA_HOME` or `~/.local/share/<identifier>`
  respectively (not verified against the dirs docs).
- `workspace.json` and `scrollback/<uuid>.txt`. Writes are atomic (temporary file, `sync_all`, `rename`).
- Permissions: on Unix, folder 0700 and files 0600. Windows: default ACL of the user profile (user, SYSTEM,
  Administrators), no custom ACL.
- Contents are never logged; an unusable file yields `null` and a log line with the reason, without content.

## Contract

| Command | Arguments | Result |
|---|---|---|
| `save_workspace` | `state: Workspace` | `null`; error text on invalid state or I/O |
| `load_workspace` | none | `Workspace` or `null` |
| `save_scrollback` | `key`, `data` (at most 2 MiB UTF-8) | `null`; error on an invalid key or if too large |
| `load_scrollback` | `key` | `string` or `null`; error only on an invalid key |
| `delete_scrollback` | `key` | `null` (also `null` if the file does not exist) |
| `clear_scrollbacks` | none | `null`; deletes only `<uuid>.txt` and its own temporary files |
| `create_session` | as before, plus optional `resume: bool` and optional `cliSessionId: string` (UUID) | `SessionInfo` with `resumed`, `cliSessionId` (`string` or `null`) and `subAgents` (`bool`); error text if `cliSessionId` is not a UUID |

```json
{
  "version": 1,
  "sessions": [
    { "key": "0f8fad5b-d9cb-469f-a165-70867728950e", "profileId": "claude",
      "cwd": "C:\\Users\\x\\projekt", "title": "Claude Code", "order": 0,
      "cliSessionId": "7c9e6679-7425-40de-944b-e07fc1f90ae7" }
  ],
  "activeKey": "0f8fad5b-d9cb-469f-a165-70867728950e"
}
```

- `key`: UUID 8-4-4-4-12 (case-insensitive, stored in lower case), unique.
  `activeKey`: `null` or one of the `key` values. `version` must be 1. Unknown fields are an error.
- `cliSessionId` (optional, since 0.3.0): conversation id of the agent CLI, a UUID like `key`
  ([[resume]]). Missing or `null` for older files and programs without `session_id_args`.
- Event on the session channel:
  `{ "type": "resumeFallback", "code": number | null, "cliSessionId": string | null, "subAgents": bool }`. Sent when the
  launch with `resume_args` or `resume_id_args` exited with a nonzero code within 5 s and the user
  did not end it. `cliSessionId` is the id of the normal start that follows (the same id after a
  failed `resume_id_args`, a new one after a failed `resume_args`); `subAgents` replaces
  `SessionInfo.subAgents`.
  The same session (same `id`, same channel) then starts once normally; its
  `output` and `exit` events follow. The failed attempt gets no `exit`. `resumed` is
  `false` afterwards.

## Known limits
- Two sessions of the same agent in the same folder resume the same most recent conversation
  ([[resume]]), except for agents with conversation ids (claude since 0.3.0).
- The scrollback may contain secrets (printed tokens). It is stored unencrypted in the user profile.
