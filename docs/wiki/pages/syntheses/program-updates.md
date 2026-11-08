---
title: Showing and running program updates (contract)
type: synthesis
tags: [updates, contract, network]
sources: ["[[update-sources]]"]
created: 2026-11-07
updated: 2026-11-08
---

# Showing and running program updates (contract)

Backend: `src-tauri/src/updates.rs`, command in `src-tauri/src/commands.rs`. Values per agent: [[updates]].

## Command
`check_program_updates({ force?: boolean })` returns
`Record<profileId, { installed: string | null, latest: string | null, updateAvailable: boolean,
updateHint: string | null, canRun: boolean, checkedAt: number }>`.

- Only installed entries with `latest`; everything else is absent from the result.
- `installed` comes from the `detect_versions` cache (ADR [[0003-program-versions]]). While that cache
  is still empty, the command queries the versions itself, with up to 5 s per program.
- `latest` is `null` if the lookup failed (no network, timeout, rate limit, unknown
  package); `updateAvailable` is then `false`.
- `checkedAt`: Unix time in ms of the lookup that produced `latest`.
- `updateHint`: the platform's `update_hint`, otherwise the install command; for display and
  copying.
- `canRun`: the platform's `update_hint` exists and can be split into an argv (see below).
  Only then does the frontend offer "Update"; otherwise copying remains. The install command
  never makes `canRun` true.
- `force: true` bypasses the disk cache. Only sensible when the user asks for it (GitHub: 60 requests
  per hour and IP).

`create_update_session({ profileId, size, onEvent })` starts the update as a normal session
and returns its `SessionInfo` with `kind: "update"` and the title "Update: <Name>". The command
deliberately takes no working folder. Errors (text, English): unknown profile, missing or invalid
`update_hint`, an update of this program is already running, program not found, working folder missing.

## Running an update (security rules)
- The frontend never sends command text, only the profile ID. The backend takes only the
  platform's `update_hint` (`PlatformHint::for_platform`), never `install_*`: install commands
  are often shell pipelines (`irm … | iex`, `curl … | bash`) and meant for reading only.
- Splitting: on whitespace into an argv (`updates::split_command`). Rejected without launching: empty,
  control characters (except tab) and shell syntax `` | & ; < > $ ` ( ) " ' % ^ ! * ? [ ] { } # ~ ``.
  `%`, `^` and `!` are on the list because launcher scripts such as `npm.cmd` run through cmd.exe on
  Windows. `argv[0]` must be a bare name or an absolute path; a relative path containing `/` or `\`
  would depend on the working folder.
- The check prevents wrong splitting and evaluation by cmd.exe; it is not a sandbox.
  `update_hint` is as trustworthy as `command`: whoever can write the catalog or profiles.toml already
  decides what Hangar launches.
- Launched like every session through `spawn` in `manager.rs`: `argv[0]` via PATH lookup (`resolve_program`,
  finds `npm.cmd`, `winget.exe`), the remaining parts as arguments, no shell. The profile's environment
  (`env`) applies here too; `TERM`, `COLORTERM` and the removal of inherited session markers work as for agents.
- Working folder: the profile's fixed folder, otherwise the home folder, never the one selected
  in the sidebar. Reason (measurement from the security review, 2026-11-08): `npm.cmd` calls `node`
  through cmd.exe, and cmd.exe looks for programs in the current folder first; a foreign project with
  its own `node.cmd` would otherwise run along with the update.
- On Windows, Hangar additionally sets `NoDefaultCurrentDirectoryInExePath=1` for update sessions
  (before the profile `env`, which may override it), so that the program lookup does not search the
  working folder at all. Proven by the PTY test
  `update_session_does_not_search_programs_in_working_folder` (`src-tauri/tests/pty.rs`): a `probe.cmd`
  in the working folder starts in a shell session (cmd.exe) but not in an update session. Agent sessions
  with a `.cmd` or `.bat` target get the variable as well, see [[cmd-launcher]].
  > [!warning] Environment during testing
  > Claude Code already sets the variable in its own shells; the test therefore removes it
  > first, otherwise the counter check would prove nothing.
- The user sees the output, can answer prompts from winget or npm and sees the exit code.
- Restarting an update session (`restart_session`) runs the update again, not the agent.
- At most one update per program: if an update session for it is already running (or starting), the
  frontend shows it instead of starting a second one (`sessions.runningUpdateId`). The backend also
  rejects `create_update_session` and the restart of an update session as long as an update session
  for the same `profileId` exists with a status other than `exited` (`ManagerError::UpdateRunning`,
  checked under the same lock as the registration).
- The frontend replaces the backend title "Update: <Name>" with the translation
  (`update.sessionTitle`).
- Update sessions are not added to the saved workspace (frontend `workspace.restorable`),
  and their scrollback is not saved.
- After the exit, the backend clears the version cache (`ProbeCaches.versions`) before the `exit` event.
  This is needed because the cache is keyed by the file timestamp of the launcher script, and an npm
  update does not touch `gemini.cmd`. The frontend then queries `detect_versions` and
  `check_program_updates` (without `force`) again.
- If the program is currently running in a session, the UI points out that the update may fail on
  locked files on Windows. There is no confirmation prompt.

## Behavior
- Disk cache `update-cache.json` in the local app data folder, keyed per source (`npm:<package>`,
  `pypi:<package>`, `github:<repo>:<prefix>`): hits stay valid for 24 h, failures for 1 h.
- Requests: HTTPS only, to `registry.npmjs.org`, `pypi.org`, `api.github.com` (redirects included,
  at most 3), timeout 5 s, at most 4 at once, response at most 8 MiB, system proxy.
- Only the address and `User-Agent: Hangar/<version>` are sent; no identifier, no cookies.
- Package and repository names are validated when the catalog and profiles are loaded
  (allowed characters only), so that nothing foreign ends up in the address.
- The frontend never calls any of these addresses itself; capabilities and CSP stay unchanged.
