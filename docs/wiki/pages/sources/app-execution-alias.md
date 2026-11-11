---
title: "App execution aliases of Store apps (reparse data and measurement)"
type: source
tags: [windows, store, icons, versions, updates]
sources: []
raw: raw/2026-11-12-app-execution-alias-measurement.md
url: https://github.com/libuv/libuv/blob/v1.x/src/win/fs.c
retrieved: 2026-11-12
created: 2026-11-12
updated: 2026-11-12
---

# App execution aliases of Store apps

Sources, read 2026-11-12: libuv `src/win/fs.c` (`fs__readlink_handle`) and `src/win/winapi.h`
(`AppExecLinkReparseBuffer`), the tag value in windows-sys 0.61.2
(`Win32::System::SystemServices::IO_REPARSE_TAG_APPEXECLINK = 0x8000001B`) and in PowerShell's
`FileSystemProvider.cs`. Plus our own measurement on the same day with PowerShell 7.7.0 from the
Microsoft Store (`%LOCALAPPDATA%\Microsoft\WindowsApps\pwsh.exe`).

## Key points
- An alias is an empty file (length 0) in `%LOCALAPPDATA%\Microsoft\WindowsApps` with a reparse
  point of tag `IO_REPARSE_TAG_APPEXECLINK`.
- Reparse data after the usual 8-byte header (`ReparseTag`, `ReparseDataLength`, `Reserved`):
  `StringCount: u32`, then NUL-separated UTF-16 strings. libuv: "String #3 in the list has the
  target filename" and requires it to be an absolute `X:\` path. Measured strings: package family
  name, application user model ID, target path, application type.
- Measured with Rust std on the alias: `metadata` works (length 0, `is_file` true, the alias's own
  time), `File::open` and `canonicalize` fail with OS error 1920 ("the system cannot access the
  file"), `read_link` fails with "Unsupported reparse point type".
- Starting the alias works: `std::process::Command` and a ConPTY session by its absolute path
  (tests `version_through_an_execution_alias`, `execution_alias_starts_in_a_pty`).
- The target (`C:\Program Files\WindowsApps\Microsoft.PowerShell_7.7.0.0_x64__8wekyb3d8bbwe\pwsh.exe`)
  can be read by the user: icon resources, size and time stamp come from there.
- `Get-AuthenticodeSignature` on the alias fails ("cannot access the file") and reports an empty
  status; on the target it reports `Valid`, Authenticode, timestamped.
- Store apps are updated by the Store (App Installer), not by winget or a setup program.

> [!question] Unverified
> Whether every Windows version since 1809 writes the fourth string, and whether the Store rewrites
> the alias on a package update (Hangar takes the stamp from the target, so it does not depend on it).

## Significance for Hangar
`src-tauri/src/exec_alias.rs` reads the alias; icons ([[0003-program-versions]]) and file stamps
use the target, update plans treat the copy as a Store install ([[program-updates]]).
