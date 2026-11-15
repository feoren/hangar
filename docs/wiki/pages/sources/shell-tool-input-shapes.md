---
title: "Reading: shell tool input shapes of Claude Code, Pi and OpenCode"
type: source
tags: [gate, hooks, shell, claude-code, pi, opencode, command-guard]
sources: []
raw: raw/2026-11-14-shell-tool-input-shapes.md
url:
retrieved: 2026-11-14
created: 2026-11-14
updated: 2026-11-14
---

# Reading: shell tool input shapes of Claude Code, Pi and OpenCode

A reading of installed files, nothing run: which field holds the command line when an agent
calls its shell tool. Record: `raw/2026-11-14-shell-tool-input-shapes.md`.

## Key points

- **Claude Code 2.1.319 to 2.1.323:** its transcripts on this machine hold `Bash` calls (136 510)
  and `PowerShell` calls (1 031), every one with `input.command` first. The hook's `tool_input` is
  the same object per Claude Code's hooks reference; the hook itself was not measured with a
  shell call.
- **Pi 1.5.2:** two shell tools, `bash` and `powershell`, built from one definition; both take
  `{ command, timeout }` (`dist/core/tools/bash.js` line 29, `powershell.js` line 29).
- **OpenCode 1.19.5:** the `bash` tool takes `command`, `timeout` and an optional `workdir`
  (relative paths resolve from the session's folder); the binary also knows PowerShell, pwsh and
  cmd as shells, chosen at run time on Windows. Its tool prompt tells the model to use `workdir`
  instead of `cd`.

## Significance for Hangar

- The command guard ([[0010-command-guard]]) reads `tool_input.command` from Claude Code's `Bash`
  and `PowerShell` hooks, Pi's `bash` (read as POSIX) and `powershell` (read as PowerShell), and
  OpenCode's `bash`, whose `workdir` becomes the working folder. On Windows the OpenCode
  extension sends the command as `Shell`, read in all three dialects, because the shell is not
  known in advance.
- Still not measured live: a shell call through each hook end to end.
