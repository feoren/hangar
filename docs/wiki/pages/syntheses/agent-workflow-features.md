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
  Windows: `tauri-winrt-notification` (the crate `tauri-plugin-notification` uses), app id =
  bundle identifier in release builds, the PowerShell app id in `tauri dev`. One notification
  per session at a time (tag or replace by session id). Title and body are cut to 200 characters.
- Click on the notification: the backend shows, unminimizes and focuses the main window and emits
  the event `notification-activated` with `{ sessionId: number }`.

### Git
All git calls: `git` from PATH, argv only, `GIT_OPTIONAL_LOCKS=0`, `GIT_TERMINAL_PROMPT=0`,
`-c core.fsmonitor=false`, timeout 10 s, no pager, no external diff or textconv.
- `git_branches(paths: string[]) -> Record<string, string | null>`: branch per folder. Reads
  `HEAD` from the `.git` directory or follows a `.git` file (`gitdir: ...`, worktrees,
  submodules) walking up from the folder; no process. `ref: refs/heads/x` gives `x`, a detached
  HEAD the first 7 hex characters; no repository gives `null`. At most 64 paths per call.
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
  `git worktree remove <path>` without `--force` (a worktree with changes stays, `reason` says
  why), then `git branch -d <branch>` (an unmerged branch stays). Refuses any path outside the
  app's worktrees folder.

### Workspace
`SavedSession` gains three optional fields (absent or `null` in old files):
`color: string | null` (`#rrggbb`), `emoji: string | null` (at most 16 bytes UTF-8), and
`worktree: Worktree | null`. The backend validates them.

### Taskbar badge
Tauri feature `image-png`; capabilities `core:window:allow-set-overlay-icon` (Windows) and
`core:window:allow-set-badge-count` (macOS, Linux). The frontend draws the number.

### Profiles in Settings
- `save_profile(profile: ProfileInput) -> Profile[]`: inserts or replaces a `[[profile]]` in
  `profiles.toml` by id, keeps comments and other entries (`toml_edit`), validates like loading,
  reloads and returns the new list. `ProfileInput`:
  `{ id, name, command, args: string[], cwd: string | null, env: Record<string, string>, secretEnv: string[] }`.
- `delete_profile(id: string) -> Profile[]`.
- `Profile` (list view) gains `editable: boolean` (user profile) and `env: string[]` (names only,
  never values) and `secretEnv: string[]`. Values of `env` are returned by
  `profile_env(id) -> Record<string, string>` only for the edit form.

### Secrets (OS keychain)
Crate `keyring` (Windows Credential Manager, macOS Keychain, Linux Secret Service). Service
`hangar`, account `<profile id>/<VARIABLE>`.
- `set_profile_secret(profileId, name, value) -> null`, `delete_profile_secret(profileId, name) -> null`,
  `profile_secrets(profileId) -> { name: string, stored: boolean }[]`. Values never leave the
  backend and never reach logs or error texts.
- On session start, each name in the profile's `secret_env` is read from the keychain and set as
  an environment variable of the child process. A missing secret is skipped.

### Export
`export_session(format: "text" | "html", content: string, suggestedName: string) -> string | null`:
the backend opens the save dialog itself (the frontend never picks a path) and writes the file;
returns the path or `null` when cancelled. At most 32 MiB.

### WSL
Windows only: the profile list gains one terminal per installed WSL distribution
(`wsl.exe --list --quiet`, UTF-16LE output), id `wsl-<distro lowercase>`, command
`wsl.exe -d <distro>`, working folder via `--cd`. Docker Desktop's internal distributions are
left out. Detection is cached per profile reload.

### Tray
Tauri feature `tray-icon`. `configure_tray(enabled: boolean, labels: { open, quit, tooltip }) -> null`.
With tray enabled, closing the window hides it; the tray icon (left click) and the menu item
Open show it again; the menu item Quit emits `tray-quit`, and the frontend saves and quits as
usual. Capabilities `core:window:allow-hide`, `core:window:allow-show`.

## Frontend features

1. **Notifications**: on a switch to `attention` while the window has no focus and setting
   `notifyDesktop` is on (default on): `notify_session`. Text: "{title} is waiting for you"
   or "{title} has finished" after an exit. `notification-activated` activates the session and
   focuses its terminal.
2. **Search** (Ctrl+Shift+F): `@xterm/addon-search` with match decorations; a small bar at the top
   right of the focused pane: field, "3 of 17", previous, next, case toggle, close. Enter next,
   Shift+Enter previous, Escape closes and returns focus to the terminal.
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
    Consolas; installed ones only), ligatures where the renderer supports them, cursor style and
    blink.
13. **Session marker**: color and emoji per session (context menu "Marker"), shown in the
    sidebar, pane header and rail; saved in the workspace.
14. **Taskbar badge**: number of waiting sessions as an overlay icon (Windows) or badge count.
15. **WebGL**: `@xterm/addon-webgl` only if a measurement under WebView2 shows a clear gain; with
    a fallback to the DOM renderer on context loss. Result in [[webgl-measurement]].
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
