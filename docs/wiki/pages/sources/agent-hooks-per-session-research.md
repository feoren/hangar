---
title: "Research: per-session edit gates for Codex, Gemini, Copilot and Qwen Code"
type: source
tags: [hooks, gate, codex, gemini, copilot, qwen, per-session, trust, windows, research]
sources: []
raw: raw/2026-11-12-agent-hooks-per-session-research.md
url: https://learn.chatgpt.com/docs/hooks
retrieved: 2026-11-12
created: 2026-11-12
updated: 2026-11-12
---

# Research: per-session edit gates for Codex, Gemini, Copilot and Qwen Code

> [!note] Built
> The recipes below are implemented, partly measured: [[agent-gate-adapters]].

Question: can Hangar make the four CLIs "team-capable" like Claude Code, meaning (a) hold or block an
edit before it happens, (b) inject context, (c) install both for one session without touching the
user's global files. Desk research on 2026-11-12 against vendor docs, GitHub sources and changelogs;
details and quotes in the raw record `raw/2026-11-12-agent-hooks-per-session-research.md`. Only
`codex --version`, `--help` and `features list` were run locally; **no hook was run against a live
model**, so every recipe below needs the measurement listed in section 7 before the catalog flags
change. Context: [[team-service-v1]] section 1 and `src-tauri/src/agent_catalog.toml`.

Versions on the day: Codex 0.167.2, Gemini CLI 0.68.0, Copilot CLI 1.0.109, Qwen Code 0.27.2
(1.0.0-preview.1 in flight).

## Result per CLI

| CLI | pre_edit_block | context_inject | per_session_config | Evidence in one line |
| --- | --- | --- | --- | --- |
| Codex 0.167.2 | yes for `apply_patch` (deny JSON or exit 2); not for shell writes | yes (`SessionStart`, `UserPromptSubmit`, `PreToolUse` `additionalContext`) | partial: `-c hooks...` makes a session layer but needs `--dangerously-bypass-hook-trust`; a project file in a linked worktree is read from the root checkout | `pre_tool_use.rs`, `discovery.rs`, `loader/mod.rs`, `codex --help` |
| Gemini CLI 0.68.0 | yes for `write_file` and `replace` (`decision` deny or exit 2) | yes (`SessionStart`, `BeforeAgent`; not in `BeforeTool`) | yes: file named by `GEMINI_CLI_SYSTEM_SETTINGS_PATH`, hooks are concatenated with the user hooks; needs a trusted folder, satisfiable per session by `GEMINI_CLI_TRUST_WORKSPACE=true` | `settingsSchema.ts` (CONCAT), `settings.ts`, `hookRegistry.ts`, configuration.md |
| Copilot CLI 1.0.109 | yes for `edit` and `create` (deny JSON or exit 2); `apply_patch` shape unverified | yes (`sessionStart`, `userPromptSubmitted`, `postToolUse`) | partial: `--plugin-dir <dir>` with a plugin that carries `hooks`; trust status of plugin hooks undocumented; the http hook is not usable (https only for `preToolUse`) | hooks-reference.md, cli-command-reference.md |
| Qwen Code 0.27.2 | yes for `write_file` and `edit` (`permissionDecision` deny or exit 2), http hooks too | yes (`SessionStart`, `UserPromptSubmit`, `PreToolUse` into the tool result) | yes: file named by `QWEN_CODE_SYSTEM_SETTINGS_PATH`, system hooks load regardless of folder trust and run next to the user hooks | hooks.md, settingsSchema.ts |

"yes" means documentation and source say so; none of it is measured yet. All four fail open on
timeout or crash (details below), so the file watcher stays the second line, as in [[team-service-v1]].

Closest to the existing Claude gate: **Qwen Code**. Its hook model is the Claude one (events,
`hookSpecificOutput.permissionDecision`, `updatedInput`, http hooks with `headers` interpolated from
`allowedEnvVars`, loopback allowed), and the session file can have the shape of `claude_settings` in
`src-tauri/src/gate.rs:72`.

## 1. Hook mechanism per CLI

### Codex CLI (openai/codex, 0.167.2)

- Mechanism: lifecycle hooks, feature `hooks` (stable, on). Config: `hooks.json` or `[hooks]` in
  `config.toml` of every config layer; all layers run, none replaces another. Handler types:
  `command` and `mcp_tool` only, **no http handler**. The command is a shell string: `cmd.exe /C` on
  Windows (with a `commandWindows` override), `$SHELL -lc` elsewhere.
- Payload (stdin JSON): `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `model`,
  `permission_mode`, `turn_id`, `tool_name`, `tool_use_id`, `tool_input`. For `apply_patch`,
  `tool_input` is `{"command": "<the complete patch text>"}`, built from the freeform tool payload
  (`apply_patch.rs`). There is no separate path list: Hangar must parse the patch.
- Patch paths: lines `*** Add File: <p>`, `*** Update File: <p>`, `*** Delete File: <p>` and
  `*** Move to: <p>` between `*** Begin Patch` and `*** End Patch`; one patch can touch several
  files. The parser accepts relative and absolute paths (a unit test is named
  `..._accepts_relative_and_absolute_hunk_paths`); they resolve against the session `cwd`. Separator
  style on Windows (`/` or `\`) is not documented [UNVERIFIED]; normalize both.
- Matcher: regex on `tool_name` and aliases; `apply_patch` also answers to `Edit` and `Write`. Shell
  commands arrive as `Bash` (or `exec_command`) with the command text only, so a shell write
  (`sed -i`, redirection) is visible as text, not as a path.
- Block: `hookSpecificOutput.permissionDecision` = `deny` plus `permissionDecisionReason`, or exit 2
  with the reason on stderr (exit 2 without stderr text does not block). `ask` is not supported and
  fails open. Errors, malformed output and timeouts let the tool run; the docs call tool hooks "a
  useful guardrail, not a complete enforcement boundary."
- Timeout: seconds, default 600. A held hook can wait up to that, then the tool runs.
- Context: `SessionStart` and `UserPromptSubmit` (stdout text or `additionalContext`), also
  `PreToolUse` `additionalContext`; about 2 500 tokens per message by default.

### Gemini CLI (google-gemini/gemini-cli, 0.68.0)

- Mechanism: hooks in `settings.json` (`hooks.<Event>[]` with `matcher` and `hooks[]`); events
  `BeforeTool`, `AfterTool`, `BeforeAgent`, `SessionStart` and more. Only `type` = `command`; the
  command runs in PowerShell on Windows (`-NoProfile -NonInteractive -Command`, from `$ComSpec`,
  `pwsh.exe` or `powershell.exe`), `bash -c` elsewhere. Enabled by default since PR #17247 (merged
  2026-01-21); minimum release [UNVERIFIED], v0.26.0 by dates.
- Payload (stdin JSON): `session_id`, `transcript_path`, `cwd`, `hook_event_name`, `timestamp`,
  `tool_name`, `tool_input` (raw model arguments), `mcp_context`. File tools: `write_file` with
  `file_path` and `content`; `replace` with `file_path`, `old_string`, `new_string`, `instruction`.
  The path is what the model sent: absolute or relative to `cwd` [UNVERIFIED which], so resolve
  against `cwd` and normalize separators.
- Block: stdout JSON with `decision` = `deny` (alias `block`) and `reason`, or exit 2 with stderr as
  the reason; the turn continues and the agent sees a tool error. Other exit codes are a warning and
  the tool proceeds. PR #21146 adds an `ask` decision (not in the reference) [UNVERIFIED].
- Timeout: milliseconds, default 60000; failure is non-blocking.
- Context: `SessionStart` and `BeforeAgent` `hookSpecificOutput.additionalContext`; `AfterTool` appends
  to the result. Nothing for `BeforeTool`.
- Shell writes are `run_shell_command` with a `command` string: not covered per path.

### GitHub Copilot CLI (github/copilot-cli, 1.0.109)

- Mechanism: hook JSON files (`"version": 1`, `hooks.<event>[]`), sources combined: policy, repo
  `.github/hooks/*.json` and `.github/copilot/settings*.json`, user `~/.copilot/hooks/*.json`
  (`$COPILOT_HOME/hooks/`), plugins. Event names in camelCase (`preToolUse`) give the native payload,
  PascalCase (`PreToolUse`) the VS Code style payload with Claude tool names. Entry types: command
  (`bash`, `powershell`, `command`, or **`exec` plus `args`, run without a shell**) and http.
- Payload camelCase: `sessionId`, `timestamp` (ms), `cwd`, `toolName`, `toolArgs: unknown`.
  PascalCase: `session_id`, `tool_name`, `tool_input` (parsed from JSON when possible). The reference
  has no file path field; the SDK example reads `toolArgs.path` for `edit` and `write_file`. Edit
  tools by name: `edit`, `create`; `apply_patch` and `str_replace_editor` are mapped to `Edit` in the
  Claude table, so they exist as tool names, but their argument shape is undocumented [UNVERIFIED].
  `toolArgs` may arrive as a JSON string or an object: accept both.
- Block: stdout JSON `permissionDecision` = `deny` with `permissionDecisionReason`, or exit 2 (deny
  even if stdout says allow, since 1.0.70); other non-zero exits also deny for `preToolUse` (since
  1.0.57). `ask` prompts in an interactive session, becomes deny in autopilot. **A timeout is
  fail-open for every event** (default `timeoutSec` 30).
- HTTP hooks: `preToolUse` and `permissionRequest` URLs must be `https://`; `http://127.0.0.1` needs
  `COPILOT_HOOK_ALLOW_LOCALHOST=1` and is still excluded for these two events. The Claude style http
  hook does not carry over; use `exec`/`args` with a Hangar helper executable.
- Context: `sessionStart` `additionalContext` (10 MiB cap, joined with a blank line),
  `userPromptSubmitted` (since 1.0.65), `postToolUse` (10 KB), `preToolUse` (since 1.0.24).

### Qwen Code (QwenLM/qwen-code, 0.27.2, Apache-2.0)

- Mechanism: own hook system, Claude-style. `hooks.<Event>[]` with `matcher` and `hooks[]` in
  `settings.json`; types `command` (with `shell` = `bash` or `powershell`), `http`, `prompt`,
  `function` (SDK only). HTTP hooks (PR #2827, merged 2026-04-16): POST of the input JSON, headers with
  `${VAR}` limited by `allowedEnvVars`, loopback allowed, private ranges blocked, no redirects, non-2xx
  is a non-blocking failure, JSON is read only for content type `application/json`. First release
  with http hooks [UNVERIFIED].
- Payload: common fields plus `tool_name` (`write_file`, `edit`; the Claude aliases `Write` and `Edit`
  also match), `tool_input` object with `file_path` (the tools call `path.resolve` on it),
  `tool_use_id`.
- Block: `hookSpecificOutput` with `hookEventName`, `permissionDecision` = `deny` and a reason; or
  exit 2. `ask` works in the TUI, becomes deny headless and in ACP. Other exit codes do not block.
  Timeouts: command 60 s, http 600 s.
- Context: `SessionStart`, `UserPromptSubmit`; `PreToolUse` `additionalContext` is appended to the
  tool result (also when denied).

## 2. Per-session installation, user files untouched, user hooks merging

| CLI | Mechanism | User hooks still run | Needs trust | Verdict |
| --- | --- | --- | --- | --- |
| Codex | `-c "hooks.PreToolUse=[...]"` creates the session-flags layer, plus `--dangerously-bypass-hook-trust` | yes, layers add up | yes, hash trust; a project layer also needs `projects.<path>.trust_level` | flags only; the bypass also removes the trust check for user and repository hooks in that run |
| Gemini | env `GEMINI_CLI_SYSTEM_SETTINGS_PATH=<file>` | yes, `CONCAT` | folder trust (default on) | file in a Hangar directory, env only |
| Copilot | `--plugin-dir <dir>` with `plugin.json` and `hooks.json` | yes, all sources run | undocumented for plugin hooks | one flag, hooks in a Hangar directory |
| Qwen | env `QWEN_CODE_SYSTEM_SETTINGS_PATH=<file>` | yes, `CONCAT` (order Project, User, System) | no: system hooks load regardless of folder trust | file in a Hangar directory, env only |

Rejected routes and why:
- **Project file in a Hangar worktree.** Codex reads the hooks folder of a linked worktree from the
  root checkout (`root_checkout_hooks_folder_for_dir`, inference from source) and needs project
  trust. Gemini needs folder trust and warns on unseen project hooks. Copilot `.github/hooks` needs
  the interactive trust dialog (since 1.0.8). Qwen needs an approval dialog or folder trust, and
  non-interactive runs never prompt. All of them also write into the repository.
- **Relocating the home** (`CODEX_HOME`, `GEMINI_CLI_HOME`, `COPILOT_HOME`, `QWEN_HOME`): moves
  credentials, sessions, trust lists and settings with it, so the user is signed out and loses
  history. Not suitable as a gate carrier.
- **Replacing the system settings path** (Gemini, Qwen) hides an administrator file at the default
  system path for that session. Fine on a personal machine; mention it for managed ones.

## 3. Trust and approval prompts

- **Codex:** untrusted non-managed hooks are skipped without a prompt in an unattended run, with a
  startup warning. Legitimate per-session answer: `--dangerously-bypass-hook-trust` (documented for
  "automation that already vets hook sources"). Alternative without the bypass: write
  `hooks.state.<key>.trusted_hash` through `-c` (the session layer may set it, `config_rules.rs`),
  but the hash comes from an internal normalized identity and is not a public contract [UNVERIFIED
  stable]; the app-server hook listing carries a `current_hash` per hook and could be read once.
- **Gemini:** an untrusted folder drops all hooks, interactive sessions show a trust dialog.
  `GEMINI_CLI_TRUST_WORKSPACE=true` or `--skip-trust` trusts the workspace for the session only (not
  written to `trustedFolders.json`) [UNVERIFIED that nothing is persisted]. It also loads the
  repository `.gemini/settings.json` and `.env`, so use it only for folders the user already works in.
- **Copilot:** no per-session trust for interactive sessions. `--trust-folder` is for `-p` and piped
  sessions only; `COPILOT_ALLOW_ALL=true` trusts but also allows all tools, not acceptable. Policy
  hooks need elevation. Whether `--plugin-dir` hooks wait for folder trust must be measured.
- **Qwen:** none needed for system-scope hooks.

## 4. Minimum versions and Windows

| CLI | Minimum for the recipe | Windows |
| --- | --- | --- |
| Codex | apply_patch hooks merged 2026-04-22 (#18391); `--dangerously-bypass-hook-trust` present in 0.167.2; exact first release [UNVERIFIED], gate on a probed version | `cmd /C` runner and `commandWindows` in source; no platform gate found |
| Gemini | hooks default on since 2026-01-21 (v0.26.0 by dates); env vars present on main | PowerShell runner |
| Copilot | first release with `exec`/`args` unknown; exit 2 deny since 1.0.70; 1.0.109 is the tested floor of the docs | `powershell` field and `exec`; BOM-tolerant files |
| Qwen | http hooks merged 2026-04-16; first release [UNVERIFIED] | command hooks take `shell: powershell`; http needs no shell |

Hangar already probes `--version` ([[0003-program-versions]]). Enable the gate only from a version
that was measured, not from this table.

## 5. Recommended recipes (argv and files, no shell string built by Hangar)

Codex, Gemini and Copilot run command hooks, so the agent starts a process that reads the JSON from
stdin. Proposal: a **helper mode of the Hangar executable**, `hangar gate-hook <agent> <event>`,
that reads stdin, converts it to the gate request (`/v1/pre-tool-use`), calls the local endpoint with
the token from the environment (`HANGAR_HOOK_TOKEN`, `src-tauri/src/gate.rs:30`) and prints the
agent-specific answer. The held decision then stays inside Hangar as for Claude Code. Exit 0 with
JSON on success; exit 2 plus stderr for a deny as the belt-and-braces answer. Qwen needs no helper.

**Qwen Code (do first, nearly free).** Session file under the Hangar data directory with the shape of
`claude_settings`: `hooks.PreToolUse` = one group with `matcher` `^(write_file|edit)$` and one hook
`{ type: http, url: http://127.0.0.1:<port>/v1/pre-tool-use, timeout: 660, headers: { Authorization:
"Bearer ${HANGAR_HOOK_TOKEN}" }, allowedEnvVars: [HANGAR_HOOK_TOKEN] }`, plus `UserPromptSubmit` for
context. Launch: program and arguments unchanged; PTY child environment gets
`QWEN_CODE_SYSTEM_SETTINGS_PATH=<file>` and `HANGAR_HOOK_TOKEN=<token>`. The reply must have content
type `application/json` and carry `hookEventName`. Whether the Claude shaped route accepts the Qwen
request unchanged is [UNVERIFIED].

**Gemini CLI.** File `<data>/gate/<session>/gemini-settings.json`: `hooks.BeforeTool` = group with
`matcher` `write_file|replace` and one hook `{ type: command, name: hangar-gate, command:
"& '<abs>\hangar.exe' gate-hook gemini before-tool", timeout: 660000 }`, plus `SessionStart` and
`BeforeAgent` for context. Env: `GEMINI_CLI_SYSTEM_SETTINGS_PATH=<file>`,
`GEMINI_CLI_TRUST_WORKSPACE=true`, and the token under a neutral name such as `HANGAR_GATE`: when the
user has environment redaction on, names containing TOKEN, KEY, AUTH and similar are removed from
the hook environment. The command is a PowerShell string: refuse install paths with a single quote
or other special characters instead of escaping them, as in [[cmd-launcher]].

**Copilot CLI.** Directory `<data>/gate/<session>/copilot-plugin/` with `plugin.json` (`name`
`hangar-gate`, `version`, `hooks` = `hooks.json`) and `hooks.json` (`version` 1, `hooks.preToolUse` =
one entry `{ type: command, exec: <abs>\hangar.exe, args: [gate-hook, copilot, pre-tool-use],
matcher: "edit|create|apply_patch", timeoutSec: 660 }`, plus `sessionStart` for context). Launch: argv
`copilot --plugin-dir <dir>` plus the user arguments; the environment carries only the token. No
shell, no quoting problem. **Gate this on a measurement** that plugin hooks run without a folder
trust prompt and that `exec` works inside a plugin file.

**Codex CLI.** Argv (one element each): `codex`, `--dangerously-bypass-hook-trust`, `-c`, and the TOML
value `hooks.PreToolUse=[{matcher="^apply_patch$",hooks=[{type="command",command='"<abs>\hangar.exe"
gate-hook codex pre-tool-use',timeout=660}]}]`, then the user arguments. The inner single quotes are
TOML literal strings, which avoid backslash escapes. The command goes through `cmd /C` on Windows, so
keep the install path free of `%`, `&`, `^` and quotes. The helper parses the patch text, reports
every path (all hunks plus move targets) to the gate and answers deny if any is claimed. Needs the
`-c` inline table syntax measured, and the bypass flag accepted as a risk (below).

## 6. Risks

- **Fail open on timeout or crash.** Codex runs the tool on error or timeout, Gemini treats other exit
  codes as warnings, Copilot is fail-open on timeout (fail-closed on a crash), Qwen command hooks do not
  block on other exit codes and http errors are non-blocking. The helper must answer fast and keep the
  held decision shorter than the hook timeout (set 660 s explicitly; the Codex default is 600 s).
- **Coverage.** Hooks see tool calls, not file writes: shell tools (`Bash`, `run_shell_command`,
  `bash` and `powershell`) write files without a path in the payload. The file watcher stays the
  second line. Copilot `apply_patch` and `create` shapes, and whether Codex hooks an `apply_patch`
  typed into the shell, need a run [UNVERIFIED].
- **Codex trust bypass applies to the whole run.** It also lets an untrusted project hook of the
  opened repository run unreviewed. Mitigation: use it only for sessions Hangar starts in a folder the
  user chose, say so in the UI text, and prefer a pre-written `hooks.state` trust if that proves
  stable.
- **Gemini `GEMINI_CLI_TRUST_WORKSPACE=true`** trusts a folder (settings, `.env`, project hooks) for
  that session without asking.
- **Moving targets.** Gemini free tiers moved to Antigravity CLI on 2026-06-18; Qwen is at 1.0.0-preview
  with large hook changes (hosted hooks, approvals). Pin a tested version range per agent and
  re-measure on a major bump.
- **Environment.** Codex starts hooks with an empty environment plus the session snapshot
  [UNVERIFIED that `HANGAR_HOOK_TOKEN` survives]; Gemini may redact names such as TOKEN. Measure that
  the helper sees the variable.
- **Secrets.** Never write the token into the session files; all recipes pass it through the child
  environment only. Copilot `args` and Codex `-c` values are visible in the process list: put only
  the port and session id there, never the token.

## 7. Open questions (each needs a measurement)

1. Codex: does the inline-table `-c` value parse and register as a session hook with the bypass flag,
   on Windows through portable-pty? Does the helper environment contain `HANGAR_HOOK_TOKEN`? What
   does the patch look like for add, update, move and delete, with Windows and POSIX separators? Is
   an `apply_patch` typed into the shell also hooked?
2. Gemini: absolute or relative `file_path` in `BeforeTool`? Does the `GEMINI_CLI_SYSTEM_SETTINGS_PATH`
   file work for `BeforeTool` with user hooks present? Does `GEMINI_CLI_TRUST_WORKSPACE` persist
   anything? Which release first accepts all of it?
3. Copilot: do plugin hooks from `--plugin-dir` run in an untrusted folder, and does `exec` work
   there? Real `toolArgs` for `edit`, `create` and `apply_patch` (string or object, key names)? First
   version with `exec` and `args`?
4. Qwen: does the Claude shaped Hangar route work unchanged (field names, `hookEventName`, content
   type)? First stable release with http hooks, and behavior of the 1.0 line.
5. Catalog: `capabilities` are three booleans and the installer knows only the Claude `--settings`
   format. A per-agent install recipe (format, env var, argv) would be needed in `agent_catalog.toml`
   and `gate_service.rs`; that is a design step after the measurements.

## Significance for Hangar

- Qwen Code can become team-capable with almost the code of Claude Code (system settings env var
  plus the existing http endpoint). Gemini needs a helper command and two env vars. Copilot needs a
  helper and `--plugin-dir`. Codex is feasible but the least clean (bypass flag, patch parsing, a
  shell string on Windows).
- The catalog comments for codex, gemini and copilot ("per-session hooks unverified") are answered on
  paper; flip the flags only after the measurements in section 7.
- **Update 2026-11-12:** the owner asked for all four to be team-capable now. Built as in section 5
  ([[agent-gate-adapters]]): Qwen Code with its HTTP hook, Gemini CLI, Copilot CLI and Codex CLI
  through the helper `hangar gate-hook`; the catalog flags are on. Of section 7 only the Windows
  start of the helper and the Codex `-c` value are measured ([[gate-hook-helper-measurement]]);
  the rest stays open and is listed in [[agent-gate-adapters]]. Deviations from section 5: the
  Codex matcher is `apply_patch` without anchors and the path stands bare through a cmd.exe
  launcher (cmd.exe eats `^` and `"` is refused); Qwen's routes answer `{}` for no decision.
- **Update 2026-11-12, later:** owner's decision "measure first". Codex was run live
  ([[codex-gate-live-measurement]]): hook fires, `HANGAR_GATE` arrives, the patch has relative
  paths, and only a JSON deny with exit 0 blocks (exit 2 plus JSON fails open, against the docs).
  Codex is offered from 0.167.2; Gemini, Copilot and Qwen keep their recipes with
  `measured = false` until each is measured live.
