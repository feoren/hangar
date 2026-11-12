---
title: Showing and running program updates (contract)
type: synthesis
tags: [updates, contract, network]
sources: ["[[update-sources]]", "[[update-installers]]", "[[github-conditional-requests]]", "[[app-execution-alias]]"]
created: 2026-11-07
updated: 2026-11-12
---

# Showing and running program updates (contract)

Backend: `src-tauri/src/updates.rs` (versions, payload), `src-tauri/src/install_source.rs` (where a
copy came from and which command updates it), commands in `src-tauri/src/commands.rs`. Values per
agent: [[updates]]. Evidence for the installers: [[update-installers]].

## Command
`check_program_updates({ force?: boolean, redetect?: boolean })` returns `Record<profileId, UpdateInfo>` with

```ts
type UpdateInfo = {
  installed: string | null;
  latest: string | null;
  updateAvailable: boolean;
  checkedAt: number;
  plan: "run" | "copy" | "none";
  reason: Reason | null;          // why there is no "run" plan, stable code
  needsAdmin: boolean;            // machine-wide install, the update asks for admin rights
  command: string | null;         // display text: the command that runs, or the text to copy
  installer: Installer | null;    // how the started copy was installed, if known
  warnings: ("second-copy" | "running")[];
  copies: string[];               // every copy found, the started one first
  planId: string;                 // fingerprint of the plan (16 hex), see create_update_session
  updateHint: string | null;      // = command, kept for older frontends
  canRun: boolean;                // = plan === "run", kept for older frontends
};
type Reason = "invalid-hint" | "not-installed" | "foreign-launcher" | "ambiguous"
  | "unknown-installer" | "no-package" | "package-mismatch" | "unsupported" | "tool-missing"
  | "store-managed";
type Installer = "npm" | "pnpm" | "bun" | "winget" | "registered" | "scoop" | "choco" | "uv"
  | "pipx" | "brew" | "brew-cask" | "cargo" | "native";
```

- Only installed entries with `latest`; everything else is absent from the result.
- `installed` comes from the `detect_versions` cache (ADR [[0003-program-versions]]). While that cache
  is still empty, the command queries the versions itself, with up to 5 s per program.
  `redetect: true` asks these programs again even when the cache has an answer, and the cache
  keeps the new one: the file stamp misses an agent that updated itself behind an unchanged
  launcher, so without it a stale `installed` would keep an offer that is no longer true.
- `latest` is `null` if the lookup failed (no network, timeout, rate limit, unknown
  package); `updateAvailable` is then `false`.
- `checkedAt`: Unix time in ms of the lookup that produced `latest`.
- `plan: "run"`: the frontend offers "Update" (one click). `"copy"`: `command` is shown for
  copying only (the install command, a planned command whose tool is missing, or an invalid
  `update_hint`). `"none"`: nothing to offer. The install command never makes a `run` plan.
- `warnings`: `running` when the program runs in an agent session (Windows may fail on locked
  files), `second-copy` when more than one copy exists (`copies`); the update may then reach a
  copy that is not the one started.
- `force: true` bypasses the disk cache, but never a rate limit pause (see Behavior). Only sensible
  when the user asks for it (GitHub: 60 requests per hour and IP).

`create_update_session({ profileId, size, onEvent, expectedPlan? })` starts the update as a
normal session and returns its `SessionInfo` with `kind: "update"` and the title
"Update: <Name>". The command deliberately takes no working folder and is async (detection reads
files and the uninstall keys off the main thread). `expectedPlan` is the `planId` the user saw;
the backend plans again and refuses with "The update of <Name> changed since it was shown; check
for updates again" (`PlanChanged`) if the fingerprint differs, so nothing runs that was not shown
(another copy came first on PATH, a profile was edited). Optional for older frontends. Other
errors (text, English): unknown profile, no update command known (`Missing`), "Hangar cannot run
the update itself (<Reason>)" (`NoPlan`), invalid `update_hint`, an update of this program is
already running, program not found, working folder missing.

The first output of every update session (also of a restart) is the exact argv, written by the
backend before the program starts: `ESC[2m<absolute program> <args…>ESC[0m\r\n` (dimmed, parts
with spaces in quotes, control characters removed). It passes the same flow control as program
output.

`planId` covers the kind and every part of the command (SHA-256 over length-prefixed parts, first
8 bytes as hex); it is no secret and no signature, only a check that the shown and the started
command are the same.

### Event `program-update-result`
After an update session exits (also after a restart of it), the backend clears the version cache,
sends `exit`, then reads the version again on a thread of its own and emits

```ts
type UpdateOutcome = {
  profileId: string;
  sessionId: number | null;   // null only if the process ended before the start returned
  code: number | null;        // exit code of the update
  before: string | null;      // version remembered at the start (null if never queried)
  after: string | null;
  changed: boolean | null;    // null if one of the two is unknown
  reason: "running" | "failed" | "shadowed" | "unchanged" | null;
  copies: string[];
};
```

`reason` is `null` when the version changed, and when the update exited with 0 but one of the
two versions is unknown (nothing to compare, so no cause is claimed). Otherwise, in this order: `running` (the program
still runs in a session), `failed` (nonzero exit code), `shadowed` (another copy exists; the
update probably reached a copy that is not the started one, or another copy is earlier on PATH),
`unchanged`.

## Update plan (`install_source::plan`)
Order: (1) the profile's `update_hint` for the platform, an explicit override; (2) an
installation-aware self-updater (`self_update` with `aware = true`, run as the resolved absolute
program with its `args`; `aware = false` only for a native install); (3) the detected installer's
command with the catalog package name; otherwise no run plan, with `reason`.

Detection uses the resolved path and file existence only, no process ([[update-installers]]):

| Installer | Evidence | Command (tool resolved absolutely) |
|---|---|---|
| npm | `node_modules/<npm>` next to the shim (Windows), `<prefix>/lib/node_modules/<npm>` next to `<prefix>/bin/<command>` (Unix), or a real path `<prefix>/lib/node_modules/<npm>/...` when `<prefix>/bin/<command>` exists; the prefix keeps the path's own case and separators | `npm install -g --prefix <prefix> <npm>@latest`, npm from the program's own folder, `Program Files\nodejs` or PATH, never from a `bin` folder that is not the program's |
| pnpm | shim in `PNPM_HOME`, package below `global/` | `pnpm add -g <pnpm or npm>@latest` |
| winget | path below `WinGet\Packages\<Id>_<source>` or `WinGet\Links` (user or Program Files) | `winget upgrade --id <winget> --exact --source winget`; the folder id must equal the catalog id |
| registered | uninstall key whose `InstallLocation` contains the path (HKLM = machine), else a folder below Program Files | the same winget command |
| scoop | below `<scoop>\apps\<app>` or `shims` | `scoop update <scoop>` |
| uv | real path below the uv tool dir, or `uv-receipt.toml` of the catalog package naming the path | `uv tool upgrade <pypi>` |
| pipx | real path below `venvs/<pkg>`, or `venvs/<pypi>/pipx_metadata.json` with the shim in the pipx bin dir | `pipx upgrade <pypi>` |
| brew, brew-cask | macOS and Linux only: `Cellar/<formula>` or `Caskroom/<cask>` in the real path below `HOMEBREW_PREFIX`, `/opt/homebrew`, `/usr/local` or `/home/linuxbrew/.linuxbrew` | `<prefix>/bin/brew upgrade [--cask] <name>` |
| bun, choco, cargo | `~/.bun/bin`; `chocolatey\bin` or `\lib`; `~/.cargo/bin` plus `.crates2.json` | none (`unsupported`) |
| native | Claude Code: `~/.local/bin` plus `~/.local/share/claude`; Codex: `Programs\OpenAI\Codex` | the self-updater |
| store | an execution alias (AppExecLink reparse point, [[app-execution-alias]]) or a path below `<Program Files>\WindowsApps\<package>`; checked before every other rule, because the package lies below Program Files | none (`store-managed`): the Microsoft Store updates it; no winget command, no administrator rights, no self-updater, no copy text; an explicit `update_hint` still wins |

- More than one match (uv receipt and pipx venv for the same shim) is `ambiguous`; nothing runs.
- A bin folder alone never decides. A launcher of another program (`copilot-shim` of VS Code,
  `bash.exe` in `System32` or `WindowsApps`, which starts WSL) is a foreign launcher: catalog
  entries skip it on PATH and count as not installed if nothing else is found.
- Uninstall entries that claim a drive root, the Windows folder or a shared root are ignored
  (measured on 2026-11-08: an entry with `C:\Windows\System32\`). MSI packages often leave
  `InstallLocation` empty (measured: PowerShell 7.6.9), hence the Program Files fallback.
- Tools (npm, winget, uv, pnpm, pipx, scoop) are looked up in known folders first (npm in the
  prefix, winget in `WindowsApps`, uv in `~/.local/bin` and `~/.cargo/bin`), then in absolute PATH
  entries only. brew is only the prefix's own. uv only counts from the standalone installer's
  folders (`UV_INSTALL_DIR`, `XDG_BIN_HOME`, `~/.local/bin`), from `~/.cargo/bin`, or on PATH when
  winget, Scoop, Homebrew, pipx or cargo installed it as the package `uv`: the live run found only
  a uv bundled inside another application's folder, which is never run (`tool-missing`, copy
  text instead). Missing tool: `copy` with the same command and the
  bare tool name, `reason: "tool-missing"`.
- Package names are validated when catalog and profiles load (npm, PyPI, winget id with a dot and
  only `A-Z a-z 0-9 . - _ +`, Homebrew lower case, Scoop) and again before building a command.
- `needsAdmin`: the detected install is machine scope (Program Files, ProgramData, uninstall key in
  HKLM; on Unix below `/usr` except `/usr/local`, or `/snap`). Also set for an `update_hint`
  override of such an install.
- Git Bash (`<root>\bin\bash.exe` or `<root>\usr\bin\bash.exe` next to `<root>\cmd\git.exe`) is
  Git for Windows: version via that `git.exe --version`, source GitHub `git-for-windows/git`
  (tags like `v2.56.0.windows.1`), winget `Git.Git`. Versions `X.Y.Z.windows.N` compare as
  `X.Y.Z.N`, as winget lists them.
- Live check on this machine (2026-11-08, read only): Claude Code native (`claude update`), Codex
  npm with a second native copy (`codex update`, warning `second-copy`), aider uv
  (`uv tool upgrade aider-chat` as copy text with `tool-missing`, because the only uv on PATH is
  bundled by another application), Git Bash and PowerShell 7 registered machine-wide (winget,
  `needsAdmin`), the VS Code `copilot` shim not installed.

## Running an update (security rules)
- The frontend never sends command text, only the profile ID. The backend runs only the plan:
  the platform's `update_hint`, a self-updater from catalog or profile data, or an installer
  command from validated package names and absolute paths found on this machine; never
  `install_*`: install commands are often shell pipelines (`irm … | iex`, `curl … | bash`) and
  meant for reading only.
- Splitting: on whitespace into an argv (`updates::split_command`). Rejected without launching: empty,
  control characters (except tab) and shell syntax `` | & ; < > $ ` ( ) " ' % ^ ! * ? [ ] { } # ~ ``.
  `%`, `^` and `!` are on the list because launcher scripts such as `npm.cmd` run through cmd.exe on
  Windows. `argv[0]` must be a bare name or an absolute path; a relative path containing `/` or `\`
  would depend on the working folder.
- The check prevents wrong splitting and evaluation by cmd.exe; it is not a sandbox.
  `update_hint` is as trustworthy as `command`: whoever can write the catalog or profiles.toml already
  decides what Hangar launches.
- Launched like every session through `spawn` in `manager.rs`: `argv[0]` of an `update_hint` via
  PATH lookup (`resolve_program`, finds `npm.cmd`, `winget.exe`; only absolute PATH entries count,
  a relative one would depend on Hangar's working folder), planned tools already absolute,
  the remaining parts as arguments, no shell. `.cmd` tools (npm, pnpm, scoop) go through the
  cmd.exe checks of [[cmd-launcher]]. The profile's environment
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
  locked files on Windows (`warnings: ["running"]`). There is no confirmation prompt.

## Behavior
- Disk cache `update-cache.json` in the local app data folder, keyed per source (`npm:<package>`,
  `pypi:<package>`, `github:<repo>:<prefix>`): hits stay valid for 25 min, failures for 15 min
  (`CACHE_TTL_MS`, `FAILURE_TTL_MS`), so each 30 min check of the frontend really asks.
- Schedule (frontend `useProgramUpdates`): first check 1.5 s after the startup version query
  (`redetect: false`), then a one minute tick compares wall-clock times and checks once the last
  attempt is 30 min old (`isCheckDue`, `UPDATE_CHECK_INTERVAL_MS`); window `focus` and
  `visibilitychange` use the same rule, so returning after sleep checks at once and returning
  after five minutes sends nothing. Periodic checks and "Check now" pass `redetect: true`; the
  frontend then reloads `detect_versions` (from the refreshed cache) for the version shown in
  the rows. The sidebar row and Settings render the offer from this state, so "Update" appears
  as soon as a check returns.
- Conditional requests: the entity tag of a successful answer is stored with the entry (`etag`)
  and sent back as `If-None-Match`; a `304` keeps `latest` and moves `checkedAt` to now. Only an
  entry with a known version sends a tag. Measured 2026-11-12: GitHub and PyPI answer `304`,
  npm `/latest` sends no `ETag` (every check is a full, small answer). Unauthenticated, a GitHub
  `304` still counts against the 60 per hour ([[github-conditional-requests]]); the gain there is
  bandwidth, the budget is kept by the 30 min interval (two requests per GitHub source per hour).
- Rate limits: a `403` or `429` with `Retry-After` (seconds), or with `X-RateLimit-Remaining: 0`
  and `X-RateLimit-Reset` (epoch seconds), or any `429`, pauses that source (`retryAt`) for at
  least 1 min and at most 6 h; the known `latest` stays and keeps the offer visible. A plain
  `403` is an ordinary failure. The pause applies per source, also to `force`.
- Requests: HTTPS only, to `registry.npmjs.org`, `pypi.org`, `api.github.com` (redirects included,
  at most 3), timeout 5 s, at most 4 at once, response at most 8 MiB, system proxy.
- Sent are the address, `User-Agent: Hangar/<version>` and, after a first answer, that source's
  own entity tag as `If-None-Match`; no cookies, no account, no ID of Hangar's own. The tag is
  chosen by the server: one that handed each client a tag of its own could recognize a returning
  client by it, as it can by the IP address anyway. The measured tags of GitHub and PyPI look like
  content hashes ([[github-conditional-requests]]); whether they are the same for every client is
  not verified.
- Package and repository names are validated when the catalog and profiles are loaded
  (allowed characters only), so that nothing foreign ends up in the address.
- The frontend never calls any of these addresses itself; capabilities and CSP stay unchanged.

> [!question] Unverified
> pnpm global layout (`<PNPM_HOME>/global/5/node_modules`), pipx folders on Windows, the native
> Claude Code marker `~/.local/share/claude` on every platform, whether `winget upgrade` needs UAC
> for machine installs and its exit codes, whether `codex update` and `pi update` handle npm
> installs, the winget id `astral-sh.uv` and the standalone uv installer folders
> (`UV_INSTALL_DIR`, `XDG_BIN_HOME`, `~/.local/bin`), and whether ConPTY starts the `WindowsApps\winget.exe` execution alias by absolute
> path the same way as by name (an `update_hint` resolved to the same file before).
