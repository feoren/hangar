---
title: "ADR 0024: Back up project folders with Hangar Drive"
type: decision
status: accepted
superseded_by:
tags: [hangar-drive, backup, url-scheme, registry, project-overview, session-menu]
sources: []
created: 2026-11-28
updated: 2026-11-28
---

# ADR 0024: Back up project folders with Hangar Drive

## Context
Hangar Drive is a separate sync and backup app by the same project, installed on its own. The
owner wants Hangar to offer backing up a project once Hangar Drive is on the machine: a click that
opens Hangar Drive with the project folder, and afterwards Hangar shows which project folders are
in Hangar Drive. Both apps stay separate programs; neither links the other's code. The contract
below was agreed with Hangar Drive while its side was still being built.

> [!note] Hangar Drive's side
> Hangar Drive 0.1.0 had neither the link handler nor the folder list and drops add-folder links
> silently. Hangar Drive 0.2.0 has both, and its security review added the refusals listed under
> "Hand-over", which Hangar mirrors so that it never offers a folder Hangar Drive would refuse.

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
- **Ready** (`ready_list`). The scheme alone is not enough: Hangar Drive 0.1.0 owns it but drops
  add-folder links silently. Hangar offers Hangar Drive only when it is installed (above) and its
  folder list (below) exists, carries the contract `drive-folders 1` and says `"setUp": true`.
  Hangar Drive 0.2.0 and later writes that file on every start (`setUp: false` with an empty list
  before it is set up) and never deletes it, so an uninstall can leave it behind; that is why the
  scheme stays a precondition. A missing file, `setUp: false` or missing, another contract or
  broken JSON: no menu entry, no button, no marks.
- **Hand-over** (`hangar_drive_add_folder`, main window, focused, since it follows a click). The
  window passes the folder it knows; the backend checks it again: local and absolute, no control
  characters, no UNC or device path (`\\server`, `\\?\`, `\\.\`), no `..`, then canonicalized, an
  existing directory on a fixed local drive (`gate::local_fixed_drive`, so a mapped network drive
  or a removable one is refused), and given back without the `\\?\` that canonicalizing adds.
  Hangar Drive refuses UNC, device and relative paths too, so Hangar never offers them. The
  backend builds the link itself (`add_folder_link`): `hangar-drive://add-folder?path=<path>`,
  exactly one parameter, no fragment, every byte outside the RFC 3986 unreserved set
  percent-encoded (space as `%20`, never `+`), at most 12,320 characters for a path of at most
  4096 bytes. Hangar Drive refuses a malformed link silently (it only logs), so Hangar checks
  everything it refuses first; on Windows that includes reserved device names (`CON`, `PRN`,
  `AUX`, `NUL`, `COM1` to `COM9`, `LPT1` to `LPT9`, also with an extension) and parts ending in a
  dot or a space, both in the window's path and in the canonical one. Hangar Drive's security
  review (its `handover.rs`, `guarded`, applied to the canonical path) refuses three more kinds of
  folder, and Hangar checks them too (`plain_folder`, `guarded`): any part starting with a dot at
  any level (`.git`, `.ssh`, `.config`); a folder directly in the home folder
  (`C:\Users\a\Documents`; home is `dirs::home_dir()` canonicalized, compared without case and with
  `/` as `\` on Windows, with case elsewhere), where Hangar also refuses the home folder itself,
  while `C:\Users\a\Documents\proj` is fine; and on Windows a folder carrying
  `FILE_ATTRIBUTE_HIDDEN` or `FILE_ATTRIBUTE_SYSTEM`, read with `GetFileAttributesW`, on the
  folder itself and on each parent strictly below the home folder (`C:\Users\a\AppData\Local\x`
  is refused for the hidden `AppData`; `D:\Data\x` is checked on `x` only), where attributes that
  cannot be read count as set. The window mirrors the pure rules in `isLocalFolder` (the status
  call hands it the canonical home folder) and gets the backend's full verdict per asked path
  (`refused`), so neither the menu entry nor the button appears for such a folder. The session
  menu knows the folder it would hand over before it shows the entry: the repository main folders
  of the sessions' folders are read when the folders change (`useRepoRoots`, `git_repo_places`,
  no process), the status call asks about those folders (`handOverFolder`), and the entry stays
  hidden while git has not answered. A session in a subfolder of a project Hangar Drive refuses
  (one directly in the home folder, say) therefore gets no entry, never one for the subfolder,
  which would back up less than the person asked for. The link fires once per click on the menu entry or the
  button, never on its own (not on render, focus or a status answer). Hangar Drive keeps one
  proposal open and ignores new ones for 30 s after an outcome without an add, so the backend
  hands the same folder over at most once per 30 s (`Proposals`, in memory; a link that did not
  open is forgotten, so the next click retries). Hangar Drive's own pauses grow after repeated
  proposals without an add (30 s, 2 min, 10 min, then ignored until it restarts); that is Hangar
  Drive's business, and Hangar does not mirror it. Only that function makes a `DriveLink`, the one
  non-web address `desktop::open_drive_link` opens: `ShellExecuteW` on Windows (as for web pages),
  the platform opener with the link as its one argument elsewhere. Hangar Drive always asks the
  person natively and shows its own notice when it is not set up, or when the folder is already in
  it or inside a synced folder; Hangar never adds a folder silently.
- **Folder list.** Hangar Drive's release build writes
  `<dirs::data_local_dir()>/Hangar Drive/public/folders.json` atomically (Windows
  `%LOCALAPPDATA%`, macOS `~/Library/Application Support`, Linux `$XDG_DATA_HOME` or
  `~/.local/share`): `{"contract":"drive-folders 1","appVersion":"…","updated":<ms>,"setUp":true,
  "folders":[{"path":"…","name":"…","state":"active|paused|stopped"}]}`. Its debug builds write
  under a `dev` folder, which Hangar never reads. Hangar reads at most 1 MiB and 4096 entries; a
  missing file, broken JSON, another contract or a missing list means no marks; an entry without
  a usable local absolute path is left out; an unknown state still marks the folder, without a
  state word (`other`). `setUp: false` comes with an empty list; `updated` is the time of the
  last change, not a heartbeat, so an old value never makes the list count as stale. The list is
  read only while Hangar Drive counts as installed, and a list without `setUp: true` gives no
  marks either.
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
- **Where it shows** (only while Hangar Drive is ready; nothing is greyed out). The session
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
  hide a mark or the offer: paths are only compared, never opened, and the size is capped.
- `folders.json` is information only, never a permission. Any process of the same user can write
  or forge it, so it decides only what Hangar shows (marks, whether to offer the hand-over); it
  never lets Hangar skip a path check, and Hangar Drive confirms and checks every folder itself.
- The link's value stays decodable by any `encodeURIComponent`-compatible decoder: every byte
  outside the unreserved set is percent-encoded, a space as `%20` and `+` as `%2B`.
- A second sibling app on the same contract would add its own scheme and file; nothing here is
  generic on purpose.
- When Hangar Drive changes the contract string, Hangar shows nothing of it (no marks, no menu
  entry, no button) until it learns the new one.
- When Hangar Drive adds a refusal, Hangar may offer a folder Hangar Drive then refuses with its
  own notice; the rules here have to follow Hangar Drive's `guarded`.
- macOS shows nothing of Hangar Drive until it ships there and this detection is extended.

## Evidence
- Unit tests in `src-tauri/src/hangar_drive.rs`: link encoding (spaces, umlauts, `#`, `&`, `%`,
  `?`, `+`, `=`; one parameter, no fragment, the link length limit), path checks (UNC, device,
  relative, `..`, control characters, length, reserved device names, trailing dots and spaces,
  parts starting with a dot, the home folder and its direct children, hidden and system bits as a
  pure function of the attribute bits, the folders whose attributes count, a real hidden folder
  under `tests/.tmp` on Windows), the 30 s cooldown per folder, the canonical form, real folders
  and files in a test folder, the folder list (unknown states, broken JSON, other contract,
  missing list, `setUp` false, missing or not a boolean, size limit, missing file), matching (case
  and separators on Windows, case on Unix, the sibling `proj2`, nested folders) and the registry
  command as a pure function of its text. Frontend: `src/hangarDrive.test.ts` (the mirrored
  rules, the backend's `refused`, the folder the menu entry would hand over, no entry for a
  subfolder of a project directly in the home folder), `src/sessionMenu.test.ts`, `src/ProjectOverview.test.tsx` (no
  button while not ready or refused), `src/DriveLine.test.tsx` (one hand-over per click, none on
  render).
- Seen on the owner's Windows 11 machine on 2026-11-28 (read only): the registry value
  `"<LOCALAPPDATA>\Hangar Drive\hangar-drive.exe" "%1"` exists in the documented form and the
  program file exists; there is a `dev` folder but no `public/folders.json` yet. A one-off run of
  `installed()` and `status()` there (debug build of Hangar, not kept as a test, since it depends
  on the machine) answered installed with no marks.
- Seen on the same machine later on 2026-11-28 with Hangar Drive 0.2.0 (read only, link never
  opened): `<LOCALAPPDATA>\Hangar Drive\public\folders.json` exists with
  `"contract":"drive-folders 1"`, `"appVersion":"0.2.0"`, `"setUp":true` and six active folders.
  A one-off run of `status()` (debug build of Hangar, not kept as a test) answered ready with the
  canonical home folder; a listed folder was marked active, a project folder on another drive was
  neither marked nor refused, and `<home>`, `<home>\Documents` and `<home>\AppData\Local\Temp`
  (hidden `AppData`) were refused.

> [!question] Unverified
> Not verified live: Hangar Drive's handling of the link (its confirmation, its notices, its
> growing pauses), `ShellExecuteW` handing a custom-scheme link to the registered
> handler from Hangar, `xdg-mime` and `xdg-open` with the scheme on Linux, that a mapped network
> drive canonicalizes to `\\?\UNC\…`, and that `focus` fires in WebView2 and WebKitGTK when the
> window returns from another app.
