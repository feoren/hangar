---
title: "ADR 0003: Showing program versions"
type: decision
status: accepted
superseded_by:
tags: [agents, version, sidebar]
sources: []
created: 2026-11-07
updated: 2026-11-07
---

# ADR 0003: Showing program versions

## Context
Each installed profile should show its version, dimmed, next to its name, for example "PowerShell 7 · 7.6.9".
There is no uniform way to read the version of an arbitrary program:
- On Windows, npm agents are `.cmd` shims with no version information in the file.
- The Windows file version of `powershell.exe` is the Windows version, not PowerShell version 5.1.
- Almost all agent CLIs support `--version`. This is unverified per agent; see the research.

## Options
Not recorded separately; the reasons above rule out reading file metadata and leave calling the program.

## Decision
- **Query by calling the program:** the backend launches the resolved program with version arguments. No PTY, empty stdin,
  stdout and stderr redirected, working folder = a temporary folder, environment as for sessions (inherited
  agent markers removed), timeout 5 s, after which the process tree is killed. On Windows with
  `CREATE_NO_WINDOW`, so that no console windows flash up.
- **Arguments:** default `["--version"]`. Can be overridden per catalog entry and per profile through the optional
  field `version_args`. Built-in exceptions by program name: `powershell` →
  `["-NoProfile", "-Command", "$PSVersionTable.PSVersion.ToString()"]`, `cmd` → `["/d", "/c", "ver"]`.
- **Parsing:** first match of the pattern `\d+(\.\d+){1,3}` in stdout (otherwise stderr); otherwise no version.
  The exit code does not matter, only the text counts.
- **Separate from the list:** a new command `detect_versions` → `Record<profileId, string | null>`, async and
  parallel. `list_profiles` stays fast. Results are cached in memory, keyed by the
  resolved path plus the file's modification time and size. Updating an agent therefore triggers a new
  query automatically.
- **Display:** dimmed after the name in "New session" and in the menu File → New session, and in the tooltip
  of the running session; nothing while the query runs, nothing on error.
- **Setting** "Show program versions", on by default. Off means no program is launched.

## Consequences
- On first opening, installed programs are launched once with `--version`. Some CLIs check for updates
  or go online while doing so; that is why there is a timeout and a setting.
- A Job Object is only assigned after the launch. If the program starts a grandchild within the first microseconds,
  that grandchild can escape the tree. With `--version` that is unlikely.
- For `.cmd` shims, Rust's `std::process::Command` uses its hardened argument passing to `cmd.exe`. The
  arguments are fixed and come from the catalog or a profile, not from user input at run time.

## Implementation (2026-11-07)
- `src-tauri/src/version.rs`: parser (`parse_version`, hand written instead of a regex dependency), argument choice
  (`version_args`), query (`probe`) and parallel detection (`detect`, one thread per entry). An empty
  list `version_args = []` in a profile or the catalog means: never launch this program for a query.
- Query sequence: read output until EOF or the timeout, then **always** kill the process tree
  (Windows: Job Object from `proctree.rs`, Unix: a dedicated process group, SIGHUP to the group, SIGKILL to the
  child) and reap the child. After the kill it waits another 0.5 s for remaining output, so that a version
  written before the timeout still counts.
- Cache (`src-tauri/src/filecache.rs`): keyed by path, modification time, size and arguments.
  "No version" is cached too, so that a hanging program does not cost 5 s on every opening.
- Command `detect_versions` → `{ [profileId]: string | null }`, installed entries only.
- Measured on the development machine (Windows 11, debug build, 9 installed entries, npm shims among
  them): 3.1 s cold (dominated by the slowest CLI under load), under 1 ms warm.
- Program icons were added in the same step (`src-tauri/src/icon.rs`, command `program_icons`): on
  Windows the file's first icon via `PrivateExtractIconsW` at 32×32, as a PNG data URL; script shims
  and files without an icon of their own yield `null`. About 10 ms cold for 9 entries.

## Evidence
- Code and measurements: see Implementation. Per agent support for `--version`: unverified (see Context).
