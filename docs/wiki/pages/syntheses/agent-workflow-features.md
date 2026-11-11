---
title: "Agent workflow features (0.3.0)"
type: synthesis
status: accepted
tags: [notifications, search, split-view, launcher, git, worktree, snippets, themes, profiles, secrets, tray]
sources: []
created: 2026-11-08
updated: 2026-11-11
---

# Agent workflow features (0.3.0)

Specification and contracts for the 0.3.0 feature batch. Backend and frontend are built in
parallel against the contracts below; whoever changes a contract updates this page first.

## Principles

- The backend owns processes, files, git, the clipboard, the keychain, the tray and
  notifications. The frontend renders and calls approved commands only (capabilities).
- Every external program (git, wsl.exe) starts as an argv array, never through a shell, with a
  timeout and, on Windows, `CREATE_NO_WINDOW`.
- Pure logic lives in small modules with tests (`src/*.ts` with Vitest, Rust unit tests).
  Components and command handlers only wire.
- Every UI text in `src/i18n.ts` in all 12 languages; every new domain term in `glossary.md`;
  every user-visible feature gets a `src/changelog.json` item for 0.3.0 in all 12 languages.
- Shortcuts only Ctrl+Shift+letter (or Ctrl+Tab etc.), central in `src/shortcuts.ts`, with tests.
- Fewer controls, clearer texts. Defaults must be good without opening Settings.

## Backend contracts (Tauri commands, camelCase JSON)

Each command is added to `COMMANDS` in `src-tauri/build.rs` and granted in
`src-tauri/capabilities/default.json`.

### Clipboard
Official plugin `tauri-plugin-clipboard-manager`; capabilities `clipboard-manager:allow-read-text`
and `clipboard-manager:allow-write-text`. Used by the menu items Copy and Paste, which today rely
on `navigator.clipboard` and fail in WebView2.

Pasting a screenshot (as built, `src/clipboardPaste.ts`, `src-tauri/src/paste_image.rs`,
`src-tauri/src/clipboard_image.rs`): when a paste carries no text, `paste_clipboard_image` reads
the clipboard's image, saves it as a PNG under `app_cache_dir()/pastes` (removed after 7 days, at
most 40 megapixels; on Unix the folder is 0700 and each file 0600) and answers
`{kind: "saved", path} | {kind: "noImage"} | {kind: "busy"}`; the window pastes the quoted path
like a dropped file. Text on the clipboard always wins over an image.

- The clipboard image is untrusted input. arboard 3.7.0 decodes the Windows "PNG" format with
  `image`'s `Limits::no_limits()` and allocates the pixel buffer from the header (read in
  `src/platform/windows.rs`), so a few kilobytes claiming 65535x65535 pixels abort the app. On
  Windows Hangar therefore reads "PNG", then `CF_DIBV5`, then `CF_DIB` itself through
  `clipboard-win` (raw data at most 4 bytes per allowed pixel plus 1 MiB), checks width and
  height from the IHDR chunk or the bitmap header before decoding, and decodes PNG with
  `png::Limits` and 24 or 32 bit bitmaps by hand; palettes, 16 bit and compressed bitmaps are
  refused.
- On macOS and Linux arboard decodes through `image::ImageReader`, whose default `Limits` cap
  each allocation at 512 MiB (image 0.25.10, `src/io/limits.rs`); `arboard` is a direct
  dependency there because the plugin's `read_image` maps `ContentNotAvailable` to an untyped
  string error.

  > [!question] Unverified
  > Not tried on a real X11, Wayland or macOS clipboard yet, and the raw transfer from the other
  > program has no limit of Hangar's own there.
- One paste at a time: the backend answers `busy` while one runs (`PasteGate`), and each terminal
  drops repeats while its own paste waits (`imagePasteInFlight`). A held Ctrl+V would otherwise
  decode a whole screen many times in parallel.
- Known limit of the quoting (also for drag and drop): it follows the program the profile starts,
  not a program started inside the session, such as bash inside a cmd session.

### Notifications
- `notify_session(id: number, title: string, body: string) -> boolean`: shows a desktop
  notification; `false` when the platform has no support (macOS and Linux for now).
  Windows: the WinRT toast API through the `windows` crate (`tauri-winrt-notification` cannot
  set a toast tag, [[windows-toast-notifications]]); app id = bundle identifier, the PowerShell
  app id for executables under `target\debug` or `target\release`. One notification
  per session at a time (tag or replace by session id). Title and body are cut to 200 characters.
- Click on the notification: the backend shows, unminimizes and focuses the main window and emits
  the event `notification-activated` with `{ sessionId: number }`.
- `close_session` also withdraws the session's toast from the notification center
  (`ToastNotificationHistory.RemoveGroupedTagWithId`) and drops its click handler.
- Titles and bodies lose control characters and invisible characters; U+FFFE and U+FFFF become
  U+FFFD, U+2028 and U+2029 a space. "Invisible" is one table shared with Export
  (`src-tauri/src/text.rs`): every character of Unicode category Cf (bidi controls, zero-width
  characters, U+FEFF, U+00AD, U+061C, U+180E, U+0600 to U+0605, U+FFF9 to U+FFFB, tags U+E0000 to
  U+E007F, ...) plus the default-ignorable characters of other categories (U+034F, variation
  selectors U+FE00 to U+FE0F and U+E0100 to U+E01EF, Hangul fillers, U+17B4, U+17B5).

### Git
All git calls: `git` from PATH, argv only, `GIT_OPTIONAL_LOCKS=0`, `GIT_TERMINAL_PROMPT=0`,
`-c core.fsmonitor=false`, timeout 10 s, no pager, no external diff or textconv. Every inherited
`GIT_*` variable is removed from git's environment before Hangar sets its own. If the process tree
cannot be put into a Job Object, git is killed and the call fails. `status` and `diff` run without
the repository's filters: Hangar first reads `git config -z --get-regexp
"^filter\..*\.(clean|smudge|process)$"` and passes `-c filter.<name>.clean= -c
filter.<name>.smudge= -c filter.<name>.process= -c filter.<name>.required=false` per driver
(a driver name with `=` or more than 64 drivers refuse the call), plus `--ignore-submodules=all`.
A process that ends before it could join the Job Object is not an error (its output is used).
Before git runs at all (`git_diff`, `create_worktree`, `remove_worktree`), the folder must be
absolute and existing, and the `.git` file found walking up from it (and the `commondir` file of
the git folder it names) must not lead from a local folder to a UNC share, `\\?\UNC\`, `\\.\` or
another `\\?\` namespace (same rule as `git_branches`); otherwise the call fails with "The
repository's git folder lies on another network share or device". Only the paths are compared.
Not covered, because git reads them itself: `[include]` and `[includeIf]` paths,
`objects/info/alternates`, `core.worktree`, `core.excludesFile`, `core.attributesFile` and other
paths in the repository's config, and `.git` entries that are symlinks or junctions to a share.
- `git_branches(paths: string[]) -> Record<string, string | null>`: branch per folder. Reads
  `HEAD` from the `.git` directory or follows a `.git` file (`gitdir: ...`, worktrees,
  submodules) walking up from the folder; no process. `ref: refs/heads/x` gives `x`, a detached
  HEAD the first 7 hex characters; no repository gives `null`. At most 64 paths per call. A
  `.git` file in a local folder whose `gitdir:` leads to a UNC share, `\\?\UNC\`, `\\.\` or
  another `\\?\` namespace is not followed (`null`): reading HEAD every 15 s would send the
  user's NTLM credentials to that host. A folder on a share may point into the same share.
- `git_diff(cwd: string) -> GitDiff`:
  `{ isRepo: boolean, files: { path: string, status: string, additions: number, deletions: number }[], diff: string, truncated: boolean }`.
  `status` is the porcelain v1 XY code. `diff` is `git diff HEAD --no-color --no-ext-diff
  --no-textconv` (repositories without commits: `git diff --cached` plus working tree), at most
  2 MiB (`truncated` then true). Untracked files appear in `files` with status `??` and no
  content.
- `create_worktree(cwd: string, label: string) -> Worktree`:
  `{ path: string, branch: string, repoRoot: string }`. Finds the repository root of `cwd`, creates
  branch `hangar/<slug>` from `HEAD` and the worktree under
  `<app local data>/worktrees/<repo name>-<8 hex of sha256(repoRoot)>/<slug>`. `slug` comes from
  `label` (lowercase ASCII letters, digits, `-`, at most 40) plus a short random suffix. The app
  data folder keeps repositories and synced folders clean.
- `remove_worktree(worktree: Worktree) -> { removed: boolean, branchDeleted: boolean, reason: string | null }`:
  first `git status --porcelain=v1 -z --ignored=matching` in the worktree (filters off as above):
  with ignored files (`!!`) nothing is removed and `reason` is "The worktree contains ignored
  files: a, b, c and N more", because `git worktree remove` deletes ignored files even without
  `--force`. Otherwise `git worktree remove <path>` without `--force` (a worktree with changes
  stays, `reason` says why); `removed` is true only when the folder is gone afterwards.
  `worktree remove` starts a `git status` of its own, which would run clean filters on files
  whose stat data changed: it gets the same filter overrides (read in the worktree, so a
  `config.worktree` filter counts too), which reach that child through `GIT_CONFIG_PARAMETERS`.
  A worktree with populated submodules is refused by git without `--force` (measured, git
  2.56.0.windows.4), so submodule filters do not come into play. Then
  `git branch -d <branch>` (an unmerged branch stays). `worktree remove`, `worktree prune` and
  `branch -d` run with `-c core.hooksPath=<app local data>/worktrees/.no-hooks` (an empty
  folder), so no repository hook runs; `worktree add` keeps the hooks. Refuses any path outside
  the app's worktrees folder, and any record whose path does not end in
  `<repo dir of repoRoot>/<slug of branch>` (so `repoRoot` cannot be swapped for another
  repository). `worktree add` and `worktree remove` get 120 s instead of 10 s, because killing a
  checkout halfway leaves a broken worktree.

### Workspace
`SavedSession` gains three optional fields (absent or `null` in old files):
`color: string | null` (`#rrggbb`), `emoji: string | null` (at most 16 bytes UTF-8), and
`worktree: Worktree | null`. The backend validates them; a worktree must have absolute `path` and
`repoRoot`, branch `hangar/<slug>` and the path shape `remove_worktree` checks.

### Taskbar badge
Tauri feature `image-png`; capabilities `core:window:allow-set-overlay-icon` (Windows) and
`core:window:allow-set-badge-count` (macOS, Linux). The frontend draws the number.

### Profiles in Settings
- `save_profile(profile: ProfileInput) -> Profile[]`: inserts or replaces a `[[profile]]` in
  `profiles.toml` by id, keeps comments, layout, order, other entries and fields the form does not
  know (`icon`, `latest`, ...) (`toml_edit`); an unchanged value keeps its exact spelling, an
  inline `env = { ... }` stays inline. Empty `args`, `cwd`, `env`, `secretEnv` are removed from
  the entry. Validates like loading, writes atomically, reloads and returns the new list.
  `ProfileInput`:
  `{ id, name, command, args: string[], cwd: string | null, env: Record<string, string>, secretEnv: string[] }`.
  The id must not be empty or contain whitespace or `/`, and must not differ only in case from
  another entry (profile, catalog, WSL): the Windows Credential Manager ignores case, so `Work`
  and `work` would share and delete each other's secrets. Loading refuses two such ids in
  `profiles.toml` too. `cwd` must be an absolute path (empty or `null` means none). A catalog or
  WSL id in the same spelling creates a user profile that replaces that entry. Names dropped from
  `secretEnv` lose their keychain entries.
- `delete_profile(id: string) -> Profile[]`: only user profiles (error otherwise; the catalog
  entry of the same id shows up again); the last `[[profile]]` of the file cannot be deleted.
  The profile's keychain entries are deleted too.
- `Profile` (list view) gains `editable: boolean` (user profile) and `env: string[]` (names only,
  never values) and `secretEnv: string[]`. Values of `env` are returned by
  `profile_env(id) -> Record<string, string>` only for the edit form (empty for catalog and WSL
  entries, error for an unknown id).
- `profiles.toml` gains `secret_env = ["NAME", ...]` per profile: names `[A-Za-z_][A-Za-z0-9_]*`,
  each once, never also in `env`. `env` names must not be empty or contain `=` or NUL. Both
  apply when loading too.

### Secrets (OS keychain)
Crate `keyring` (Windows Credential Manager, macOS Keychain, Linux Secret Service). Service
`hangar`, account `<profile id>/<VARIABLE>`.
- `set_profile_secret(profileId, name, value) -> null`, `delete_profile_secret(profileId, name) -> null`,
  `profile_secrets(profileId) -> { name: string, stored: boolean }[]`. Values never leave the
  backend and never reach logs or error texts.
- On session start (also restart and the normal start after a failed resume), each name in the
  profile's `secret_env` is read from the keychain and set as an environment variable of the
  child process. A missing secret is skipped; a failing keychain is logged with the name only.
  Update sessions get no secrets.
- `set_profile_secret` only accepts a name listed in the `secret_env` of an existing profile
  (save the profile first; otherwise "Invalid profile id" or "Invalid variable name"). It rejects
  an empty value, a value with NUL and values over 1280 UTF-16 units (Windows Credential Manager
  stores at most 2560 bytes); errors never contain the value. Profile id and name follow the
  rules above. `profile_secrets` lists the names of the profile's `secret_env`; an unknown id is
  an error. It asks the keychain whether an entry exists without reading the value (Windows:
  attributes; Linux: item attributes); macOS has no attributes, so there the value is read and
  wiped at once.
- Windows stores credentials with local persistence (`CRED_PERSIST_LOCAL_MACHINE`), not the
  roaming Enterprise default; an entry written before keeps its persistence until written again.
- Values Hangar holds (incoming value, values read for a start) are `Zeroizing<String>` and wiped
  after use. Copies outside Hangar's control are not: the IPC message, the keychain library's
  buffers, the child's environment block.
- Session start: a working folder (profile `cwd` or the one passed in) must be absolute.

### Export
`export_session(format: "text" | "html", content: string, suggestedName: string) -> string | null`:
the backend opens the save dialog itself (the frontend never picks a path, the dialog is modal to
the window, filter "Text" `*.txt` or "HTML" `*.html`) and writes the file atomically; returns the
path or `null` when cancelled. At most 32 MiB (checked before the dialog opens). The suggested
name is cleaned: `< > : " / \ | ? *` become `_`, control and invisible characters (the table under
Notifications) go,
leading and trailing dots and spaces go, Windows device names (`CON`, `NUL`, `COM1`, ...) get a
leading `_`, at most 100 characters, empty becomes `session`; the extension is added once.

### WSL
Windows only: the profile list gains one terminal per installed WSL distribution
(`%SystemRoot%\System32\wsl.exe --list --quiet`, UTF-16LE output, `WSL_UTF8` removed from its
environment, `CREATE_NO_WINDOW`, Job Object, 5 s limit), id `wsl-<distro lowercase>`, command
`%SystemRoot%\System32\wsl.exe -d <distro>` (the same absolute program as detection, never one
from PATH; the profile list shows that path as `command`), the session's working folder appended as `--cd <folder>`. Docker Desktop's
internal distributions (`docker-desktop*`) and names outside `[A-Za-z0-9._-]` are left out. The
entries have `source: "catalog"` (no new source value), `editable: false`, icon `WSL` and no
version query (`version_args = []`). They come after the catalog; a user profile or catalog entry
with the same id wins. Detection runs on a thread at startup, again on `reload_profiles`, and
`list_profiles` or a start of a `wsl-` id waits for it if it has not finished; measured
[[wsl-list-measurement]].

### Tray
Tauri feature `tray-icon`. `configure_tray(enabled: boolean, labels: { open, quit, tooltip }) -> null`.
Calling it again updates menu texts and tooltip; `enabled: false` removes the icon. Labels are cut
to one line of at most 64 characters (empty ones fall back to English). While the icon exists,
the backend hides the main window on a close request (`on_window_event`, `CloseRequested`:
`prevent_close` plus `hide`); without the icon it does nothing. Tauri 2.12 emits the close
request to the frontend in both cases (its own handler runs first, then the app's), so the
frontend's `onCloseRequested` handler always runs: it calls `preventDefault()` and quits only
when the tray is not active, where "active" means the last `configure_tray(true, ...)` resolved
without error and no `configure_tray(false, ...)` followed. A failed `configure_tray(true)`
leaves no icon, and closing then quits as before. The tray icon (left click) and the menu item
Open show, unminimize and focus the window again; the menu item Quit emits `tray-quit` (payload
`null`), and the frontend saves and quits as usual (`exit`, which does not pass through the close
request). Right click opens the menu. No window capability is needed for hiding;
`core:window:allow-show` stays granted but the frontend does not need it either.

## Frontend features

1. **Notifications**: on a switch to `attention` while the window has no focus and setting
   `notifyDesktop` is on (default on): `notify_session`. Text: "{title} is waiting for you"
   or "{title} has finished" after an exit. `notification-activated` activates the session and
   focuses its terminal.
   As built (`src/desktopNotify.ts`): the setting sits under Attention and only acts while
   attention is on (without attention nothing switches to `attention`). "Has finished" is chosen
   when the terminal has received its exit event. The body is the short working folder, so two
   sessions of the same program can be told apart. Placeholders (negative IDs) never notify.
   Unknown session IDs in `notification-activated` are ignored.
2. **Search** (Ctrl+Shift+F): `@xterm/addon-search` with match decorations; a small bar at the top
   right of the focused pane: field, "3 of 17", previous, next, case toggle, close. Enter goes to
   the next older match (up, since new output arrives at the bottom), Shift+Enter to the next
   newer one; typing searches from the newest output. Escape closes and returns focus to the
   terminal.
3. **Split view**: modes single, two side by side, four in a grid (menu View and launcher).
   Clicking a session shows it in the focused pane (or focuses it when visible). In split modes
   each pane gets a slim header (badge, title, attention dot); the focused pane has an accent
   edge. Empty slots say how to fill them. Mode persists in the layout.
   As built (`src/split.ts`): the active session is always the focused pane; a hidden session
   that becomes active takes the first empty pane, otherwise the focused one; empty panes take
   hidden sessions in sidebar order, so a pane stays empty only while there are fewer sessions
   than panes. Switching modes fills again: active session first, then sidebar order. Ctrl+Tab
   activates the next session in sidebar order, which follows the same rules as a click. Double
   click on a pane header shows only that session. No extra shortcut. The three modes were
   replaced by snap layouts in 0.3.0 (item 22).
4. **Launcher** (Ctrl+Shift+P): one field, fuzzy search over: running sessions (switch),
   "program in folder" for favorites and recent folders (start), notes folders (open notes),
   snippets (insert), view and broadcast commands. Own scorer, no dependency.
   As built (`src/fuzzy.ts`, `src/launcher.ts`): every word of the query must match as a
   subsequence; word starts and consecutive letters score higher. At most 8 results in sections.
   Without a query: sessions, snippets and commands (programs in the current folder only while
   no session runs). The UI calls it "quick access".
5. **Folders**: favorites and the last 8 used folders; the "Starts in" button opens a menu with
   favorites, recent folders, "Choose folder…" and "Add to favorites"/"Remove from favorites".
   Comparison ignores case for Windows paths and trailing separators.
6. **Status line**: per session in the sidebar: second line with folder, git branch and run time;
   first line status "working for 3 min", "waiting for you", "active 5 min ago", exit text.
   Branches via `git_branches`, refreshed every 15 s while the window is visible.
   As built (`src/statusLine.ts`, `src/gitBranches.ts`): durations are one whole unit (minutes,
   hours, days) through `Intl.NumberFormat` unit style, so no unit is translated; below one
   minute the plain status stays ("working", "running"). The working phase starts at the switch to
   `working` (kept in `SessionTerminal`, set on state changes only); "active N ago" uses the last
   output, which includes echo and redraws. Waiting and ended sessions keep their plain status
   text; the exit code stays in the tooltip. Line 2 shows the folder name, the branch (icon plus
   name, the first to be cut) and the run time since `createdAt` (not for ended sessions). One
   clock tick of 30 s in the sidebar re-renders only the sidebar. Branch paths: the sessions'
   `cwd` strings, each once, at most 64; refreshed at once on focus and visibility return. The
   collapsed rail shows both lines in its tooltip. Session actions lie over the right end of the
   row on its own background with a soft left edge (mask) and fade in on hover or keyboard focus,
   the active row included, so the status line keeps the whole row and nothing moves.
7. **Broadcast**: dialog "Send to several": text, checkboxes for running sessions (or start new
   ones with chosen programs in the current folder), then each gets the text as bracketed paste
   plus Enter; afterwards split view shows them side by side.
   As built (`src/broadcast.ts`): starting new sessions from the dialog is left out to keep it
   light. Checked at the start: running sessions whose command is not a known shell
   (`shellKind`), update sessions never. Enter is a separate write about 80 ms after the paste
   has reached the backend, so TUIs do not take it as part of the paste. Two targets show side by
   side, three or four in the grid.
8. **Worktree**: option "Own worktree" in New session (only for git folders): `create_worktree`,
   then start in it. Sidebar marks such sessions. Closing removes the worktree
   (`remove_worktree`); if it stays, a notice says why.
   As built (`src/worktree.ts`): the option is a checkbox below "Starts in", enabled only while
   `git_branches` reports a branch for that folder (the folder is added to the status line's branch
   paths). It is not saved: it applies for the current run to every start in the "Starts in"
   folder (sidebar, File, New session, Ctrl+Shift+T, empty state). Quick access lists separate
   entries "program in folder, own worktree" for that folder instead of following the checkbox. The
   label is the program name. While git checks out, a calm "Creating worktree" notice shows; create
   errors for missing git, no repository and network shares get translated sentences, others keep
   git's words. If the session then fails to start, the new worktree is removed again. Recent
   folders record the repository folder, not the worktree. The sidebar shows a worktree icon (accent
   color) with the branch in line 2, and the tooltip names the branch. The close confirmation says
   the worktree goes too unless it holds changes; exited sessions close without confirmation, as
   removal never forces. After closing, a notice appears only when something stayed: the folder
   with the reason (remaining changes and ignored files as a translated sentence, anything else in
   git's words), or (folder gone, branch kept) the branch name, each copyable. Placeholders
   from a failed restore keep their worktree and remove it when closed.
9. **Snippets**: stored texts (Settings section, defaults "Review the changes", "Write tests",
   "Explain this"), inserted into the active session as bracketed paste via `term.paste` after
   `sanitizePaste`; from the launcher and the terminal context menu.
   As built (`src/snippets.ts`): stored in the settings as `{ id, text }`, seeded in the UI
   language of the first start; an empty list stays empty. The label is the first line. The
   context menu lists up to five, more are in quick access. No Enter is appended.
10. **Changes**: panel "Changes" (Ctrl+Shift+D) at the right like the notes panel: file list with
    status and +/-, unified diff with colors, refresh, auto refresh when the session's attention
    changes.
    As built (`src/gitDiff.ts`, `src/ChangesPanel.tsx`): notes and changes share the right side,
    its width and the docked or overlay placement; opening one closes the other (opening the changes
    also saves the notes as closed in the layout). The changes panel is not remembered
    across restarts. It reads `git_diff` on open, on a new active session, on Refresh and on every
    attention state change of the active session; the previous list stays visible while reading.
    Each file shows a status letter in a box (A, D, R, U, !, M, with its name for screen readers),
    name, folder and +/-. Files open by themselves while each has at most 200 diff lines and
    together at most 800; each open file renders 300 lines per page ("Show more"). Diff text goes
    into text nodes only. Quoted paths (`core.quotePath`) are decoded, CRLF is tolerated. Escape
    returns the focus to the terminal. Also in View and quick access.
11. **Drag and drop**: files from Explorer onto a pane insert their quoted paths as bracketed paste
    (`onDragDropEvent`, pane under the cursor). Quoting per program: PowerShell `'...'` with `''`,
    cmd `"..."`, POSIX shells and WSL `'...'` with `'\''`, agent CLIs `"..."` when needed.
12. **Terminal look**: color scheme (Hangar, Catppuccin, Solarized, each with light and dark variant
    following the app theme), font (Cascadia Code, Cascadia Mono, JetBrains Mono, Fira Code,
    Consolas; installed ones only), cursor style and blink. Ligatures are left out for now
    (unverified assumption: `@xterm/addon-ligatures` reads font files through Node, which the
    WebView does not offer).
13. **Session marker**: color and emoji per session (context menu "Marker"), shown in the
    sidebar, pane header and rail; saved in the workspace.
    As built (`src/marker.ts`): a fixed palette of eight colors plus none, stored lowercase; emoji
    is free text of 1 to 16 bytes without control characters, with eight quick picks. Opened from
    the session menu (right click or context menu key on a session, right click on a pane
    header) and File, Marker. Invalid stored values are dropped, the session is kept. The pane
    header never takes the focus (mouse down is prevented), so every key after a click on it,
    Ctrl+C included, still reaches the program; by keyboard the session menu opens from the
    sidebar row.
14. **Taskbar badge**: number of waiting sessions as an overlay icon (Windows) or badge count.
    As built (`src/taskbarBadge.ts`, `src/badgeImage.ts`): no setting of its own; it follows the
    attention setting (without attention the count is 0). Windows draws a 32 px disc in the
    attention color with a dark ring, "1" to "9", then "9+", passed through `Image.fromBytes`
    (permission `core:image:allow-from-bytes` comes with `core:default`) and closed after
    `setOverlayIcon`. macOS and Linux get `setBadgeCount` with the real number. The platform comes
    from the user agent. A call is sent only when the label or color changes; errors are swallowed
    and retried on the next change.
    Quitting clears a shown badge first (at most 300 ms), so no count stays on a Dock or launcher
    icon.
15. **WebGL**: measured, not adopted. The DOM renderer was as fast or faster in every scenario
    and WebGL was about 4x slower on color-heavy output ([[webgl-measurement]]). Revisit only with
    data from a real WebView2 host and fractional DPI.
16. **Profile editor**: Settings, Programs: form for name, command, arguments, folder,
    environment and secret variables, saved at once via `save_profile`.
    As built (`src/profileForm.ts`, `src/ProfileEditor.tsx`): each installed row has "Edit" (own
    profiles) or "Customize" (catalog and WSL entries: saving creates a user profile with the same
    id; deleting it brings the built-in entry back). "New program" opens an empty form; its id comes
    from the name (lowercase ASCII, unique without regard to case) and is never shown. Own profiles
    whose command is not found get a list "Not found on this computer", so they can be fixed or
    deleted. Arguments are one line: spaces separate, `'...'` is literal, `"..."` knows only `\"`
    and `\`, so Windows paths can be typed as they are; `joinArgs` quotes only where needed.
    `profile_env` is read once when the form opens. Problems found before saving and backend errors
    are shown at the field or below the form. The new list from `save_profile` and
    `delete_profile` replaces the app's list at once. Delete asks inline.
17. **Secrets**: per secret variable a password field; stored via `set_profile_secret`, shown only as
    "stored".
    As built: password fields are uncontrolled, so values never enter React state; they are read
    on Save after `save_profile` succeeded, sent, and the field is cleared. The state "Stored" (filled
    dot) or "Not stored" (ring) comes from `profile_secrets`, with "Remove value"
    (`delete_profile_secret`). A plain variable whose name looks like a credential (KEY, TOKEN,
    SECRET, PASSWORD, CREDENTIAL) shows a hint with "Make it secret", which moves the name and puts
    its value into the new password field.
18. **Export**: menu "Export session as text / HTML" (SerializeAddon `serializeAsHTML`), via
    `export_session`.
19. **WSL**: no frontend work beyond badges.
    As built: a three letter ASCII icon such as `WSL` stays whole in the letter badge (smaller
    type); a program icon from `program_icons` still wins in "Automatic". WSL entries count as
    shells for drag and drop and broadcast (`shellKind`).
20. **Tray**: setting "Keep running in the notification area" (default off); `configure_tray`;
    `tray-quit` runs the normal quit.
    As built (`src/tray.ts`): the setting is the first one under General, Sessions. `TrayState`
    tracks "active" as the contract says; overlapping calls are ordered, an answer older than one
    already settled is ignored, and turning off counts at once. Nothing is sent while the setting
    was never on. The labels are re-sent when the language or the number of waiting sessions
    changes; the tooltip reads "Hangar · waiting: N" when sessions wait.

Paste fix: Ctrl+V in the terminal lets the native paste event happen (handled in `handlePaste`);
menu paste uses the clipboard plugin.

## Additions (2026-11-08, second request)

21. **Reorder sessions** in the sidebar by drag and drop (mouse) and with the keyboard (e.g.
    Alt+Up/Down on a focused row); the order is the workspace order and survives a restart.
22. **Snap layouts** like Windows 11: the split view gets layout presets instead of three fixed
    modes: single, two side by side, three columns, one large plus two stacked, four in a grid.
    Dragging a session from the sidebar over the body shows a slim bar with layout thumbnails
    at the top; dropping on a zone switches to that layout and places the session in that zone.
    The same picker is available from View and the launcher without dragging. Replaces
    `splitMode` (old stored values migrate).
    As built, 21 (`src/sessionDrag.ts`, `src/useSessionDrag.ts`, reducer action `moved`): a press
    on a row becomes a drag after 5 px, so click, double click (rename) and the context menu stay
    as they were. A 2 px accent line shows where the row lands (none when nothing would change);
    the dragged row fades and its hover actions hide; a small ghost (badge, title) follows the
    pointer by `transform` only, so no React render per move. Near the list edges the list
    scrolls (up to 14 px per frame). Escape, window blur or a lost pointer capture cancel. The
    collapsed rail reorders the same way. Alt+Up / Alt+Down on a focused row moves it one place
    (`rowMoveStep` in `src/shortcuts.ts`, handled by the row and not by `matchShortcut`, because
    in the terminal Alt+arrow belongs to shells and agents); the focus stays on the row and a
    status region announces "Title: position 2 of 5". The workspace saves the new order.
    Pointer events with pointer capture, not HTML5 drag and drop: Tauri's native file drop
    (`dragDropEnabled`, needed for files from Explorer, item 11) is said to disable HTML5 drag
    and drop inside the page on Windows.
    > [!question] Unverified
    > The HTML5 statement comes from Tauri's configuration reference for `dragDropEnabled` and
    > was not measured here. Pointer capture also keeps the gesture away from xterm, so no
    > program sees mouse motion with a pressed button. File drops and session drags never meet.
    As built, 22 (`src/snapLayouts.ts`, `src/split.ts`, `src/SnapBar.tsx`): layouts `single`,
    `pair`, `triple`, `mainStack` (left 60 %, two stacked right) and `quad`, each a list of zones
    as fractions; one pane per zone. Stored as `snapLayout` in the layout; `splitMode` values
    migrate (`columns` to `pair`, `grid` to `quad`). Panes stay absolutely positioned from the
    zone rectangles instead of a CSS grid: hidden panes stay in the DOM (invisible, at the size of the whole panes area), so the scrollback survives. Switching to a
    layout with as many zones keeps every session in its pane; otherwise the old fill rule
    applies. `placeSession`: same zone count, the session takes the zone (swapping with its old
    pane when visible); another count, the zone gets the session, the other zones keep the
    visible sessions in order, then hidden ones. While a session is dragged over the panes area,
    the snap bar shows at the top center; the zone under the pointer lights up and an accent
    outline previews it in the panes area. Dropping on a pane outside the bar puts the session
    into that pane of the current layout. The picker ("Choose layout ...", View and quick
    access) shows the same bar with a caption; arrows move through all 13 zones, Enter places the
    active session, Escape, a click outside or focus leaving closes it. View and quick access
    also list the five layouts by name. "Send to several" picks the smallest fitting layout
    (three sessions now get `triple`). Reduced motion turns off the bar's slide and the
    preview's movement.
23. **Sub-agents in the body**: right click on a running agent session, "Show sub-agents". Sub-agents
    run inside the agent's process and have no terminal of their own, so Hangar shows each one as
    a read-only live view of its transcript in a pane. Depends on research into where each CLI
    stores sub-agent transcripts and how Hangar maps its session to the CLI's session id.
    As built, backend (`src-tauri/src/subagents/`, `src-tauri/src/cli_session.rs`): every claude
    session gets a conversation id ([[resume]]); `SessionInfo.subAgents` is `true` when the
    program has a sub-agent source (`sub_agents = "claude"` in the catalog) and the session an
    id, which is when the menu entry applies.
    - `watch_sub_agents(sessionId: number, onEvent: Channel, excerpts?: boolean, focus?: string |
      null)`: `null`, or an error text (unknown session, no sub-agents for it, invalid `focus`).
      A second call for the same session replaces the first watch. `excerpts` comes from the
      frontend setting; `false` means no assistant text is read at all and every `excerpt` is
      `null`; missing means `true`. `focus` names the one sub-agent whose activity log is
      streamed as well (missing or `null`: none).
    - `focus_sub_agent(sessionId: number, agentId: string | null)`: `true` if a watch runs and
      got the new focus, `false` if none runs; an id outside `[A-Za-z0-9_-]{1,64}` is refused.
      The watch polls at once, the next log batch has `reset: true`. Switching sub-agents never
      restarts the watch. Switches that queued up while a poll ran collapse to the newest one,
      and the window sends a switch only after it settled for 100 ms (`FocusSender` in
      `src/subAgentStore.ts`), so clicking through sub-agents costs one reset and one read. The
      refused id in the error text is cut to 64 characters.
    - `unwatch_sub_agents(sessionId: number)`: `true` if a watch was running. `close_session` and
      app exit end the watch too, as does a channel the window dropped (noticed at the next
      change; a reload that calls `watch_sub_agents` again replaces the old watch).
      `restart_session` ends it as well, because the restart runs under a new `cliSessionId`: the
      window calls `watch_sub_agents` again with the returned `SessionInfo` if the view is open.
      A `resumeFallback` carries the current `cliSessionId` and `subAgents`: the same id after
      `resume_id_args`, a new one after `resume_args` (`--continue`), which had none. While a
      `--continue` resume runs, `subAgents` is `false`.
    - Event, always a full snapshot that replaces the previous one, sent once at the start and
      then only on a change (one poll every 250 ms, so at most 4 events per second per session):
      `{ "type": "snapshot", "located": bool, "agents": [{ "id": string, "description":
      string | null, "agentType": string | null, "spawnDepth": number | null, "status": "running" |
      "finished" | "stale", "startedAt": number | null, "lastActivity": number | null, "tool":
      string | null, "excerpt": string | null }] }`. `located: false` until claude wrote the
      session's transcript (after the first prompt). Times are Unix milliseconds; agents are
      ordered oldest first; `stale` = not finished and no write for 60 s; `tool` = newest tool
      call name; `excerpt` = newest assistant text, at most 400 characters. For the snapshot,
      prompts, tool inputs, tool results and every other record type are never read.
    - Log event of the focused sub-agent (`src-tauri/src/subagents/claude_log.rs`), sent after
      the snapshot of the same poll and only when there are new lines or on a reset (so at most
      one snapshot and one log batch per poll): `{ "type": "log", "agentId": string, "reset":
      bool, "entries": [ { "kind": "prompt", "text" } | { "kind": "text", "text" } | { "kind":
      "tool", "name", "detail": string | null } | { "kind": "result", "text": string | null,
      "moreLines": number, "error": bool } | { "kind": "gap" } ] }`. `reset` replaces what was
      shown (new focus, transcript that started over). The log has its own read position: the
      first read takes the last 1 MiB, then only what was appended; when that left out the
      start, the task (first line) is read once and a `gap` follows it; a burst keeps its newest
      400 entries behind a `gap`. `prompt` is user text (the task), `text` assistant text (both
      at most 4000 characters), `tool` the call with its key argument (Bash and PowerShell
      `command`, Read/Write/Edit/MultiEdit `file_path`, NotebookEdit `notebook_path`, Grep and
      Glob `pattern`, WebFetch `url`, WebSearch `query`, Task and Agent `description`, other
      tools the first of the usual fields; first line only, at most 200 characters), `result`
      the first non-empty line of a tool result (at most 200 characters) plus the count of
      further lines. Thinking blocks, `isMeta` records and other record types are never shown.
      With `excerpts: false` only assistant records are parsed and only tool names are sent
      (`detail: null`); no task, text, input or result. The log reads the transcript only when
      the snapshot's scan found it in the contained sub-agent folder.
    - Excerpt privacy (`src-tauri/src/subagents/redact.rs`, `masked` in `claude.rs`): in this
      order, first complete terminal sequences (CSI such as colors, OSC such as titles and
      links, DCS and the other string sequences, two-byte and charset escapes, their 8-bit
      forms), every other C0 and C1 control character except newline and tab, and invisible
      format characters (`src-tauri/src/text.rs`) are removed, so a color code cannot split
      `API_KEY` from its value; then secrets are masked on the whole text; only then is it cut.
      Masked as `[redacted]`: values of names ending in key, token, secret, password, passwd,
      pwd, passphrase, credential or credentials, and of `pass`, `auth` and `cookie` as a whole
      name or after `_` or `-` (`DB_PASS`, npm `_auth=`, `Set-Cookie`; `compass` and `oauth2`
      stay), with `=` or `:` (`API_KEY=...`, `"client_secret": "..."`, `"auth": "..."`,
      `password: ...`, `x-api-key: ...`); a bare unquoted `key:`, `auth:` or `cookie:` masks only
      a value of at least 16 token characters or one holding `=`, so prose headings stay, and a
      `Cookie:` value runs to the end of the line. Command line flags with such a name take the
      next word too (`--password X`, `--client-secret "X"`, `--token X`; not `--no-...`, not when
      the next word is another flag); `-p` only where it is a password: `sshpass -p X` and
      `-pX`, `docker login -p X` (also podman), the glued `mysql -pX` (also mysqldump,
      mysqladmin, mysqlimport, mariadb; `mysql -p db` prompts and `docker run -p 80:80` is a
      port), also glued (`docker login -pX`); `-u`, `-uX` or `--user` with `user:password`
      everything after the first `:` unless both sides are numbers (`docker run -u 1000:1000`).
      Words follow shell quoting (`-u "user:pa ss"`, `-p'a b'`), quoted values honor backslash
      escapes (`"a\"b"`), and any Unicode space (tab, no-break space) separates a flag from its
      value. Also masked: the credential after `Authorization:` with the scheme `Basic`,
      `Bearer`, `Token` or `Digest` (at least 8 characters), or without a scheme when it looks
      like a key (at least 8 characters holding a digit or symbol, or at least 16), the token
      after `Bearer ` anywhere (at least 16), the value of PowerShell
      `ConvertTo-SecureString "X" -AsPlainText` (also with `-String`), URL passwords
      (`https://user:...@host`) and long URL user names, words starting with a known token prefix
      (`sk-`, `sk_live_`, `ghp_`, `ghr_`, `github_pat_`, `glpat-`, `xox?-`, `xapp-`, `AKIA`,
      `AIza`, `npm_`, `hf_`, `SG.`, `dop_v1_`, `shpat_`, `ya29.`, `eyJ` and others, with enough
      characters after them) and `-----BEGIN ... KEY-----` or `PRIVATE KEY BLOCK` blocks. The
      description and every log text (task, text, tool argument, result) are masked the same
      way. Every pass is linear in the text length (a skipped value is passed over as a whole),
      checked by a test with 1 MB lines such as `key:` repeated. Known limits: names written
      with Unicode lookalikes (fullwidth `ＡＰＩ_ＫＥＹ`, Cyrillic letters) and secrets of
      tools or formats not listed here pass unmasked; the filter matches ASCII names on purpose
      and needs no dependency. A best-effort filter, not a guarantee; the setting's help text
      says so. Excerpts and log lines live only in the watcher's memory and the
      window's view state: it is never persisted (workspace, scrollback, notes), exported, logged
      or put into notifications, and the frontend must keep it that way.
    - Files: `<CLAUDE_CONFIG_DIR or ~/.claude>/projects/*/<id>.jsonl` (the encoded working
      folder first; while nothing is found the search repeats after 2, 4, 8, 16, then every 30 s,
      and goes through all project folders only when the projects folder's modification time
      changed or at the 30 s delay), then `<id>/subagents/agent-<agentId>.jsonl` and
      `.meta.json`; links and junctions out of the projects folder are not followed, and every
      transcript and meta file is opened without following a link in its last part
      (`FILE_FLAG_OPEN_REPARSE_POINT` on Windows, `O_NOFOLLOW` on Unix) and read only when the
      opened handle is a regular file (`open_shared` in `tail.rs`; on Unix also `O_NONBLOCK`, so a
      FIFO is refused instead of hanging the watcher). Not covered: a folder above the file
      swapped for a link between the per-poll folder check and the open, and hard links; both
      need write access to the user's Claude folder, that is the same user; the newest
      256 sub-agents at most (at most 1024 directory entries read per poll); only the last
      256 KiB of a transcript are read at first; lines over 1 MiB are skipped. Polling instead of `notify` (CC0-1.0, outside the license policy).
    - Codex, OpenCode and Gemini: not built; each would be another `SubAgentKind` with its own
      `SubAgentSource` ([[subagent-transcripts]]).
    As built, frontend (`src/subAgents.ts`, `src/useSubAgents.ts`, `src/SubAgentPane.tsx`),
    reworked 2026-11-11 on the owner's request (one sub-agent at a time, its full activity, and
    finished ones removable): "Show sub-agents" sits in the session menu (sidebar row, rail, pane
    header) and in quick access, only when `subAgents` is true (or the view is open, then it
    reads "Hide sub-agents"). Opening it makes the session active; the body then always uses
    `pair`: the session's terminal in the main zone (so the main agent stays in sight and can be
    answered, and switching sub-agents never resizes it) and one read-only pane beside it. The
    earlier grid of one pane per sub-agent was dropped. The pane shows either the activity log
    of the sub-agent picked in the tree, or, without a pick (menu entry, "N more" row, back
    button), the list of all sub-agents (newest first; a row opens its log). The log reads like
    Claude Code's own conversation: `>` plus the task in a framed block, `●` plus assistant
    text, `●` plus the bold tool name and its argument in parentheses, `⎿` plus the result's
    first line and "+N lines" (failed results in the error color with the word "Error"), and a
    centered note where entries were left out; plain React text with `pre-wrap`, in the
    terminal's font, text and background color (`--terminal-font`, `--terminal-fg`,
    `--terminal-bg` set by the app from the terminal look). It follows new lines while scrolled
    to the end (within 24 px); scrolling up stops that and shows a "Jump to end" button. The
    view keeps at most 1000 lines (`applyLog`): older ones give way to one gap line, the task
    stays first. After a switch it drops appends until the first `reset`, so lines still in
    flight from an earlier focus never show. The header: back button, name, type, status (text plus a dot whose shape
    differs: filled, ring, square; the running dot pulses only without reduced motion), run time
    (`m:ss`, ticking each second), close. With texts off, the log says so and lists tool names
    only; a sub-agent whose files are gone says so. The split view's own slots stay untouched
    and come back when the view closes: by the close button, Escape inside the pane (the
    terminal gets the focus back), the menu entry, clicking the shown tree row again, another
    session becoming active, the session closing, or any arranging of the panes. Switching
    between list and log by keyboard moves the focus into the new body (the first row of the
    list). Before the first snapshot, while `located` is `false`, when `subAgents` turned
    `false` and on a refused watch, a notice says so. Watch, focus and unwatch run one after
    the other in call order, so the unwatch of a closing view never ends the watch that replaces
    it. Setting "Show sub-agent text" (Settings, Sessions, default on) is passed as `excerpts`.
    Snapshots and the log live only in memory, in an external store (`src/subAgentStore.ts`)
    read with `useSyncExternalStore`: the tree and the pane re-render per event, the app only
    when the visible count, `located` or the error changes; log lines are memoized rows with
    stable ids.
    As built, sidebar tree (`src/SubAgentTree.tsx`, `subAgentTree` and `watchedSession` in
    `src/subAgents.ts`): one watch at most, for the open view's session, else the active
    session while the expanded sidebar shows the Sessions group. Without the view it runs with
    `excerpts: false`; opening the view restarts it with the setting, keeping the agents (the
    key is session plus conversation id only) and dropping excerpts at once when they are no
    longer wanted. The tree lists up to five sub-agents (the shown one, running and newer
    first, in start order, else four plus "N more"); a row shows that sub-agent's log and is
    highlighted. Finished rows get a remove button that replaces the run time on hover or
    keyboard focus; with two or more finished ones a last row "Remove finished" removes them
    all. Removing only hides them in the tree and the list (`dismissFinished`, finished ones
    only, per session and conversation id); transcripts are never touched. The removed ids live
    in the app's memory for this run of Hangar, not in the workspace: a view preference that
    needs no format change, and a new conversation starts with a clean tree anyway. Removing the
    shown sub-agent closes the view; removing by keyboard moves the focus to the next row (or
    the previous one), and to the terminal once the tree is empty; by mouse the terminal gets
    the focus back.
    As built, conversation ids (`src/workspace.ts`, `src/sessions.ts`): `cliSessionId` is saved
    per entry (lower case, invalid or duplicate ids dropped to `null`, so the backend never
    refuses the workspace), passed to `create_session` on restore and on restarting a
    placeholder, and updated from `restart_session` and from `resumeFallback` (reducer action
    `fellBack`, which also clears `resumed`). A fallback that arrives before `create_session`
    answers is kept on the terminal and applied after the session is added.

24. **Clearer attention in the sidebar** (owner's request): the left bar marks only the active
    session; a waiting session gets a status chip at the right ("done", bell icon when the program
    rang) and a clearly visible, soft breathing tint of the whole row; distinct shapes per state
    (working: three animated dots, waiting: check or bell, crashed: red exclamation) so color is
    never the only signal; collapsed rail: pulsing ring around the badge plus a count dot; a short
    synthesized two-tone sound (Web Audio, no asset) when a session starts waiting while it is not
    visible, a lower tone on a crash, with a setting (on by default, quiet), volume and a preview
    button; reduced motion replaces pulsing with a strong static highlight.
    As built (`src/attention.ts`, `src/sessionChip.ts`, `src/StatusChip.tsx`, `src/chime.ts`,
    `src/chimeAudio.ts`, `src/attentionLook.ts`): the tracker reports a reason with every state
    (`quiet`, `bell`, `exit`; a bell after an exit keeps the exit), and a reason change alone does
    not flash or notify again. Chip per row: working three dots (static with reduced motion),
    "done" check, "asks" bell (status line "asks you something"), failed red exclamation (status
    line "exited with an error", also after it was seen), ordinary exit a quiet square. The chip is
    decorative (`aria-hidden`); the status line carries the words. On hover or keyboard focus in
    the actions, the actions replace the chip (it fades out). A waiting row breathes (2.8 s by
    default) on the sidebar color with a thin ring; the actions breathe along on the same
    keyframes. Each state (working, done, asks, pause) has its own state look (`src/stateLook.ts`,
    `src/stateLookCss.ts`): color (default cream for waiting, the theme's green otherwise), pulse
    strength off, subtle, medium (the original breath) or strong, and duration 0.8 to 4 s; the
    app sets `--look-<state>-*` on the root and `look-<state>` classes hand them to a row. A
    custom color keeps edges at 3 : 1 and the "working" chip label at 4.5 : 1. The older single
    `attentionColor` setting migrates to both waiting states. The
    peak tint is computed per color and theme so the row's text keeps 4.5 : 1 and its muted text
    3 : 1; the chip text color (ink or white) follows the attention color, in the light theme the
    chip uses the darkened edge color with white text. Rail: breathing outline around the badge,
    the chip's shape in a small circle for waiting and failed sessions, and the waiting count on
    the expand button (also in its description). Chime: setting "Play a sound" (on), volume
    5 to 100 % (default 40, squared to a gain of at most 0.32), "Preview"; it plays when a session
    enters "attention" while the window is unfocused or the session is in no pane, or when a
    waiting session then fails; requests within 150 ms merge (failure wins), then 2 s of silence.
    No chime before the first click or key press in the window (the engine keeps audio suspended
    until then), and none whose audio resume takes longer than 500 ms, so a chime never sounds late.
