---
title: "Agent workflow features (0.3.0)"
type: synthesis
status: accepted
tags: [notifications, search, split-view, launcher, git, worktree, snippets, themes, profiles, secrets, tray]
sources: []
created: 2026-11-08
updated: 2026-11-08
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
- Titles and bodies lose control characters, bidi controls (U+200E, U+200F, U+202A to U+202E,
  U+2066 to U+2069), zero-width characters (U+200B to U+200D, U+2060, U+FEFF); U+FFFE and U+FFFF
  become U+FFFD.

### Git
All git calls: `git` from PATH, argv only, `GIT_OPTIONAL_LOCKS=0`, `GIT_TERMINAL_PROMPT=0`,
`-c core.fsmonitor=false`, timeout 10 s, no pager, no external diff or textconv. Every inherited
`GIT_*` variable is removed from git's environment before Hangar sets its own. If the process tree
cannot be put into a Job Object, git is killed and the call fails. `status` and `diff` run without
the repository's filters: Hangar first reads `git config -z --get-regexp
"^filter\..*\.(clean|smudge|process)$"` and passes `-c filter.<name>.clean= -c
filter.<name>.smudge= -c filter.<name>.process= -c filter.<name>.required=false` per driver
(a driver name with `=` or more than 64 drivers refuse the call), plus `--ignore-submodules=all`.
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
  stays, `reason` says why); `removed` is true only when the folder is gone afterwards. Then
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
  The id must not be empty or contain whitespace or `/`. A catalog or WSL id creates a user
  profile that replaces that entry. Names dropped from `secretEnv` lose their keychain entries.
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
- `set_profile_secret` rejects an empty value and values over 1280 UTF-16 units (Windows
  Credential Manager stores at most 2560 bytes). Profile id and name follow the rules above.
  `profile_secrets` lists the names of the profile's `secret_env`; an unknown id is an error.

### Export
`export_session(format: "text" | "html", content: string, suggestedName: string) -> string | null`:
the backend opens the save dialog itself (the frontend never picks a path, the dialog is modal to
the window, filter "Text" `*.txt` or "HTML" `*.html`) and writes the file atomically; returns the
path or `null` when cancelled. At most 32 MiB (checked before the dialog opens). The suggested
name is cleaned: `< > : " / \ | ? *` become `_`, control and invisible format characters go,
leading and trailing dots and spaces go, Windows device names (`CON`, `NUL`, `COM1`, ...) get a
leading `_`, at most 100 characters, empty becomes `session`; the extension is added once.

### WSL
Windows only: the profile list gains one terminal per installed WSL distribution
(`%SystemRoot%\System32\wsl.exe --list --quiet`, UTF-16LE output, `WSL_UTF8` removed from its
environment, `CREATE_NO_WINDOW`, Job Object, 5 s limit), id `wsl-<distro lowercase>`, command
`wsl.exe -d <distro>`, the session's working folder appended as `--cd <folder>`. Docker Desktop's
internal distributions (`docker-desktop*`) and names outside `[A-Za-z0-9._-]` are left out. The
entries have `source: "catalog"` (no new source value), `editable: false`, icon `WSL` and no
version query (`version_args = []`). They come after the catalog; a user profile or catalog entry
with the same id wins. Detection runs on a thread at startup, again on `reload_profiles`, and
`list_profiles` or a start of a `wsl-` id waits for it if it has not finished; measured
[[wsl-list-measurement]].

### Tray
Tauri feature `tray-icon`. `configure_tray(enabled: boolean, labels: { open, quit, tooltip }) -> null`.
Calling it again updates menu texts and tooltip; `enabled: false` removes the icon. Labels are cut
to one line of at most 64 characters (empty ones fall back to English). With tray enabled,
closing the window hides it (the frontend's close handler calls `hide()`); the tray icon (left
click) and the menu item Open show, unminimize and focus it again; the menu item Quit emits
`tray-quit` (payload `null`), and the frontend saves and quits as usual. Right click opens the
menu. Capabilities `core:window:allow-hide`, `core:window:allow-show`.

## Frontend features

1. **Notifications**: on a switch to `attention` while the window has no focus and setting
   `notifyDesktop` is on (default on): `notify_session`. Text: "{title} is waiting for you"
   or "{title} has finished" after an exit. `notification-activated` activates the session and
   focuses its terminal.
2. **Search** (Ctrl+Shift+F): `@xterm/addon-search` with match decorations; a small bar at the top
   right of the focused pane: field, "3 of 17", previous, next, case toggle, close. Enter goes to
   the next older match (up, since new output arrives at the bottom), Shift+Enter to the next
   newer one; typing searches from the newest output. Escape closes and returns focus to the
   terminal.
3. **Split view**: modes single, two side by side, four in a grid (menu View and launcher).
   Clicking a session shows it in the focused pane (or focuses it when visible). In split modes
   each pane gets a slim header (badge, title, attention dot); the focused pane has an accent
   edge. Empty slots say how to fill them. Mode persists in the layout.
4. **Launcher** (Ctrl+Shift+P): one field, fuzzy search over: running sessions (switch),
   "program in folder" for favorites and recent folders (start), notes folders (open notes),
   snippets (insert), view and broadcast commands. Own scorer, no dependency.
5. **Folders**: favorites and the last 8 used folders; the "Starts in" button opens a menu with
   favorites, recent folders, "Choose folder…" and "Add to favorites"/"Remove from favorites".
   Comparison ignores case for Windows paths and trailing separators.
6. **Status line**: per session in the sidebar: second line with folder, git branch and run time;
   first line status "working for 3 min", "waiting for you", "active 5 min ago", exit text.
   Branches via `git_branches`, refreshed every 15 s while the window is visible.
7. **Broadcast**: dialog "Send to several": text, checkboxes for running sessions (or start new
   ones with chosen programs in the current folder), then each gets the text as bracketed paste
   plus Enter; afterwards split view shows them side by side.
8. **Worktree**: option "Own worktree" in New session (only for git folders): `create_worktree`,
   then start in it. Sidebar marks such sessions. Closing removes the worktree
   (`remove_worktree`); if it stays, a notice says why.
9. **Snippets**: stored texts (Settings section, defaults "Review the changes", "Write tests",
   "Explain this"), inserted into the active session as bracketed paste via `term.paste` after
   `sanitizePaste`; from the launcher and the terminal context menu.
10. **Changes**: panel "Changes" (Ctrl+Shift+D) at the right like the notes panel: file list with
    status and +/-, unified diff with colors, refresh, auto refresh when the session's attention
    changes.
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
14. **Taskbar badge**: number of waiting sessions as an overlay icon (Windows) or badge count.
15. **WebGL**: measured, not adopted. The DOM renderer was as fast or faster in every scenario
    and WebGL was about 4x slower on color-heavy output ([[webgl-measurement]]). Revisit only with
    data from a real WebView2 host and fractional DPI.
16. **Profile editor**: Settings, Programs: form for name, command, arguments, folder,
    environment and secret variables, saved at once via `save_profile`.
17. **Secrets**: per secret variable a password field; stored via `set_profile_secret`, shown only as
    "stored".
18. **Export**: menu "Export session as text / HTML" (SerializeAddon `serializeAsHTML`), via
    `export_session`.
19. **WSL**: no frontend work beyond badges.
20. **Tray**: setting "Keep running in the notification area" (default off); `configure_tray`;
    `tray-quit` runs the normal quit.

Paste fix: Ctrl+V in the terminal lets the native paste event happen (handled in `handlePaste`);
menu paste uses the clipboard plugin.
