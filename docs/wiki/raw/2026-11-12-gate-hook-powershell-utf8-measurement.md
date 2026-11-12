# Raw: `hangar gate-hook` through PowerShell, console build, UTF-8

Measured 2026-11-12 on Windows 11 Pro 10.0.26300 during the acceptance review, Windows PowerShell
5.1 (`powershell.exe`), PowerShell 7 (`pwsh.exe`), the debug (console-subsystem) build of
`hangar.exe` from `cargo test`, Python 3 as the driver. No agent CLI and no model were involved.

## 1. Non-ASCII hook input on stdin

Payload (UTF-8): `{"tool_name": "write_file", "cwd": "C:/r", "tool_input": {"file_path":
"src/ä.rs", "content": "Grüße, 日本 ✓"}}`, `HANGAR_GATE` unset, started as
`<shell> -NoProfile -NonInteractive -Command "& '<exe>' gate-hook gemini before-tool"` and directly.

| How started | Exit | stderr |
| --- | --- | --- |
| `powershell.exe` | 0 | `hangar gate-hook: HANGAR_GATE is missing or malformed; ...; the edit goes ahead` |
| `pwsh.exe` | 0 | same |
| directly | 0 | same |

Read: the input parsed as JSON in every case (the helper got as far as the missing variable, not
the "not JSON of the expected shape" error), so PowerShell passes UTF-8 stdin through unchanged.

## 2. Non-ASCII deny on stdout, exit code

A local Python HTTP server answered every POST with Claude Code's deny shape and the reason
`Hangar: Grüße ändert src/ä.rs, 日本 ✓`; `HANGAR_GATE=<port>:abc`; input
`{"tool_name": "write_file", "cwd": "C:/r", "tool_input": {"file_path": "src/a.rs"}}`. Helper at
that time: deny with exit 2.

| How started | Exit | stdout decoded as UTF-8 JSON, `reason` unchanged |
| --- | --- | --- |
| `powershell.exe -Command "& '<exe>' ..."` | 1 | yes |
| `pwsh.exe -Command "& '<exe>' ..."` | 1 | yes |
| directly | 2 | yes |

Read: stdout bytes come through PowerShell unchanged in UTF-8. PowerShell turns the console
program's exit code 2 into 1 (the GUI-subsystem probe of `2026-11-12-gate-hook-helper-measurement.md`
gave 0).
