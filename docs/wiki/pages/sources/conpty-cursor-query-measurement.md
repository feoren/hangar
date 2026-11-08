---
title: "Measurement: ConPTY blocks until ESC[6n is answered"
type: source
tags: [conpty, windows, pty, measurement]
sources: []
raw: raw/2026-11-07-conpty-cursor-query.md
url:
retrieved: 2026-11-07
created: 2026-11-07
updated: 2026-11-07
---

# Measurement: ConPTY blocks until ESC[6n is answered

Own measurement with portable-pty 0.9.0 on Windows 11 (build 26300).

## Key points
- A new ConPTY session first sends `ESC[6n` (cursor position query).
- Until a reply `ESC[<row>;<column>R` arrives, ConPTY passes on no further output, and the
  child process does not run to completion.
- Once the reply is sent, echo, exit code, resize, input, as well as exit and restart all work.

## Significance for Hangar
- xterm.js answers `ESC[6n` itself. The reply must not get lost, though, even if it
  arrives before the frontend knows the session ID. That is why `src/terminal.ts` buffers input until
  `setId`.
- Tests without a terminal emulator must answer the query themselves (`src-tauri/tests/pty.rs`).
- See [[0001-foundation]].

> [!question] Unverified
> The suspected cause is the `PSEUDOCONSOLE_INHERIT_CURSOR` flag in portable-pty. Not checked in the source code.
