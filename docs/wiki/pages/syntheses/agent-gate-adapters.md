---
title: "Agent gate adapters: Qwen Code, Gemini CLI, Copilot CLI and Codex CLI"
type: synthesis
tags: [gate, hooks, team, codex, gemini, copilot, qwen, catalog, helper]
sources: ["[[agent-hooks-per-session-research]]", "[[gate-hook-helper-measurement]]", "[[codex-gate-live-measurement]]", "[[claude-http-hook-measurement]]"]
created: 2026-11-12
updated: 2026-11-12
---

# Agent gate adapters: Qwen Code, Gemini CLI, Copilot CLI and Codex CLI

How Hangar's agent gate ([[team-service-v1]] step B) reaches one session of each agent CLI, so
that the CLI can be a team session ([[0008-private-and-team-sessions]]). The recipes follow
[[agent-hooks-per-session-research]] section 5; [[gate-hook-helper-measurement]] settled the
Windows start of the helper and the Codex `-c` value. Code: `src-tauri/src/hook_recipes.rs`
(recipes, pure), `gate_service.rs` (install, folders, cleanup), `gate_hook.rs` (helper),
`agent_catalog.rs` (`HookFormat`), `agent_catalog.toml` (flags).

> [!note] What is offered
> Only **Codex CLI** (from 0.167.2) is team-capable besides Claude Code: its recipe was run live
> once ([[codex-gate-live-measurement]]). Gemini CLI, Copilot CLI and Qwen Code have their recipe
> built and tested against the endpoint, but `measured = false` keeps them off until each one
> is run live once (owner's decision: measure first).

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
  Codex 0.167.2 (measured; the first release with the bypass flag is not known), Copilot 1.0.109,
  Gemini 0.68.0, Qwen 0.27.2 (the versions the research read).
- **Variables a child Hangar strips.** `hook_recipes::GATE_ENV_NAMES` lists every variable the
  gate sets. `child_env` always removes the token variables (`HANGAR_HOOK_TOKEN`, `HANGAR_GATE`);
  when Hangar itself inherited one of them (it runs inside a gated session), it removes the
  settings and trust variables too. Without a token they are the user's or an administrator's own
  and stay for private sessions.
- **One endpoint.** Every format ends at the same loopback endpoint and the same decision logic
  as Claude Code (`/v1/pre-tool-use`). Qwen Code calls it as an HTTP hook; the other three run the
  helper `hangar gate-hook <agent> <event>`, which turns their hook input into Claude Code's
  request and the answer back into their own format.
- **Token only in the environment.** `HANGAR_HOOK_TOKEN` for HTTP hooks (header interpolation),
  `HANGAR_GATE=<port>:<token>` for the helper (a neutral name: Gemini CLI may strip names with
  TOKEN, KEY, AUTH, and Codex's shell environment policy drops such names by default). No file,
  no argument holds it (unit tests per format read every file and argument).
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
  conflict].

## Recipe per CLI

| CLI | Format | What the start gets | Edit tools |
| --- | --- | --- | --- |
| Qwen Code | `qwen_system_settings` | env `QWEN_CODE_SYSTEM_SETTINGS_PATH=<folder>/qwen-settings.json`, `HANGAR_HOOK_TOKEN` | matcher `^(write_file\|edit)$` |
| Gemini CLI | `gemini_system_settings` | env `GEMINI_CLI_SYSTEM_SETTINGS_PATH=<folder>/gemini-settings.json`, `GEMINI_CLI_TRUST_WORKSPACE=true`, `HANGAR_GATE` | matcher `write_file\|replace` |
| Copilot CLI | `copilot_plugin` | argv `--plugin-dir <folder>/copilot-plugin` first, `HANGAR_GATE` | matcher `edit\|create\|apply_patch` |
| Codex CLI | `codex_config_flags` | argv `--dangerously-bypass-hook-trust -c hooks.PreToolUse=[...]` first, `HANGAR_GATE` | matcher `apply_patch` |

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

## The helper `hangar gate-hook`

- `main.rs` branches to `gate_hook::main` before Tauri, so no window, single instance lock or
  plugin starts. The release build is a GUI-subsystem program; measured, cmd.exe, PowerShell and a
  direct start wait for it and pipe stdin and stdout.
- Input per agent: Gemini CLI and Codex `tool_name`, `tool_input`, `cwd`; Copilot CLI `toolName`,
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
  Hangar's "ask" (queue full) stays an ask for Copilot and becomes a deny that says so for Gemini
  and Codex, which have no usable ask.
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
  write files without a hook path. The file watcher (claims) stays the second line.

## Measured and not measured

| Point | State |
| --- | --- |
| Helper reads stdin, answers on stdout, exit codes; round trip through cmd.exe, PowerShell and a direct start against a real endpoint | measured, `tests/gate_hook.rs` (debug build) and [[gate-hook-helper-measurement]] (GUI-subsystem probe) |
| Codex accepts and registers the `-c` value, also through `codex.cmd` | measured with 0.167.2 |
| Codex runs the hook on a live `apply_patch`; `HANGAR_GATE` in the hook environment; real patch text (LF, paths relative to `cwd`); a JSON deny with exit 0 stops the write, exit 2 does not | measured with 0.167.2 through `codex.cmd`, [[codex-gate-live-measurement]] (probe as hook command; the helper itself in `tests/gate_hook.rs`) |
| Gemini CLI: settings file honoured with user hooks, `file_path` absolute or relative, trust env persists nothing, minimum release | not measured (not installed) |
| Copilot CLI: plugin hooks without a folder trust prompt, `exec` in a plugin, real `toolArgs` of `edit`, `create`, `apply_patch`, first release with `exec` | not measured (not installed) |
| Qwen Code: HTTP hook from the system settings file, `{}` read as no decision, first release with HTTP hooks | not measured (not installed) |

Minimum versions are enforced from the catalog's `min_version` (see Shape). Context injection
stays off for the three helper CLIs until their output shapes are measured; Qwen Code gets it
through its HTTP prompt hook once it is offered.
