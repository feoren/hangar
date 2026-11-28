---
title: "ADR 0022: Back up project folders with Hangar Drive"
type: decision
status: accepted
superseded_by:
tags: [hangar-drive, backup, url-scheme, registry, project-overview, session-menu]
sources: []
created: 2026-11-28
updated: 2026-11-28
---

# ADR 0022: Back up project folders with Hangar Drive

## Context
Hangar Drive is a separate sync and backup app by the same project, installed on its own. The
owner wants Hangar to offer backing up a project once Hangar Drive is on the machine: a click that
opens Hangar Drive with the project folder, and afterwards Hangar shows which project folders are
in Hangar Drive. Both apps stay separate programs; neither links the other's code. The contract
below was agreed with Hangar Drive while its side was still being built.

> [!warning] Hangar Drive's side is not built yet
> When this decision was written, Hangar Drive's link handler and its folder list did not exist.
> Hangar's side is coded against the contract and tested with fixtures only (see Evidence).

## Options
1. **Hangar adds the folder itself** (writes Hangar Drive's config). Rejected: it reaches into
   another app's data, and nobody would confirm what gets uploaded.
2. **A local socket or named pipe between the apps.** Two-way, but a second protocol, a running
   Hangar Drive and an authentication question, for one hand-over and one list.
3. **A link scheme for the hand-over, a public file for the list.** Hangar Drive registers
   `hangar-drive://` and asks the person to confirm every folder; it writes a small public list
   that Hangar only reads. No running process needed, nothing secret on either side.

## Decision
Option 3.

- **Detection** (`hangar_drive.rs`, `installed`, asked on every status call, no cache).
  Windows: Hangar Drive is a per-user install and registers its scheme at start as
  `HKCU\Software\Classes\hangar-drive\shell\open\command` = `"<exe>" "%1"`. Installed means the
  value exists, its program part (`command_program`: the quoted first part, or the first word) is
  an absolute path to an `.exe`, and that file exists. The value is read raw
  (`install_source::registry::raw_string_value`), since the trimming reader drops the quotes a
  path with spaces needs. Linux: `xdg-mime query default x-scheme-handler/hangar-drive` names a
  handler (2 s limit, `bounded_output`). macOS: never, Hangar Drive is not shipped there.
  Debug builds of Hangar Drive never own the scheme, so they never count.
- **Hand-over** (`hangar_drive_add_folder`, main window, focused, since it follows a click). The
  window passes the folder it knows; the backend checks it again: local and absolute, no control
  characters, no UNC or device path (`\\server`, `\\?\`, `\\.\`), no `..`, then canonicalized, an
  existing directory on a fixed local drive (`gate::local_fixed_drive`, so a mapped network drive
  or a removable one is refused), and given back without the `\\?\` that canonicalizing adds.
  Hangar Drive refuses UNC, device and relative paths too, so Hangar never offers them. The
  backend builds the link itself (`add_folder_link`): `hangar-drive://add-folder?path=<path>`,
  exactly one parameter, no fragment, every byte outside the RFC 3986 unreserved set
  percent-encoded (space as `%20`, never `+`). Only that function makes a `DriveLink`, the one
  non-web address `desktop::open_drive_link` opens: `ShellExecuteW` on Windows (as for web pages),
  the platform opener with the link as its one argument elsewhere. Hangar Drive always asks the
  person natively and shows its own notice when it is not set up, or when the folder is already in
  it or inside a synced folder; Hangar never adds a folder silently.
- **Folder list.** Hangar Drive's release build writes
  `<dirs::config_local_dir()>/Hangar Drive/public/folders.json` atomically (Windows
  `%LOCALAPPDATA%`, macOS `~/Library/Application Support`, Linux `$XDG_CONFIG_HOME` or
  `~/.config`): `{"contract":"drive-folders 1","appVersion":"…","updated":<ms>,"setUp":true,
  "folders":[{"path":"…","name":"…","state":"active|paused|stopped"}]}`. Its debug builds write
  under a `dev` folder, which Hangar never reads. Hangar reads at most 1 MiB and 4096 entries; a
  missing file, broken JSON, another contract or a missing list means no marks; an entry without
  a usable local absolute path is left out; an unknown state still marks the folder, without a
  state word (`other`). The list is read only while Hangar Drive counts as installed.
- **Matching** (`covers`, `mark`): a project folder is in Hangar Drive when it equals a listed
  folder or lies inside it, part by part (`C:\a\proj2` is not inside `C:\a\proj`), `/` and `\`
  alike and without case on Windows, a leading `\\?\` dropped. Nested listed folders: the
  innermost gives the state. The matching runs in the backend; the window gets one state or
  `null` per asked path (`hangar_drive_status`, at most 256 paths).
- **When the list is read.** When the project overview opens or its folders change, and whenever
  the main window comes back to the front (`focus`, `visibilitychange`; `useHangarDrive.ts`). No
  file watcher and no polling: the list only changes through Hangar Drive, whose confirmation
  takes the focus, so returning to Hangar is exactly when a new mark can appear. A watcher would
  need a thread on another app's folder (an atomic replace is a rename, so the folder, not the
  file, is watched) for no visible gain.
- **Where it shows** (only while Hangar Drive is installed; nothing is greyed out). The session
  menu (right click on a session in the sidebar, the rail or a pane header, which the owner calls
  the session tab) gets "Back up with Hangar Drive …" while the session's folder is not in Hangar
  Drive; it hands over the repository's main folder when git knows one (a session in a subfolder
  backs up the whole project, a worktree Hangar made its main folder), else the session's folder.
  Each project card in the project overview ends with one quiet line: "In Hangar Drive" (with
  "paused" or "stopped" where it does not sync), or the same "Back up with Hangar Drive …" as a
  text button. Update sessions and network folders get neither.

## Consequences
- Threat model: any web page or program can fire a `hangar-drive://add-folder` link, not only
  Hangar; the protection is Hangar Drive's own confirmation, not Hangar. Hangar only reads
  Hangar Drive's public file, never writes to it, and a broken or hostile file can at worst show or
  hide a mark: paths are only compared, never opened, and the size is capped.
- A second sibling app on the same contract would add its own scheme and file; nothing here is
  generic on purpose.
- When Hangar Drive changes the contract string, Hangar shows no marks until it learns the new
  one; the menu entry and the button still work, and Hangar Drive reports a folder it already
  has.
- macOS shows nothing of Hangar Drive until it ships there and this detection is extended.

## Evidence
- Unit tests in `src-tauri/src/hangar_drive.rs`: link encoding (spaces, umlauts, `#`, `&`, `%`,
  `?`, `+`, `=`), path checks (UNC, device, relative, `..`, control characters, length), the
  canonical form, real folders and files in a test folder, the folder list (unknown states, broken
  JSON, other contract, missing list, size limit, missing file), matching (case and separators on
  Windows, case on Unix, the sibling `proj2`, nested folders) and the registry command as a pure
  function of its text. Frontend: `src/hangarDrive.test.ts`, `src/sessionMenu.test.ts`,
  `src/ProjectOverview.test.tsx`.
- Seen on the owner's Windows 11 machine on 2026-11-28 (read only): the registry value
  `"<LOCALAPPDATA>\Hangar Drive\hangar-drive.exe" "%1"` exists in the documented form and the
  program file exists; there is a `dev` folder but no `public/folders.json` yet. A one-off run of
  `installed()` and `status()` there (debug build of Hangar, not kept as a test, since it depends
  on the machine) answered installed with no marks.

> [!question] Unverified
> Not verified live: Hangar Drive's handling of the link (its confirmation, its notices), its
> `folders.json` (not written yet), `ShellExecuteW` handing a custom-scheme link to the registered
> handler from Hangar, `xdg-mime` and `xdg-open` with the scheme on Linux, that a mapped network
> drive canonicalizes to `\\?\UNC\…`, and that `focus` fires in WebView2 and WebKitGTK when the
> window returns from another app.
