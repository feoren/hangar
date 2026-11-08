---
title: "Measurement: .cmd launcher scripts through portable-pty (node in the working folder, BatBadBut)"
type: source
tags: [windows, conpty, portable-pty, security, cmd, measurement]
sources: []
raw: raw/2026-11-08-cmd-launcher-measurement.md
url:
retrieved: 2026-11-08
created: 2026-11-08
updated: 2026-11-08
---

# Measurement: .cmd launcher scripts through portable-pty

Own measurement on Windows 11 (build 26300) with ConPTY and portable-pty 0.9.0, through the real
`SessionManager` before the fix. In addition, the portable-pty 0.9.0 code from the Cargo registry cache.

## Key points
- A `.cmd` launcher script that calls `node` without a path (like an npm shim without a `node.exe` next to it)
  runs a `node.cmd` from the working folder as long as `NoDefaultCurrentDirectoryInExePath` is not set.
- portable-pty quotes `.cmd` targets by the C runtime rules, not by those of cmd.exe:
  - `&` in an argument without spaces stays unquoted and separates commands (injection proven).
  - `"` in an argument becomes `\"`; cmd.exe does not understand that, and the quote state flips (injection proven).
  - `%VAR%` is expanded, even inside quotes.
  - `&` in the program path breaks the call.
- Spaces, parentheses in quoted parts, empty arguments and a working folder containing `&`
  work. The working folder is not on the command line.
- portable-pty does not accept a raw command line. Custom escaping by cmd.exe rules
  would be quoted a second time.

## Significance for Hangar
Protection and trade-offs: [[cmd-launcher]]. Raw data: `raw/2026-11-08-cmd-launcher-measurement.md`.
