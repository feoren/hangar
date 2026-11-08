---
title: "ADR 0001: Hangar's foundation"
type: decision
status: accepted
superseded_by:
tags: [architecture, tauri, pty, xterm]
sources: ["[[conpty-cursor-query-measurement]]"]
created: 2026-11-07
updated: 2026-11-07
---

# ADR 0001: Hangar's foundation

## Context
A first runnable foundation: a sidebar with the sessions on the left, the active terminal on the right.
The first audience is our own use on Windows; the code stays platform neutral.
Scope: launch from a profile and a folder, switch, status, exit, rename, restart.
Restoring sessions after an app restart is deliberately left out.

## Options
- **A: Tauri 2 + portable-pty + xterm.js** (the planned stack)
- B: Electron + node-pty (larger, departs from the planned stack)
- C: Wails (Go) + ConPTY (less widely used PTY libraries)

## Decision
A, with these detailed decisions:

| Topic | Decision | Why |
|---|---|---|
| Profiles | `profiles.toml` in the app config folder (`%APPDATA%\com.feoren.hangar\`), created with default profiles on first start | TOML allows comments and is easy to edit by hand; `deny_unknown_fields` reports typos |
| Program lookup | our own search over PATH and PATHEXT (`src-tauri/src/resolve.rs`) | a clear "not found" message; `codex.cmd` wins over the shell script of the same name without an extension |
| Output | one reader thread per session, bounded channel (64 × 64 KB), batching at 12 ms or 256 KB, sent as UTF-8 text over one Tauri channel per process launch | UTF-8 sequences that span chunk boundaries stay intact |
| Backpressure | the frontend acknowledges processed bytes (`ack_session`), the backend pauses above 2 MB of unacknowledged bytes; after more than 5 s without an acknowledgement the counter is reset | the UI does not freeze, and a reloaded frontend never blocks a session |
| Input | a dedicated writer thread per session, ordered through a channel | a child process that does not read never blocks the main thread |
| Process exit | after `wait()` the waiter thread closes the pseudo console; EOF follows, the remaining output is sent, then the exit event | otherwise ConPTY never closes the output |
| Process tree | Windows: Job Object with `KILL_ON_JOB_CLOSE`; Unix: `killpg` | no zombie agents, even if the app crashes |
| Permissions | commands registered via `AppManifest`, each one allowed individually in `capabilities/default.json` | the frontend may only call what is allowed |
| Frontend | one xterm.js instance per session outside React; all panes are stacked, inactive ones are merely hidden | scrollback and size survive switching |
| Keys | Ctrl+C copies when there is a selection and goes to the process otherwise; Ctrl+V and Ctrl+Shift+V paste via `term.paste` (bracketed paste); Ctrl+Tab switches the session | behaves like Windows Terminal |

## Consequences
- Measured quirk: at startup ConPTY queries the cursor position with `ESC[6n` and blocks until
  it gets a reply ([[conpty-cursor-query-measurement]]). The frontend therefore buffers input until the
  session ID has arrived, and the integration tests answer the query themselves.
- Open: restoring after a restart, a profile editor in the app, macOS (login shell PATH), Linux,
  CI matrix, code signing.
- The frontend bundle is about 590 KB, mostly because of xterm.js. For a desktop app that is
  not a concern.

## Evidence
- Code: `src-tauri/src/manager.rs`, `src-tauri/src/output.rs`, `src-tauri/src/proctree.rs`, `src/terminal.ts`
- Tests: `src-tauri/tests/pty.rs` (real ConPTY), unit tests in the modules, `src/*.test.ts`
