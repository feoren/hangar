---
title: Hardening of process launch, command surface and update chain
type: synthesis
tags: [security, windows, unix, sessions, tauri, updater, release]
sources: ["[[cmd-launcher-measurement]]"]
created: 2026-11-15
updated: 2026-11-15
---

# Hardening of process launch, command surface and update chain

Findings of the security review of 2026-11-15 and how each one was settled. Code paths relative to
the project root; every rule has a test named below.

## Process launch (`src-tauri/src/manager.rs`, `child_env.rs`, `resolve.rs`, `proctree.rs`)

| Finding | Rule now | Test |
|---|---|---|
| Program lookup in the working folder only blocked for `.cmd`/`.bat`; then kept for every user profile, also one that starts an agent (`command = "claude"`, as `default_profiles_windows.toml` suggests) | `no_cwd_lookup(kind, source, batch, program)` decides on the resolved program: update sessions, every catalog agent, every `.cmd`/`.bat` and every program that is not an interactive shell get `NoDefaultCurrentDirectoryInExePath=1`. Only shells keep the lookup, known by file name (`is_interactive_shell`: cmd, powershell, pwsh, wsl, bash, sh, zsh, fish, nu, git-bash, with or without `.exe`). Empty value in the profile `env` still opts out | unit `only_shells_of_the_users_own_profiles_keep_the_program_lookup_in_the_folder`, `shells_are_known_by_their_file_name`; PTY `catalog_agent_does_not_search_programs_in_working_folder` and `user_profile_of_an_agent_does_not_search_programs_in_working_folder` (a copy of cmd.exe named `agentlike.exe` as a user profile's program does not run a planted `marker.bat`; cmd.exe as a shell profile does) |
| Close sent only SIGHUP to the process group (Unix) | `ProcTree::terminate_tree`: SIGHUP to the group, on Linux also to every process of the child's terminal session; after `KILL_GRACE` (2 s, on its own thread) SIGKILL to both | unit `a_program_that_ignores_sighup_is_killed_after_the_grace`, PTY `close_kills_a_tree_that_ignores_sighup` (`trap '' HUP; sleep 1000 &`) |
| The app's end skipped SIGKILL (Unix): the grace ran on a thread that died with the process | `SessionManager::shutdown` calls `proctree::end_all`: SIGHUP to every tree, at most `KILL_GRACE` in all (polling `ProcTree::alive`), then SIGKILL synchronously before `RunEvent::Exit` returns | unit `shutdown_kills_every_stubborn_tree_before_it_returns`, `shutdown_does_not_wait_for_trees_that_end_on_sighup` (cfg(unix), not run on Windows) |
| Linux session signals ignored the reaping: once every member had ended, a new session could carry the same id | `signal_session` signals after the reaping only while a member of the session started strictly before it (`/proc/<pid>/stat` field 22 against `CLOCK_BOOTTIME` in clock ticks, `stat::still_ours`); zombies do not count as alive | unit `a_reaped_session_counts_only_with_an_older_member` (every platform), `a_session_id_that_may_be_reused_is_left_alone` (cfg(linux), not run here) |
| The wait thread dropped the pseudo console while holding its mutex | `let master = lock(&master_wait).take(); drop(master);`, so a blocking ConPTY close does not hold up resize or kill | review |
| killpg after the child was reaped | The wait thread calls `mark_reaped` right after `wait`; the group is never signalled afterwards (its id may name another group) | unit `a_reaped_child_is_never_signalled_again` |
| PTY spawn error carried the whole command line | `spawn_failed`: program path plus the OS error code only (portable-pty 0.9 formats `CreateProcessW \`<cmdline>\` ... failed: <io error>`) | unit `a_failed_start_names_the_program_and_the_code_only` |
| `write_session` unbounded | Input queue `sync_channel(16)`, `try_send`; full queue is `Busy`, more than 1 MiB per call is `InputTooLarge`. The command answers `false` for `Busy`; `src/sessionInput.ts` splits a paste into pieces of 256 Ki UTF-16 units (at most 768 KiB UTF-8, never a surrogate pair cut), retries with backoff for up to 30 s and keeps the order per session. After 30 s the rest is dropped with `InputStalled`: a dropped end of a bracketed paste is still sent (`ESC[201~`) when its start had arrived, writes of the same session queued before the stall was known are dropped with it (`inputQueue`, one notice, so a key is never lost while the Enter after it arrives alone), the terminal shows a translated notice and "Send to several" sends no Enter | PTY `input_is_bounded`, Vitest `sessionInput.test.ts`, `broadcast.test.ts` |
| Resize unbounded | Clamped to 4096 x 2048 (and at least 1 x 1) before it reaches the PTY | unit `terminal_sizes_are_clamped`, PTY `huge_sizes_are_clamped` |
| Update sessions ran in the profile's fixed folder | `updates::update_agent` drops the folder: updates always run in the home folder, where no foreign `.npmrc` or `pip.conf` steers npm or pip | unit in `update_agent_runs_only_the_plan`, PTY `update_session_does_not_search_programs_in_working_folder` |
| Editor variables reached every child | Removed before the profile `env`: `VSCODE_IPC_HOOK_CLI`, `CLAUDE_CODE_SSE_PORT` (session markers), every `VSCODE_GIT_*`, and `GIT_ASKPASS` when it points into an editor's git extension (`/extensions/git/dist/askpass`) | `child_env` tests |
| Network or device folder as session folder | `is_network_or_device`: `\\host\share`, `\\?\UNC\`, `\\.\`, `\\?\` other than a drive are refused before the folder is touched (asking a share whether a folder exists already authenticates against its host). Also for `create_worktree`. The error is serialized as `[remote_cwd] ...` (a missing folder as `[bad_cwd] ...`), which `src/backendError.ts` shows in the user's language | unit `network_and_device_folders_are_recognized`, `a_network_folder_is_refused_before_it_is_touched`, `folder_errors_carry_a_stable_code_for_the_window`, Vitest `backendError.test.ts` |
| The folder check was textual: a symlink or junction to a share passed | `manager::check_cwd` uses `no_links::folder_place`: share or device text, then a walk that follows only links to local fixed drives (`Links::FollowLocal`); a link off the machine is `[remote_cwd]`. A path on a mapped network drive named directly (`Z:\work`) stays allowed: the user mapped it, Windows authenticated against its host then, so a lookup there hands over nothing new, and sessions on such drives worked before; a link that leads to a mapped drive is still refused. `git::check_folder` (activity, worktrees, shared notes, cleanup) and `create_worktree` use the same place check; `git_branches` and `git_repo_places` refuse share and device paths by their text before reading `.git` | unit `folders_are_placed_without_leaving_the_machine`, `links_to_network_paths_are_refused_without_connecting`, `working_folders_must_be_local_folders`, `share_folders_are_refused_before_they_are_touched` (git, project notes), `share_folders_are_skipped_without_touching_them` (activity) |
| Shared notes ran git in any folder the panel named | `ProjectNotes::folder_path` refuses relative paths and share or device paths (`\\host\share`, `//host/share`, `\\?\UNC\...`) before `locate` touches them | unit `share_folders_are_refused_before_they_are_touched` (project notes) |
| `HANGAR.md` text with `{file}` or `{text}` was filled in a second time | `instruction_args` and `prompt_args` replace every placeholder in one pass from left to right; a value is never scanned again | unit `placeholders_inside_the_text_stay_as_they_are` |
| JetBrains IDEs' askpass helpers reached every child | `GIT_ASKPASS` and `SSH_ASKPASS` are removed when their file name starts with `intellij-git-askpass` or `intellij-ssh-askpass`, like VS Code's git extension helper | unit `jetbrains_askpass_helpers_are_found` |
| Folders and Hangar's own files opened through explorer.exe's command line, which splits paths at commas | `reveal_session_folder` and `reveal_notes_folder` use `desktop::open_folder`, `open_profile_file` and `open_hangar_md` use `desktop::open_text_file` (`ShellExecuteW` "open", and "openas" when no program is associated, `SE_ERR_NOASSOC`); `desktop::open` is private and unused on Windows | unit `only_a_missing_association_asks_for_a_program`, static `folders_and_files_never_open_through_explorer_arguments` |
| Relative program path with a separator | `resolve_program`/`resolve_all` accept a path only when it is absolute, as `updates::split_command` already did | unit `relative_program_paths_are_refused` |
| NUL in profile `env` values | Refused by `profile::parse` (and with it by profile editing) | `env_values_with_nul_are_refused`, `an_env_value_with_nul_is_refused` |
| Package names could start with `-` | npm (each scope part), PyPI and GitHub names start with a letter or digit | `package_names_start_with_a_letter_or_digit` |

### Job Object assigned after the spawn (Windows): documented, not changed

portable-pty 0.9 offers no `CREATE_SUSPENDED`, so `ProcTree::attach` assigns the child to its Job
Object a moment after `CreateProcessW`. A process the child starts within that window is not in the
job and survives the session. Not changed, because:

- A Toolhelp snapshot after the assignment would itself race (a grandchild started between the
  snapshot and its own assignment escapes the same way) and would need recursion over every level.
- The window is the time between `CreateProcessW` returning and `AssignProcessToJobObject`, a few
  system calls; agent CLIs spend hundreds of milliseconds starting their runtime (node, Python)
  before they start anything.

> [!question] Unverified
> The length of the window has not been measured. A real fix needs a launcher of Hangar's own that
> starts the program suspended, assigns it and resumes it.

## Command surface (`src-tauri/src/commands.rs`, `capabilities/`, `tauri.conf.json`)

- **Events reach every window that may listen.** Tauri 2.12 delivers an event emitted with
  `emit_to("main", ...)` also to every JS listener registered for the target `Any`, in any webview
  (`match_any_or_filter` in `tauri/src/event/listener.rs`), and `@tauri-apps/api/event` `listen`
  registers exactly such listeners. Consequences:
  - The file viewer windows lost `core:event:allow-listen` and `allow-unlisten`; they never listen.
  - `gate-changed` carries no payload any more; the main window reads `gate_snapshot` (main window
    only) on each signal, and only the answer to the newest read counts (`src/useGate.ts`).
  - `team-changed` and `account-changed` are payload-free signals as well since the team security
    review; the main window reads `team_snapshot` and `account_status`.
  - `claims-changed` and `program-update-result` are payload-free signals since the second
    review: the main window reads `claims_snapshot` and `take_update_results` (each waiting update
    result handed out once, at most 32 kept). Still with payload and readable by the panel window:
    `context-usage-changed` (token counts per session id, no paths).
- **Git reads name a session.** `git_diff`, `git_changed_count`, `git_log` (with `root` for the
  repository's main folder, as the project overview needs) and `git_commit_diff` take a `SessionId`;
  the backend finds the folder. Unknown ids answer "Unknown session N". `create_worktree` keeps its
  folder argument (no session exists yet; main window only) but refuses network and device paths.
- **Session starts off the main thread.** `create_session` and `restart_session` run on
  `spawn_blocking`, like `create_update_session`. Since the second review `close_session`, `kill_session`,
  `delete_gate_rule`, `account_open` and `team_link_repo` as well (static test
  `blocking_commands_run_off_the_main_thread`).
- **The panel addresses one session.** `git_diff`, `git_log`, `git_commit_diff`, the git actions,
  `list_session_folder`, `list_session_paths`, `reveal_session_folder` and `open_file_viewer`
  answer the panel window only for the session the main window last handed it
  (`set_panel_session`, main window only, called before every `panel-context`); any other id is
  "Unknown session N". The main window may name any session (`PanelScope`, unit
  `the_panel_addresses_only_the_session_it_was_handed`, static
  `panel_session_commands_check_the_handed_session`).
- **Main window state out of `localStorage`.** The panel and file viewer windows share the main
  window's origin and with it `localStorage`. The security switches (`claimsEnabled`,
  `gateEnabled`, `commandGuardEnabled`; the gate needs the claims, so all three) and the queued
  messages live in the backend (`main_store.rs`: `security-switches.json` and
  `queued-messages.json` in the local app data folder, atomic writes, validated both ways, a
  damaged file gives the safe defaults). The commands `security_switches`,
  `save_security_switches`, `load_message_queue` and `save_message_queue` are in `default.json`
  only and check the caller's label (`check_main`). The main window migrates the old
  `localStorage` values once, while the backend has nothing stored, then removes them there and
  never reads them again (`src/mainStore.ts`; an empty queue is stored too, so a later write to the old key is never migrated); `parseSettings` ignores the switches and
  `saveSettings` leaves them out. The gate and the command guard are configured only once the
  switches are read, and the restore waits for that (`gateReady`). Remaining: a viewer that was
  compromised in an older version before the update could have written values that the one
  migration takes over. Tests: `main_store` units, Vitest `mainStore.test.ts`,
  `settings.test.ts` ("never reads the security switches from localStorage").
- **The file viewer's own storage (investigated, not built).** Tauri 2.12.5 offers
  `WebviewWindowBuilder::incognito` and `data_directory`; wry 0.57.1 maps `incognito` on Windows
  to `ICoreWebView2ControllerOptions::SetIsInPrivateModeEnabled` in the same WebView2
  environment (cheap: no second browser process), while another `data_directory` needs a WebView2
  environment and browser process of its own. An InPrivate viewer would no longer see the main
  window's `localStorage` at all, but it reads language and theme from there
  (`src/FileViewerWindow.tsx`); those would first have to come through the URL query or a viewer
  command. Not built in this round because that file belonged to parallel work; recommended as
  the next step. The panel window cannot go InPrivate: it shares its window prefs with the main
  window through `localStorage` and the `storage` event.
- **CSP** adds `object-src 'none'; base-uri 'self'; form-action 'none'; frame-ancestors 'none'`;
  `default-src` does not cover the last three.

Tests: `src-tauri/tests/security_config.rs`, `src-tauri/tests/contract.rs`
(`file_viewer_capability_is_narrow`), `commands::tests::git_reads_refuse_an_unknown_session`.

## Update chain

- `plugins.updater.requireSignedVersion: true` (key verified in tauri-plugin-updater 2.14.1
  `config.rs`, camelCase): the version in the signed trusted comment must match the version
  `latest.json` announces, so an old validly signed installer cannot be offered as a new version.
  `tools/release.py` already signs with `--app-version`; signatures without a version are refused.
- `tools/release.py` checks after signing that the key id in the `.sig` (bytes 2..10 of the decoded
  signature line) equals the key id of the `pubkey` in `tauri.conf.json`, and aborts and deletes the
  signature otherwise. Tests with throwaway key material under `tests/.tmp`.
- The Tauri CLI runs as `node.exe node_modules/@tauri-apps/cli/tauri.js ...`, never through
  `pnpm.cmd`, so no argument passes cmd.exe; a `node` that resolves to a `.cmd`/`.bat` is refused.
