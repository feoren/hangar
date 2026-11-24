---
title: Restoring sessions after a restart
type: synthesis
tags: [sessions, persistence, resume, contract]
sources: ["[[resume-measurement]]", "[[resume-id-measurement]]"]
created: 2026-11-07
updated: 2026-11-24
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
- Before a scrollback is written, `save_scrollback` masks every key shape of the secret guard
  ([[0015-secret-guard]], `secret_scan::mask` in `src-tauri/src/secret_scan.rs`): the body after
  the kind's prefix becomes `*`, one per byte (`ghp_****...`; a private key block keeps its header
  and end line). Same length on purpose: every body byte is ASCII and one column wide, so the
  restored layout (line wraps, cursor moves) stays as it was, and the prefix still tells what was
  there. Terminal escape sequences stay intact; the shapes see them as boundaries, so a token right
  after a colour sequence is found too. Only full matches are masked. A private key without its end
  line within 16 KiB has only base64 runs of 16 or more characters masked. Files saved before this
  change keep their content until the next save of that session.

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
- `cliSessionId` (optional, since 0.3.0, unique across entries): conversation id of the agent
  CLI, a UUID like `key`
  ([[resume]]). Missing or `null` for older files and programs without `session_id_args`.
- `queuedMessage` (optional, since 0.14.0): the message queued for the session
  ([[queued-messages-and-stall-hint]]), not blank, at most 20 000 UTF-16 units (the editor's
  `MAX_QUEUED_CHARS`, counted the way JavaScript counts). Checked on saving and on loading; a file
  with a longer one is not loaded. Left out of the file when there is none, so a workspace without
  queued messages still reads in an older Hangar. With one, an older Hangar refuses the file
  (`deny_unknown_fields`), starts without the saved sessions and overwrites it with its own;
  accepted, because the updater never goes back and every earlier new field (`cliSessionId`,
  `subAgent`) broke a downgrade for every workspace. The file limit is 1 MiB plus room for every
  session's message at its longest with each unit escaped (`MAX_WORKSPACE_BYTES`), so a valid
  workspace always fits.
- `save_workspace` and `load_workspace` answer the main window only (also checked in the
  command, not only through the capability): the workspace holds text Hangar pastes into agents.
- Event on the session channel:
  `{ "type": "resumeFallback", "code": number | null, "cliSessionId": string | null, "subAgents": bool }`. Sent when the
  launch with `resume_args` or `resume_id_args` exited with a nonzero code within 5 s and the user
  did not end it. `cliSessionId` is the id of the normal start that follows (the same id after a
  failed `resume_id_args`, a new one after a failed `resume_args`); `subAgents` replaces
  `SessionInfo.subAgents`.
  The same session (same `id`, same channel) then starts once normally; its
  `output` and `exit` events follow. The failed attempt gets no `exit`. `resumed` is
  `false` afterwards.

## After a crash
Restoring runs the same way after a crash as after a clean quit; what differs is that the window
says so. Backend: `src-tauri/src/exit_marker.rs`, frontend: `src/crashRestore.ts`,
`src/CrashNotice.tsx`.

- **Exit marker.** At the start, first thing in `setup`, the backend reads and then writes
  (`persist::write_atomic`) a marker file in the local data folder: `running.marker`, in debug
  builds `running-debug.marker`. Debug builds run next to the installed Hangar
  ([[single-instance]]); one shared marker would report a crash on every dev start and lose the
  installed one's marker when the dev build quits. Every clean quit removes it: first thing in
  `RunEvent::Exit` (closing the window with quit, the tray's Quit, the process plugin's restart, and on Windows
  a shutdown or logoff: tao turns `WM_ENDSESSION` into `Exit` before it ends the process; first,
  because Windows ends a process that takes more than a few seconds there)
  and through a resource in the app's resource table, which Tauri drops in `cleanup_before_exit`.
  The latter covers the quit paths without an `Exit` event: the Windows updater before it starts
  the installer, and a restart requested on the main thread (verified in tauri 2.12.5 `app.rs`,
  `restart` and `cleanup_before_exit`, and tauri-plugin-updater 2.14.1, `on_before_exit`).
- **SIGTERM and SIGHUP (Linux, macOS).** Without a handler the process dies at the signal (shutdown,
  logoff, a service manager stopping it) and leaves the marker behind. `src-tauri/src/quit_signal.rs`
  installs handlers for both in `setup`, on Tauri's async runtime (tokio's `signal` feature, Unix
  only), and calls `AppHandle::exit(0)` at the first one, so `RunEvent::Exit` removes the marker and
  ends the sessions as after Quit. If the handlers cannot be installed, that is logged and the
  default action stays. SIGKILL cannot be caught and counts as a crash, as intended.
- **Judging.** Only a missing marker means a clean end. A marker that is there, damaged, too large,
  a folder or unreadable means the previous run did not quit cleanly (crash, kill, power loss): a
  wrong notice costs less than a missing one. Without a data folder every start counts as clean.
  The first start of a version that writes the marker finds none and counts as clean.
- **Command** `last_exit_was_clean`: none, `bool`; main window only (`check_main`, error text
  otherwise), granted in `capabilities/default.json` only. The value is the one read at this start;
  it does not change during the run.
- **Notice.** After the restore of a run that did not quit cleanly, and only when sessions were
  saved, a notice in the notice column: "Hangar did not quit cleanly. 4 sessions restored, the
  agents continue their conversations." Without a resumed conversation only the count; with nothing
  restored "No session could be restored." Below it the titles of placeholders ("Not started") and
  of sessions whose resume failed and that started fresh ("Started fresh, without the earlier
  conversation"); the latter follow the `resumeFallback` event, which may arrive seconds after the
  restore. A notice without such titles goes after 15 s; one with them stays until closed. Sessions
  closed in the meantime drop out. No notice after a clean quit, after a reload of the window with
  sessions still running, or with "Remember sessions" off.
- **Scrollback interval: 10 s** (was 30 s), with the same rules: only sessions with new output,
  only after 1.5 s without output, one session per idle callback, the whole file rewritten
  atomically with `sync_all`. A crash now loses at most about 10 s of output. Write load: idle
  sessions write nothing; a session printing without pause writes nothing until it pauses; a busy
  agent with pauses writes its file at most once per 10 s. With a full 2 MiB scrollback that is at
  most 0.2 MB/s per busy session; typical scrollbacks are a few hundred KiB. Four agents busy for
  eight hours a day write a few gigabytes a day with typical scrollbacks and at most about 23 GB
  with full ones, a small share of an SSD's rated endurance (an estimate from these limits, not
  measured). Lower intervals were not chosen: below the 1.5 s
  quiet time plus serializing (tens of milliseconds per session on the main thread) the gain shrinks
  while the cost grows linearly.
- **The last prompt is deliberately not saved.** The agent CLIs bring their conversation back
  themselves when they resume (`--continue`, `resume`, conversation ids, [[resume]]), so a stored
  prompt would add nothing there. And a prompt often holds pasted keys or tokens: Hangar would put
  them on disk unencrypted, next to the scrollback, for every session and every crash. A shell
  session has no conversation to resume; its scrollback shows what was typed up to the last save.

## Recently opened
"Recently opened" is a sidebar group of its own, right above New session, with the same
collapsible header (`layout.ts` `SIDEBAR_GROUPS`, fold kept in the layout). It offers the programs
the person started in a folder that are not open now, so the next start skips choosing a folder
and then a program. It is hidden while empty and not in the collapsed rail. One click, the File
menu or Ctrl+Shift+O (the newest shown entry) starts it again, using the restore path: the same
profile in the same folder, `create_session(..., resume: true, cliSessionId)`, with the title and
marker brought back. A failed resume falls back to a fresh start (`resumeFallback`) as on restore.
A shell has no conversation and just starts fresh in its folder. Code: `src/recentlyOpened.ts`,
`src/RecentSessionList.tsx`, `start`, `close` and `reopenRecent` in `src/App.tsx`.
- **Recorded:** every successful `start` (program list, palette, menu, shortcut, templates, a
  reopen, a handover or partner start in private mode) adds an entry with the session's title and
  the start time. `close` replaces it with the final title, marker and conversation id and the
  close time. A restore after app start does not record; closing a restored session does.
- **Not recorded:** update sessions, sessions started as a sub-agent definition, team sessions
  and any session with a worktree of its own (arena contestants included). That worktree is
  removed on close, and agent CLIs resume by folder ([[resume]]), so the conversation would not
  come back. A close that failed in the backend does not update the entry, because the process
  still holds the conversation.
- **List:** most recent first. One entry per program and folder (`samePath`), and one per
  conversation id, which the backend requires; the newest stays. At most 10 are kept and 5 shown.
  An entry is hidden while a session in the list (running or a placeholder) runs its program in
  its folder or holds its conversation, so the list offers only what is not open. An entry without
  a conversation id resumes the program's last conversation in its folder (`resume_args`).
  Reopening keeps the entry (the new start replaces it); it leaves the list when the start fails
  (a missing folder gives the start's own `bad_cwd` text) or the program is missing or not
  installed (an error notice). The × on a row removes the entry.
- **Row:** program badge and folder name on the first line; the session title (or the program's
  name when the title is just the folder name) and a short relative time (`format.ts` `shortAge`,
  `Intl.RelativeTimeFormat` in the UI language) on the second. Title, program and full path in
  the tooltip.
- **First list:** when the backend holds no list yet, the recent folders (`folders.ts`, newest
  first) are offered with the program the next start would use (`launch.ts` `pickProgram`, only
  installed and shown programs), up to 5, with the folder's name as the title and no
  conversation. Their `closedAt` is 0 (`UNKNOWN_TIME`): no time is shown, and a click starts the
  program fresh. This happens once per machine (`localStorage` flag
  `hangar.recentlyOpened.seeded`), so a list emptied by hand stays empty; the flag only ever
  suppresses entries, so a planted value cannot start anything.
- **Storage:** `recently-closed.json` in the main store (`main_store.rs` `ClosedSession`,
  commands `load_recently_closed` and `save_recently_closed`, main window only); the names stay
  from when only closed sessions were listed, and `closedAt` now means "last used". An entry
  starts a program in a folder, the same threat as the recent folder list, so it never goes to
  `localStorage`. Both sides check every field (program id, folder, title up to 256 characters,
  `#rrggbb` color, emoji up to 16 bytes, lowercase UUID used once, `closedAt` up to
  `Number.MAX_SAFE_INTEGER`). A file with one bad entry, an unknown field or more than 10 entries
  reads as an empty list.
- **Shortcut:** Ctrl+Shift+T, the browsers' key for reopening, already starts a new session, so
  reopening uses Ctrl+Shift+O (`reopenRecent`). Without Shift, Ctrl+O stays with the shell
  (operate-and-get-next in Readline).

## Known limits
- Two sessions of the same agent in the same folder resume the same most recent conversation
  ([[resume]]), except for agents with conversation ids (claude since 0.3.0).
- The scrollback may contain secrets of other shapes (passwords, keys of other services, a key
  split across lines or encoded, a private key whose header line was trimmed off with the oldest
  lines). The known shapes are masked; the rest is stored unencrypted in the user profile.
- Several debug builds running at once share `running-debug.marker`: one that quits removes the
  marker of the others, and one that starts while another runs shows the crash notice. Release
  builds are single instance and unaffected.
- Installing a new version by hand while Hangar runs: the NSIS template of tauri-cli 2.12.4
  (`CheckIfAppIsRunning`, read from the strings in the CLI binary) asks "is running! Click OK to
  kill it" and then calls the Restart Manager with `RmShutdown(..., RmForceShutdown)`. Restart
  Manager first asks a GUI app to end (`WM_QUERYENDSESSION`, `WM_ENDSESSION`) and forces the end
  only if it does not; whether Hangar's `Exit` runs in time there is [UNVERIFIED]. If not, the next
  start shows the crash notice. The in-app update quits cleanly before the installer starts.
- A marker that cannot be removed (no permission, a folder in its place, a file held open by
  another program) is logged on every quit, and every start then shows the crash notice. Deleting
  the file in the local data folder by hand ends that.
