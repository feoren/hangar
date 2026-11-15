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
| Program lookup in the working folder only blocked for `.cmd`/`.bat` | `no_cwd_lookup(kind, source, batch)`: update sessions, every catalog agent and every `.cmd`/`.bat` get `NoDefaultCurrentDirectoryInExePath=1`; only user profiles (the shells) keep the lookup. Empty value in the profile `env` still opts out | unit `only_the_users_own_profiles_keep_the_program_lookup_in_the_folder`; PTY `catalog_agent_does_not_search_programs_in_working_folder` (a planted `marker.bat` does not run for a catalog agent, runs for a shell profile) |
| Close sent only SIGHUP to the process group (Unix) | `ProcTree::terminate_tree`: SIGHUP to the group, on Linux also to every process of the child's terminal session; after `KILL_GRACE` (2 s, on its own thread) SIGKILL to both | unit `a_program_that_ignores_sighup_is_killed_after_the_grace`, PTY `close_kills_a_tree_that_ignores_sighup` (`trap '' HUP; sleep 1000 &`) |
| killpg after the child was reaped | The wait thread calls `mark_reaped` right after `wait`; the group is never signalled afterwards (its id may name another group) | unit `a_reaped_child_is_never_signalled_again` |
| PTY spawn error carried the whole command line | `spawn_failed`: program path plus the OS error code only (portable-pty 0.9 formats `CreateProcessW \`<cmdline>\` ... failed: <io error>`) | unit `a_failed_start_names_the_program_and_the_code_only` |
| `write_session` unbounded | Input queue `sync_channel(16)`, `try_send`; full queue is `Busy`, more than 1 MiB per call is `InputTooLarge`. The command answers `false` for `Busy`; `src/sessionInput.ts` splits a paste into pieces of 256 Ki UTF-16 units (at most 768 KiB UTF-8, never a surrogate pair cut), retries with backoff for up to 30 s and keeps the order per session. After 30 s the rest is dropped with `InputStalled`: a dropped end of a bracketed paste is still sent (`ESC[201~`), the terminal shows a translated notice and "Send to several" sends no Enter | PTY `input_is_bounded`, Vitest `sessionInput.test.ts`, `broadcast.test.ts` |
| Resize unbounded | Clamped to 4096 x 2048 (and at least 1 x 1) before it reaches the PTY | unit `terminal_sizes_are_clamped`, PTY `huge_sizes_are_clamped` |
| Update sessions ran in the profile's fixed folder | `updates::update_agent` drops the folder: updates always run in the home folder, where no foreign `.npmrc` or `pip.conf` steers npm or pip | unit in `update_agent_runs_only_the_plan`, PTY `update_session_does_not_search_programs_in_working_folder` |
| Editor variables reached every child | Removed before the profile `env`: `VSCODE_IPC_HOOK_CLI`, `CLAUDE_CODE_SSE_PORT` (session markers), every `VSCODE_GIT_*`, and `GIT_ASKPASS` when it points into an editor's git extension (`/extensions/git/dist/askpass`) | `child_env` tests |
| Network or device folder as session folder | `is_network_or_device`: `\\host\share`, `\\?\UNC\`, `\\.\`, `\\?\` other than a drive are refused before the folder is touched (asking a share whether a folder exists already authenticates against its host). Also for `create_worktree`. The error is serialized as `[remote_cwd] ...` (a missing folder as `[bad_cwd] ...`), which `src/backendError.ts` shows in the user's language | unit `network_and_device_folders_are_recognized`, `a_network_folder_is_refused_before_it_is_touched`, `folder_errors_carry_a_stable_code_for_the_window`, Vitest `backendError.test.ts` |
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
  - Still with payload and readable by the panel window: `claims-changed` and
    `context-usage-changed`. Their emitters and hooks belong to other work; the same
    change (signal plus a main-only snapshot command) applies to them.
- **Git reads name a session.** `git_diff`, `git_changed_count`, `git_log` (with `root` for the
  repository's main folder, as the project overview needs) and `git_commit_diff` take a `SessionId`;
  the backend finds the folder. Unknown ids answer "Unknown session N". `create_worktree` keeps its
  folder argument (no session exists yet; main window only) but refuses network and device paths.
- **Session starts off the main thread.** `create_session` and `restart_session` run on
  `spawn_blocking`, like `create_update_session`.
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
