---
title: "Agent gate adapters: Codex, Gemini, Copilot, Qwen, Pi, Oh My Pi, OpenCode and Droid"
type: synthesis
tags: [gate, hooks, team, codex, gemini, copilot, qwen, pi, omp, opencode, droid, catalog, helper]
sources: ["[[agent-hooks-per-session-research]]", "[[agent-hooks-remaining-agents-research]]", "[[gate-hook-helper-measurement]]", "[[codex-gate-live-measurement]]", "[[opencode-gate-live-measurement]]", "[[pi-gate-live-measurement]]", "[[claude-http-hook-measurement]]"]
created: 2026-11-12
updated: 2026-11-12
---

# Agent gate adapters: Codex, Gemini, Copilot, Qwen, Pi, Oh My Pi, OpenCode and Droid

How Hangar's agent gate ([[team-service-v1]] step B) reaches one session of each agent CLI, so
that the CLI can be a team session ([[0008-private-and-team-sessions]]). The recipes follow
[[agent-hooks-per-session-research]] section 5 and [[agent-hooks-remaining-agents-research]]
section 2; [[gate-hook-helper-measurement]] settled the Windows start of the helper and the Codex
`-c` value. Code: `src-tauri/src/hook_recipes.rs` (recipes, pure), `gate_plugins/*.js` (the
in-process extension and plugin), `gate_service.rs` (install, folders, cleanup), `gate_hook.rs`
(helper), `agent_catalog.rs` (`HookFormat`), `agent_catalog.toml` (flags).

> [!note] What is offered
> Team-capable besides Claude Code: **Codex CLI** from 0.167.2 ([[codex-gate-live-measurement]]),
> **OpenCode** from 1.19.5 ([[opencode-gate-live-measurement]]) and **Pi** from 1.5.2
> ([[pi-gate-live-measurement]]; live Pi process with a scripted model, as both stored sign-ins
> had expired). Gemini CLI, Copilot CLI, Qwen Code, Oh My Pi and Droid have their recipe built and
> tested, but `measured = false` keeps them off until each one is run live once (owner's
> decision: measure first; Oh My Pi and Droid are not installed on the measuring machine).

## Shape

- **Data-driven.** Each catalog entry names its `hook_format`; `Capabilities::gate()` needs
  `pre_edit_block`, `per_session_config`, `measured` and a format. No code names a program; a
  format is a `HookFormat` variant with its recipe.
- **Only what works on this machine.** `ProfileView.teamCapable` and the Team check of
  `create_session` both use `Capabilities::team_ready` (`manager.rs`, `team_ready`): the catalog
  data, the installed version at or above the catalog's `min_version` (from the version query of
  [[0003-program-versions]] and its cache; an unknown version does not count), and no
  administrator's system settings that the session's file would replace (Gemini CLI, Qwen Code:
  the variable already set in Hangar's own environment, or the default file present:
  `C:\ProgramData\gemini-cli\settings.json`, `/etc/gemini-cli/settings.json`,
  `/Library/Application Support/GeminiCli/settings.json`; Qwen Code
  `C:\ProgramData\qwen-code\settings.json` from its docs, `/etc/qwen-code/settings.json` and
  `/Library/Application Support/QwenCode/settings.json` by analogy [UNVERIFIED]). Floors:
  Codex 0.167.2 (measured; the first release with the bypass flag is not known), OpenCode 1.19.5
  and Pi 1.5.2 (measured), Copilot 1.0.109, Gemini 0.68.0, Qwen 0.27.2, Oh My Pi 18.19.0 (the
  versions the research read), Droid 0.174.0 (the version the CLI reference names). For OpenCode
  an `OPENCODE_CONFIG_CONTENT` already in Hangar's own environment counts like an administrator's
  settings: the session's value would take its place, so OpenCode is not offered then.
- **Variables a child Hangar strips.** `hook_recipes::GATE_ENV_NAMES` lists every variable the
  gate sets. `child_env` always removes the token variables (`HANGAR_HOOK_TOKEN`, `HANGAR_GATE`);
  when Hangar itself inherited one of them (it runs inside a gated session), it removes the
  settings and trust variables too. Without a token they are the user's or an administrator's own
  and stay for private sessions.
- **One endpoint.** Every format ends at the same loopback endpoint and the same decision logic
  as Claude Code (`/v1/pre-tool-use`). Qwen Code calls it as an HTTP hook; Gemini CLI, Copilot
  CLI, Codex CLI and Droid run the helper `hangar gate-hook <agent> <event>`, which turns their
  hook input into Claude Code's request and the answer back into their own format; Pi, Oh My Pi
  and OpenCode run Hangar's extension inside the agent, which sends the same request itself.
- **Token only in the environment.** `HANGAR_HOOK_TOKEN` for HTTP hooks (header interpolation),
  `HANGAR_GATE=<port>:<token>` for the helper and the in-process extensions (a neutral name:
  Gemini CLI may strip names with TOKEN, KEY, AUTH, and Codex's shell environment policy drops
  such names by default). No file, no argument holds it (unit tests per format read every file and
  argument); the extension file is the same text for every session.
- **Session folders.** Formats that need files write them to
  `<app local data>/gate/sessions/<port>-<session>-<n>/`, one fresh folder per process start,
  created with `create_dir` (an existing folder is skipped, never reused), files with
  `create_new`; on Unix folders 0700 and files 0600.
  `LaunchGate::ended` removes them on a thread of its own (it runs under the session lock); a
  later start of the endpoint removes folders whose port no longer listens, as for the Claude Code
  launcher settings files.
- **Arguments before the user's.** `GateLaunch.prepend` puts global options before the profile's
  arguments, because Codex's may start with a subcommand (`resume --last`).
- **Conflicts.** A profile that already sets `QWEN_CODE_SYSTEM_SETTINGS_PATH` or
  `GEMINI_CLI_SYSTEM_SETTINGS_PATH` gets no gate (`ProfileSettings`), as Claude Code with its own
  `--settings`. Copilot plugin folders add up and never conflict. A Codex profile or resume
  argument `-c`/`--config` that sets `hooks` or `hooks.PreToolUse` conflicts: it names the key of
  Hangar's layer and, coming later, would replace it [UNVERIFIED last wins, so treated as a
  conflict]. Pi `-e` extensions add up and never conflict; Oh My Pi conflicts with the user's
  `--trusted-extension` (refused next to `-e`), OpenCode with a profile's
  `OPENCODE_CONFIG_CONTENT`, Droid with the user's own `--settings`. A variable counts whether
  the profile keeps it plain (`env`) or in the keychain (`secret_env`).

## Recipe per CLI

| CLI | Format | What the start gets | Edit tools |
| --- | --- | --- | --- |
| Qwen Code | `qwen_system_settings` | env `QWEN_CODE_SYSTEM_SETTINGS_PATH=<folder>/qwen-settings.json`, `HANGAR_HOOK_TOKEN` | matcher `^(write_file\|edit)$` |
| Gemini CLI | `gemini_system_settings` | env `GEMINI_CLI_SYSTEM_SETTINGS_PATH=<folder>/gemini-settings.json`, `GEMINI_CLI_TRUST_WORKSPACE=true`, `HANGAR_GATE` | matcher `write_file\|replace` |
| Copilot CLI | `copilot_plugin` | argv `--plugin-dir <folder>/copilot-plugin` first, `HANGAR_GATE` | matcher `edit\|create\|apply_patch` |
| Codex CLI | `codex_config_flags` | argv `--dangerously-bypass-hook-trust -c hooks.PreToolUse=[...]` first, `HANGAR_GATE` | matcher `apply_patch` |
| Pi | `pi_extension` | argv `-e <folder>/hangar-gate.ts` first, `HANGAR_GATE` | `write`, `edit` in the extension |
| Oh My Pi | `omp_extension` | argv `-e <folder>/hangar-gate.ts` first, `HANGAR_GATE` | `write`, `edit`, `ast_edit` in the extension |
| OpenCode | `opencode_plugin` | env `OPENCODE_CONFIG_CONTENT={"plugin":["file:///<folder>/hangar-gate.ts"]}`, `HANGAR_GATE` | `write`, `edit`, `multiedit`, `patch`, `apply_patch` in the plugin |
| Droid | `droid_settings` | argv `--settings <folder>/droid-settings.json` first, `HANGAR_GATE` | matcher `Edit\|Create\|ApplyPatch` |

- **Qwen Code.** The settings file has Claude Code's shape: `hooks.PreToolUse` with one `http`
  hook to `http://127.0.0.1:<port>/v1/pre-tool-use/json`, `timeout` 660, header
  `Authorization: Bearer ${HANGAR_HOOK_TOKEN}`, `allowedEnvVars`; `UserPromptSubmit` to
  `/v1/user-prompt-submit/json` for context. The `/json` routes answer "no decision" with `{}`
  instead of the empty body Claude Code gets, because Qwen reads a body only as JSON; decisions
  carry `hookSpecificOutput.hookEventName`. Content type is always `application/json`.
- **Gemini CLI.** `hooks.BeforeTool`, one `command` hook named `hangar-gate`, `timeout` 660000 ms,
  command `& '<exe>' gate-hook gemini before-tool` (PowerShell on Windows) or
  `"<exe>" gate-hook gemini before-tool` (`bash -c` elsewhere).
- **Copilot CLI.** `plugin.json` (`name` `hangar-gate`, `hooks` = `hooks.json`) and `hooks.json`
  (`version` 1, `preToolUse` with `exec` = the Hangar executable, `args` =
  `[gate-hook, copilot, pre-tool-use]`, `timeoutSec` 660). No shell, so the path needs no quoting.
  Through npm's `copilot.cmd` the folder must pass the cmd.exe check, else `Launcher`.
- **Codex CLI.** `-c` value with TOML literal strings:
  `hooks.PreToolUse=[{matcher='apply_patch',hooks=[{type='command',command='<cmd>',timeout=660}]}]`
  where `<cmd>` is `"<exe>" gate-hook codex pre-tool-use` (quoted path), or the bare path when
  Codex starts through a cmd.exe launcher, which must not get `"`; a path that would need quotes
  there means `InstallPath`. A path with `(`, `)` or `@` is refused for cmd.exe even quoted. The
  matcher is unanchored for the same reason (`^` does not survive cmd.exe); no other Codex tool
  name contains `apply_patch`. The bypass flag is left out when the user's arguments carry it
  (Codex refuses it twice).
- **Pi and Oh My Pi.** One extension file (`gate_plugins/core.js` plus `pi.js`, plain JavaScript
  that is valid TypeScript, no dependency): a `tool_call` handler for `write` and `edit` (Oh My Pi
  also `ast_edit`), looked up as the tool table's own keys only (`Object.hasOwn`). It reads `path`,
  `file_path`, `filePath` and a `paths` list exactly as given (no trimming), and patch text
  (`*** Begin Patch`) only in `edit`'s `input` field (Oh My Pi's `apply_patch` mode), never in file
  content. For Pi a leading `@` is dropped and `~` is the home folder, as Pi's `resolveToCwd`
  does; relative paths are resolved against `ctx.cwd`. It sends
  `tool_name` `Write` or `Edit` with `tool_input.file_path` and `cwd` to `/v1/pre-tool-use` with
  `node:http`, not `fetch`: Node's `fetch` adds `Sec-Fetch-Mode`, which the endpoint refuses as a
  browser. A deny returns `{ block: true, reason }`; an "ask" opens Pi's own confirmation when
  there is a UI and blocks otherwise; an aborted turn aborts the request (the card goes away).
  Through npm's `pi.cmd` the file path must pass the cmd.exe check, else `Launcher`. Oh My Pi's
  hashline `edit` mode puts paths in `[PATH#TAG]` headers that the extension does not read
  [UNVERIFIED shape]; it stays unmeasured.
- **OpenCode.** The same core with `opencode.js`: a `tool.execute.before` hook for `write`,
  `edit`, `multiedit`, `patch` and `apply_patch` (measured: `write` with an absolute `filePath`).
  Paths are taken as given; patch text is read only for `patch` and `apply_patch`, from
  `patchText`, `patch` or `input`. `cwd` is the plugin context's `directory`. A deny or ask throws
  an `Error` with Hangar's reason, which OpenCode reports as the tool's failure. The plugin is
  named in `OPENCODE_CONFIG_CONTENT` as a `file://` URL (`url::Url::from_file_path`), not through
  `OPENCODE_CONFIG_DIR`: OpenCode runs an npm install of `@opencode-ai/plugin` in every
  configuration folder it starts with (measured), which reaches the registry and runs install
  scripts with the session's environment. With the inline configuration the session folder holds
  only the plugin file and the user's own plugins still load (measured).
- **Droid.** `droid-settings.json` with `hooks.PreToolUse`, matcher `Edit|Create|ApplyPatch`, one
  `command` hook `"<exe>" gate-hook droid pre-tool-use` (double quoted path, read the same by
  cmd.exe and a POSIX shell; which shell Droid uses on Windows is not documented), `timeout` 660 s.
  Whether `--settings` carries `hooks` in the interactive REPL is undocumented.

## The helper `hangar gate-hook`

- `main.rs` branches to `gate_hook::main` before Tauri, so no window, single instance lock or
  plugin starts. The release build is a GUI-subsystem program; measured, cmd.exe, PowerShell and a
  direct start wait for it and pipe stdin and stdout.
- Input per agent: Gemini CLI, Codex and Droid `tool_name`, `tool_input`, `cwd`; Copilot CLI `toolName`,
  `toolArgs` (object or JSON string; also the snake case shape of its PascalCase events). Path keys
  `file_path`, `path`, `filePath`, `absolute_path`; any string field with `*** Begin Patch` (or the
  whole `toolArgs` string) is read as a patch.
- Patch: every `*** Add File:`, `*** Update File:`, `*** Delete File:` and `*** Move to:` line, in
  order, without repeats, CRLF and indentation tolerated. Relative paths go to the endpoint with
  the hook's `cwd`; the endpoint resolves them and compares keys with `/` for `\`
  (`git::path_key`), so both separator styles match.
- Paths are collected without repeats (a set) and only up to 65. One request per path (at most
  64), one after the other; the first deny or ask decides. A call that names more than 64 files is
  not let through silently: it gets Hangar's "ask" with "touches more than 64 files" (a deny for
  Gemini and Codex).
- Answer: nothing and exit 0 for "go ahead". A deny is JSON on stdout:
  - Gemini `{"decision":"deny","reason":..}` with exit 0, because PowerShell turns a native exit 2
    into 0 (GUI-subsystem build) or 1 (console build, `pnpm tauri dev`;
    [[gate-hook-helper-measurement]]), and Gemini lets the tool run on any code but 0 and 2.
  - Codex `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny",..}}`
    with exit 0 and nothing on stderr. Measured live: this blocks the patch, while the same JSON
    with exit 2 and the reason on stderr counts as a failed hook and the patch goes ahead
    ([[codex-gate-live-measurement]]).
  - Copilot `{"permissionDecision":"deny","permissionDecisionReason":..}` with the reason on
    stderr and exit 2 (documented: exit 2 denies whatever stdout says; not measured).
  - Droid: Claude Code's `hookSpecificOutput` with `deny` (or `ask`, which Droid documents) and
    exit 0, nothing on stderr. Droid documents exit 2 and the JSON both as a block; the Codex
    measurement showed exit 2 together with JSON can fail open, so the JSON goes with exit 0 [not
    measured for Droid].
  Hangar's "ask" (queue full) stays an ask for Copilot and Droid and becomes a deny that says so
  for Gemini and Codex, which have no usable ask.
- Fails open with a line on stderr: no `HANGAR_GATE`, endpoint unreachable, a non-200 answer (an
  ended session's token gives 401), malformed input, input over 16 MiB, stdin not closed within
  the deadline (read on a thread of its own). Total wait at most 630 s, above Hangar's 600 s
  decision time and below the hooks' 660 s.

## New gate states

`GateStatus` gained `installPath` (the Hangar executable's path holds a character the agent's hook
shell would read, or needs quotes through a cmd.exe launcher) and `hookFiles` (the session folder
could not be written). The window shows both like the other states; `profileSettings` now names
"hook settings" instead of `--settings`.

## Accepted risks

- **Codex trust bypass.** `--dangerously-bypass-hook-trust` applies to the whole run: repository
  and user hooks that the user never reviewed run too. Team sessions start only in a worktree of a
  repository the user chose; a reviewed `hooks.state` trust hash would avoid it but its hash is not
  a public contract.
- **Gemini workspace trust.** `GEMINI_CLI_TRUST_WORKSPACE=true` trusts the session's worktree for
  the session (its `.gemini/settings.json`, `.env` and project hooks) without asking. Set only
  for a team session, which runs in a worktree of a repository the user chose. Child processes of
  that Gemini session inherit the variable, so a Gemini CLI started from its shell trusts its own
  folder too.
- **`HANGAR_GATE` visible to the agent.** The agent's shell commands see `HANGAR_GATE` (port and
  token), as Claude Code's see `HANGAR_HOOK_TOKEN` today. The token only lets that session ask the
  local gate about its own edits; it dies with the process.
- **System settings.** For Qwen Code and Gemini CLI the session's file would take the place of an
  administrator's system settings file; Hangar therefore offers neither as a team session where
  such a file or variable is present (see above).
- **Fail open and coverage.** Each CLI lets the tool run on a hook error or timeout; shell tools
  write files without a hook path (measured with OpenCode: after two denied writes the model wrote
  the file with PowerShell). The file watcher (claims) stays the second line.
- **In-process extensions fail open by choice.** Pi blocks a tool when a `tool_call` handler
  throws, and OpenCode when `tool.execute.before` throws, so a fail-closed gate would be easy. It
  is not used: the endpoint closes connections over its limits unanswered (8 unauthenticated, 64
  known) and answers 503, which would turn a busy moment into denied edits, and a session whose
  endpoint is gone has no way to recover. So the extension catches every error and lets the edit
  through (Pi shows a warning in its UI), like the helper and Claude Code's HTTP hook; the
  extension waits at most 630 s.
- **`OPENCODE_CONFIG_CONTENT` for the session.** Child processes of the OpenCode session inherit
  it, together with `HANGAR_GATE`, so an OpenCode started from its shell also loads the gate plugin
  and asks with the parent session's token while that session lives. It replaces an inline
  configuration of the user's own; Hangar therefore does not offer OpenCode where Hangar's
  environment or the profile already sets it.

## Measured and not measured

| Point | State |
| --- | --- |
| Helper reads stdin, answers on stdout, exit codes; round trip through cmd.exe, PowerShell and a direct start against a real endpoint | measured, `tests/gate_hook.rs` (debug build) and [[gate-hook-helper-measurement]] (GUI-subsystem probe) |
| Codex accepts and registers the `-c` value, also through `codex.cmd` | measured with 0.167.2 |
| Codex runs the hook on a live `apply_patch`; `HANGAR_GATE` in the hook environment; real patch text (LF, paths relative to `cwd`); a JSON deny with exit 0 stops the write, exit 2 does not | measured with 0.167.2 through `codex.cmd`, [[codex-gate-live-measurement]] (probe as hook command; the helper itself in `tests/gate_hook.rs`) |
| Gemini CLI: settings file honoured with user hooks, `file_path` absolute or relative, trust env persists nothing, minimum release | not measured (not installed) |
| Copilot CLI: plugin hooks without a folder trust prompt, `exec` in a plugin, real `toolArgs` of `edit`, `create`, `apply_patch`, first release with `exec` | not measured (not installed) |
| Qwen Code: HTTP hook from the system settings file, `{}` read as no decision, first release with HTTP hooks | not measured (not installed) |
| OpenCode: plugin loads from `OPENCODE_CONFIG_DIR/plugins` (with an npm install there) | measured with 1.19.5, three live turns, [[opencode-gate-live-measurement]]; no longer used |
| OpenCode: plugin loads from an `OPENCODE_CONFIG_CONTENT` `file://` URL without any install, `HANGAR_GATE` and the token arrive, `write` (`filePath`) is held before it runs, a thrown deny stops it, an empty answer lets it run, the user's own plugins in `~/.config/opencode/plugins` still load | measured with 1.19.5, two live turns, [[opencode-gate-live-measurement]] |
| OpenCode: `edit`, `multiedit`, `apply_patch` argument names; merging with a `plugin` list in the user's `opencode.json` | not measured |
| Pi: `-e` loads without a trust prompt next to the user's packages, `HANGAR_GATE` and the token arrive, `write` is held before it runs, `{ block: true }` stops it | measured with 1.5.2, live process and scripted model, [[pi-gate-live-measurement]] |
| Pi: `edit` live, Hangar's "ask" in the TUI, a real model's reaction to the reason | not measured (Node test of the extension in `tests/gate_hook.rs`) |
| Oh My Pi: `-e` without trust prompt, `tool_call` block, edit modes and their path fields, `node:http` under Bun | not measured (not installed) |
| Droid: `--settings` carries `hooks` in the REPL, `Edit` and `ApplyPatch` input, JSON deny with exit 0, hook shell on Windows | not measured (not installed); helper round trip through cmd.exe or `sh -c` in `tests/gate_hook.rs` |

Minimum versions are enforced from the catalog's `min_version` (see Shape). Context injection
stays off for the helper CLIs and the in-process extensions until their output shapes are
measured; Qwen Code gets it through its HTTP prompt hook once it is offered.
