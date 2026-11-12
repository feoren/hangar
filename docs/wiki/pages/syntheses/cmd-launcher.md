---
title: Launching .cmd/.bat launcher scripts safely (Windows)
type: synthesis
tags: [windows, security, portable-pty, cmd, sessions]
sources: ["[[cmd-launcher-measurement]]"]
created: 2026-11-08
updated: 2026-11-12
---

# Launching .cmd/.bat launcher scripts safely (Windows)

Code: `src-tauri/src/batch.rs`, called from `spawn` in `src-tauri/src/manager.rs`. Tests:
`batch.rs` (unit) and `cmd_starter_*` in `src-tauri/tests/pty.rs` (real ConPTY).

On Windows, many agents ship as an npm shim (`codex.cmd`, `gemini.cmd` …). CreateProcessW
runs such a target through cmd.exe, which opens two gaps, both measured
([[cmd-launcher-measurement]]).

## 1. Program lookup in the working folder

**Rule:** If the resolved program is a `.cmd` or `.bat`, Hangar sets
`NoDefaultCurrentDirectoryInExePath=1`, as it already does for update sessions ([[program-updates]]).
Shells (`cmd.exe`, `pwsh.exe`, `powershell.exe`, `wsl.exe`) are `.exe` files and do not get the variable.

**Criterion ".cmd/.bat target" rather than "profile from the catalog"**, because:
- The gap comes from the mechanism (cmd.exe looks for `node` in the current folder first), not from
  where the profile came from. A user profile with its own `.cmd` wrapper is just as exposed, and a
  catalog agent installed as an `.exe` (e.g. `claude.exe`) is not.
- Shells drop out on their own, without a list of names.
- A catalog criterion would treat profiles with the same `id` from `profiles.toml` differently
  depending on how they were merged; the target criterion is unambiguous.

**Inheritance:** The agent (node) inherits the variable, and so does everything it starts, including
shells for tool calls. Preventing that would take a launcher of our own between cmd.exe and node,
and it is intended anyway: when the agent runs `npm test` in a foreign repo, the same gap opens
again. Consequence for users: in such a child cmd.exe, `tool` no longer finds a `tool.exe` in the
current folder, while `.\tool` still works.

**Opting out:** An empty value in the profile `env` (`NoDefaultCurrentDirectoryInExePath = ""`,
any casing) removes the variable for that profile, even if Hangar inherited it. Reason: only the
presence of the variable counts, not its value (the counter check in the PTY test proves the
removal; whether `X=` with an empty value already counts as set has not been measured).

> [!question] Unverified
> PowerShell never searches the current folder anyway (vendor docs not taken into the wiki).
> Whether `CreateProcessW` itself (without cmd.exe) honors the variable when an agent starts a
> program without a path. Only the cmd.exe lookup has been measured.

## 2. Quoting for cmd.exe (BatBadBut)

portable-pty 0.9.0 quotes only by the C runtime rules and does not accept a raw command line
([[cmd-launcher-measurement]]). The escaping in Rust std since 1.77.2 (its own line
`cmd.exe /e:ON /v:OFF /d /c "…"`, `%` as `%%cd:~,%`) therefore cannot be reproduced without
forking portable-pty or taking over spawning ourselves.

**Decision: reject instead of escaping.** Before launch, `batch::check` inspects the program path
and arguments of a `.cmd`/`.bat` target and returns `ManagerError::UnsafeBatch` before a PTY is
opened. The message names the position and the character, never the value (an argument can be a token).

| Character | rejected | Reason |
|---|---|---|
| `% !` | always | cmd.exe expands variables even inside quotes (`!` with delayed expansion from the registry) |
| `"` | always | portable-pty writes `\"`, which cmd.exe does not understand, so the quote state flips |
| `& \| < > ^` | always | without a space, part of the argument stays unquoted; in the path they take effect despite quotes, because npm shims run `SET dp0=%~dp0` unquoted |
| `( )` | only in parts without spaces | inside a `( … )` block, a `)` from `%*` ends the block; quoted it is harmless, so `C:\Program Files (x86)\…` stays allowed |
| control characters | always | line breaks cut the line short |

Still allowed include spaces, `@ / \ : = , + # ~ $ * ?`, umlauts and empty arguments. The
working folder is not on the command line; a folder containing `&` works (measured).

Why rejecting is enough: arguments come from the profile and the catalog, not from untrusted input,
and no catalog entry contains any of these characters. The danger would be a PATH folder or a profile
with such characters; that now fails clearly instead of going wrong silently.

> [!question] Unverified
> Hangar does not check how the launched script passes its arguments on. A script that uses
> `%~1` (without quotes) instead of `%*` may also interpret allowed characters such as spaces
> differently. The same holds for the escaping in Rust std.

## Not affected
- Version queries (`src-tauri/src/version.rs`) start through `std::process::Command` in Hangar's
  own `version-probe` folder in the app data folder, not the project and not the temp folder, and
  always with `NoDefaultCurrentDirectoryInExePath=1` (same rule and opt-out as sessions,
  `src-tauri/src/child_env.rs`). Test: `version_query_ignores_a_node_in_its_working_folder`
  (a planted `node.cmd` in that folder does not run).

> [!question] Unverified
> That `std::process::Command` has escaped `.cmd` arguments by cmd.exe rules since Rust 1.77.2
> (CVE-2024-24576) has neither been measured here nor taken from the source.
- macOS and Linux: `batch::is_batch` is always false there.
