# Measurement: .cmd launcher scripts via portable-pty (2026-11-08)

Platform: Windows 11 Pro, build 26300, ConPTY. Rust 1.99.1, portable-pty 0.9.0 (Cargo.lock).
Setup: real `SessionManager` from `src-tauri/tests/pty.rs`, profile `command` = absolute path to
a test script, run before the fix (as of commit 1797c11).

## 1. node from the working folder

`bin\agent.cmd`:

```
@echo off
node --hangar-probe 2>nul
echo HANGAR-STARTER-ENDE
```

`work\node.cmd` (working folder of the session):

```
@echo HANGAR-FREMDES-NODE
@type nul > node-lief.txt
```

`NoDefaultCurrentDirectoryInExePath` removed from the test environment beforehand.
Output of the session (test `cmd_starter_sucht_node_nicht_im_arbeitsordner` (renamed to `cmd_launcher_does_not_search_node_in_working_folder` on 2026-11-08), before the fix):

```
\u{1b}[6n\u{1b}[?9001h\u{1b}[?1004h\u{1b}[m\u{1b}]0;C:\WINDOWS\system32\cmd.exe\u{7}\u{1b}[?25h\u{1b}[?9001l\u{1b}[?1004lHANGAR-FREMDES-NODE\r\n
```

The foreign `node.cmd` ran. `HANGAR-STARTER-ENDE` is missing because a `.cmd` that calls another one
without `CALL` does not return.

## 2. Special characters in path and arguments

`bin\echoargs.cmd` and a copy under `bin&x\echoargs.cmd`:

```
@echo off
echo HANGAR-ARGS=[%*]
```

| Profile args | Output (shortened to the text after the VT sequences) | Exit |
|---|---|---|
| `["x&echo.HANGAR-INJIZIERT"]` | `HANGAR-ARGS=[x]\r\nHANGAR-INJIZIERT\r\n` | 0 |
| `["x&echo HANGAR-INJIZIERT"]` (with space) | `HANGAR-ARGS=["x&echo HANGAR-INJIZIERT"]\r\n` | 0 |
| `["a \"b&echo HANGAR-INJIZIERT2\" c"]` | `HANGAR-ARGS=["a \"b]\r\nHANGAR-INJIZIERT2\" c"\r\n` | 0 |
| `["%OS%"]` | `HANGAR-ARGS=[Windows_NT]\r\n` | 0 |
| none, program `bin&x\echoargs.cmd` | `Der Befehl "C:\Users\…\hangar-cmd-messung-…\bin" ist entweder falsch geschrieben oder konnte nicht gefunden werden.` | 1 |
| `["fix (this) now", "--model=x", ""]`, working folder `wo&rk` | `HANGAR-ARGS=["fix (this) now" --model=x ""]\r\n` | 0 |

## 3. Code of portable-pty 0.9.0

`src/cmdbuilder.rs`, `CommandBuilder::cmdline` and `append_quoted` (Windows): argv[0] and every
argument are quoted only if they contain a space, tab, `\n`, `\v` or `"`;
`"` is written as `\"`, with preceding backslashes doubled (C runtime rules after
ArgvQuote). There is no special case for `.bat`/`.cmd` and no way to pass a raw
command line. `src/win/psuedocon.rs`, `spawn_command`: `CreateProcessW` with the
program path as `lpApplicationName` and this line as `lpCommandLine`.
