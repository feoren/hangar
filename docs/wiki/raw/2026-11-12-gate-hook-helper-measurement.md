# Raw: hook helper start on Windows and Codex `-c` hook layer

Measured 2026-11-12 on Windows 11 Pro 10.0.26300, Codex CLI 0.167.2 (`codex --version`:
`codex-cli 0.167.2`), Windows PowerShell 5.1 (`powershell.exe`), PowerShell 7 (`pwsh.exe`),
`cmd.exe`, Python 3 as the driver. Gemini CLI, Copilot CLI and Qwen Code were not installed and
were not run. No model was called.

## 1. A GUI-subsystem program as a hook command

Probe: a Rust program built with `#![windows_subsystem = "windows"]` (as Hangar's release build),
that reads all of stdin, sleeps 1.5 s, prints `OUT len=<n> args=<argv> env=<HANGAR_GATE>` to
stdout and `ERR line` to stderr and exits with code 2. Started by Python `subprocess.run` with
piped stdin, stdout and stderr, `HANGAR_GATE=marker` in the environment and `CREATE_NO_WINDOW`,
the payload `{"tool_name":"write_file","tool_input":{"file_path":"a.txt"}}` (61 bytes) or
`{"tool_name":"apply_patch","tool_input":{"command":"*** Begin Patch"}}` (70 bytes) on stdin.

| How started | Exit | Took | stdout | stderr |
| --- | --- | --- | --- | --- |
| directly (CreateProcess, like Copilot's `exec`) | 2 | 1.88 s | `OUT len=61 args=["gate-hook", "gemini", "before-tool"] env=marker` | `ERR line` |
| `powershell.exe -NoProfile -NonInteractive -Command "& '<exe>' gate-hook gemini before-tool"` | 0 | 1.66 s | same as above | `ERR line` |
| `pwsh.exe` with the same arguments | 0 | 1.67 s | same | `ERR line` |
| `powershell.exe ... -Command "& '<exe>' ...; exit $LASTEXITCODE"` | 0 | 1.66 s | same | `ERR line` |
| `cmd.exe /C "<quoted exe> gate-hook codex pre-tool-use"` (raw command line) | 2 | 1.53 s | `OUT len=70 args=["gate-hook", "codex", "pre-tool-use"] env=marker` | `ERR line` |
| `cmd.exe /D /S /C "..."` (raw command line) | 2 | 1.53 s | same | `ERR line` |

Read: both shells wait for the GUI-subsystem program (the 1.5 s sleep is in every time), pass
stdin through, and return its stdout and stderr. cmd.exe returns the exit code 2. PowerShell
returns 0 in all three variants, so the program's exit code does not reach the caller through
`-Command`; `$LASTEXITCODE` is not set for it.

An earlier run that passed the cmd.exe command through Python's list quoting failed with
"Der Befehl ... ist entweder falsch geschrieben oder konnte nicht gefunden werden." because Python
escaped the inner quotes as `\"`; Codex builds the raw line itself, so only the raw rows count.

## 2. Codex `-c hooks.PreToolUse=[...]` registration

Driver: `codex app-server -c <value>` over stdio JSON-RPC, `initialize`, `initialized`, then
`hooks/list` with `cwds = [<repository folder>]`; printed are the hooks with `source` =
`sessionFlags`.

Value A (double quotes inside a TOML literal string, anchored matcher):

```text
hooks.PreToolUse=[{matcher="^apply_patch$",hooks=[{type="command",command='"C:\Program Files\Hangar\hangar.exe" gate-hook codex pre-tool-use',timeout=660}]}]
```

Started through `C:\Users\<user>\AppData\Roaming\npm\codex.CMD` (the first `codex` on PATH, npm
shim). Result:

```json
{"source": "sessionFlags", "eventName": "preToolUse", "matcher": "apply_patch$", "command": "\"C:\\Program Files\\Hangar\\hangar.exe\" gate-hook codex pre-tool-use", "timeoutSec": 660, "trustStatus": "untrusted", "enabled": true, "sourcePath": "C:\\<session-flags>\\config.toml"}
```

The `^` of the matcher is missing: cmd.exe consumed it on the way through the `.cmd` shim.

Value B (no double quote, unanchored matcher, path without spaces), through the same shim:

```text
hooks.PreToolUse=[{matcher='apply_patch',hooks=[{type='command',command='C:\Users\x\AppData\Local\Hangar\hangar.exe gate-hook codex pre-tool-use',timeout=660}]}]
```

```json
{"source": "sessionFlags", "eventName": "preToolUse", "matcher": "apply_patch", "command": "C:\\Users\\x\\AppData\\Local\\Hangar\\hangar.exe gate-hook codex pre-tool-use", "timeoutSec": 660, "trustStatus": "untrusted", "enabled": true, "sourcePath": "C:\\<session-flags>\\config.toml"}
```

Value A also passes `codex app-server -c <value> --print-startup-config` (exit 0,
`{"appServerVersion":"0.167.2",...}`). A malformed value is refused:
`-c 'hooks.PreToolUse=[{matcher=1}]'` gives `failed to load configuration: invalid type: integer
'1', expected a string in 'hooks'` (quotes adapted).

## 3. Codex flag placement (native `codex.exe` 0.167.2)

- `codex --dangerously-bypass-hook-trust -c <value B> resume --help`: prints the `resume` help,
  exit 0. Global options before a subcommand are accepted.
- `codex --dangerously-bypass-hook-trust --dangerously-bypass-hook-trust --help`:
  `error: the argument '--dangerously-bypass-hook-trust' cannot be used multiple times`.
- `codex resume --dangerously-bypass-hook-trust --help`: prints the `resume` help.

Not measured: whether a `-c` hook with `--dangerously-bypass-hook-trust` actually runs on an
`apply_patch` of a live session, whether `HANGAR_GATE` reaches the hook's environment, and what
the patch text looks like for each operation.
