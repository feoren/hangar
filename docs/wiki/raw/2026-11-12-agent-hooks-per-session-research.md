# Raw: per-session hooks for Codex CLI, Gemini CLI, Copilot CLI and Qwen Code

Retrieved 2026-11-12. Desk research against vendor docs, GitHub sources on the default branches and
changelogs. Nothing here was run against a live model; the only local measurement is
`codex --version`, `codex --help` and `codex features list` on the research machine (Windows 11).
Extracts are paraphrased except where quoted.

## Versions seen on 2026-11-12

| CLI | Version | Evidence |
| --- | --- | --- |
| Codex CLI | 0.167.2 (latest stable, published 2026-11-11) | `codex --version` locally; https://github.com/openai/codex/releases |
| Gemini CLI | 0.68.0 stable (2026-11-10), 0.69.0-preview.0 | https://github.com/google-gemini/gemini-cli/releases |
| Copilot CLI | 1.0.109 stable (changelog entry 2026-11-10; npm `@github/copilot` 1.0.109) | https://github.com/github/copilot-cli/blob/main/changelog.md |
| Qwen Code | 0.27.2 stable (2026-11-10), 1.0.0-preview.1 (2026-11-12) | https://github.com/QwenLM/qwen-code/releases |

## Codex CLI

Sources: https://learn.chatgpt.com/docs/hooks (308 redirect from developers.openai.com/codex/hooks),
`codex --help` and `codex features list` (0.167.2), and these files of openai/codex `main` read on
2026-11-12: `codex-rs/hooks/src/engine/discovery.rs`, `engine/command_runner.rs`,
`events/pre_tool_use.rs`, `config_rules.rs`, `codex-rs/core/src/tools/handlers/apply_patch.rs`,
`codex-rs/config/src/loader/mod.rs`, `codex-rs/utils/cli/src/shared_options.rs`,
`codex-rs/apply-patch/src/parser.rs`.

- Docs: hooks come from `hooks.json` or inline `[hooks]` tables in `config.toml` in every active
  config layer (`~/.codex/...`, `<repo>/.codex/...`). Layers do not replace each other, every
  matching hook runs. Project-local hooks load only when the project `.codex/` layer is trusted.
  Feature flag `[features] hooks` (alias `codex_hooks` deprecated); `codex features list` shows
  `hooks stable true` on 0.167.2.
- Docs: PreToolUse input has the common fields `session_id`, `transcript_path`, `cwd`,
  `hook_event_name`, `model`, `permission_mode` plus `turn_id`, `tool_name`, `tool_use_id`,
  `tool_input`. Tool names: `Bash`, `apply_patch`, MCP tools, `exec_command`. A matcher on
  `apply_patch` may also be written `Edit` or `Write`; `tool_name` still reports `apply_patch`.
  The docs do not say what `tool_input.command` holds for `apply_patch`.
- Source (`apply_patch.rs`, `apply_patch_payload_command`, `pre_tool_use_payload`): `tool_input` is
  `{ "command": <raw patch text> }`; the patch is the freeform `Custom` payload string, so the hook
  sees the whole patch, not a path list.
- Source (`parser.rs`): markers `*** Begin Patch`, `*** Add File: `, `*** Delete File: `,
  `*** Update File: `, `*** Move to: `, `*** End Patch`. A unit test named
  `test_parse_patch_accepts_relative_and_absolute_hunk_paths` shows both relative and absolute hunk
  paths parse; paths resolve against the session cwd (`resolve_path(cwd)`).
- Docs and source (`pre_tool_use.rs`): block with `hookSpecificOutput.permissionDecision` set to
  `deny` plus `permissionDecisionReason`, the legacy JSON `decision` = `block` plus `reason`, or exit
  code 2 with the reason on stderr. `permissionDecision` = `ask` and the deprecated `approve` fail open
  (hook marked failed, tool runs). `allow` without `updatedInput` fails open. Exit 2 without stderr
  text is a failure that does not block. Other exit codes and timeouts do not block. The docs call
  tool hooks "a useful guardrail, not a complete enforcement boundary."
- Docs: default timeout 600 s (seconds); `SessionStart` output `additionalContext` or plain stdout is
  added as developer context; `UserPromptSubmit` likewise; default limit about 2 500 tokens per hook
  message, `additionalContextLimit` per handler. Source: `PreToolUseOutcome` carries
  `additional_contexts`.
- Docs: handlers `command` and `mcp_tool`; `prompt` and `agent` are parsed and skipped. No http
  handler.
- Source (`command_runner.rs`): the command string runs through a shell. Windows: `%COMSPEC%` or
  `cmd.exe` with `/C` and the command wrapped in quotes via `raw_arg`; Unix: `$SHELL` or `/bin/sh`
  with `-lc`. The child starts with an empty environment (`env_clear`) and gets the environment
  snapshot of the session plus the own `env` of the hook, minus a short list of restricted names
  (`NON_INHERITABLE_ENV_VARS`: Codex-internal tokens). JSON goes in on stdin. There is a
  `commandWindows` / `command_windows` override.
- Source (`discovery.rs`): config layer `SessionFlags` (what `-c` creates) is a hook source with
  `is_managed = false`. A non-managed hook is loaded only when its stored hash equals its current
  hash (`HookTrustStatus::Trusted`) or `bypass_hook_trust` is set. The hash is computed from a
  normalized identity of event, matcher and handler (`hook_hash`), keyed by source path, event,
  group index and handler index; for session flags the source path is the synthetic
  `<session-flags>/config.toml`. Trust state (`hooks.state.<key>.trusted_hash`, `enabled`) is read
  from the user layer and from the session-flags layer only (`config_rules.rs`); project, managed and
  plugin layers cannot write it.
- `codex --help` 0.167.2: `-c, --config <key=value>` (dotted path, value parsed as TOML, else
  literal), `--dangerously-bypass-hook-trust` (text: run enabled hooks without requiring persisted
  hook trust for this invocation, DANGEROUS, intended only for automation that already vets hook
  sources), `-C/--cd`, `--add-dir`, `-p/--profile` (layers `$CODEX_HOME/<name>.config.toml`),
  `--enable/--disable <feature>`.
- Source (`loader/mod.rs`): project layers are loaded but disabled when the directory is untrusted;
  trust comes from `projects.<path>.trust_level` in the user config. In
  `root_checkout_hooks_folder_for_dir`, for a linked git worktree the hooks folder is taken from the
  `.codex` of the root checkout, not from the own `.codex` of the worktree (code comment: regular
  checkouts resolve both paths to the same root, linked worktrees do not). Read as: a hooks file
  written into a Hangar-owned worktree is not the one Codex loads. This is an inference from reading
  the function, not a run.
- PRs: "fix(core): emit hooks for apply_patch edits" #18391, merged 2026-04-22; "Support PreToolUse
  updatedInput rewrites" #20527, merged 2026-05-12. First release carrying them not determined.

## Gemini CLI

Sources: https://geminicli.com/docs/hooks/ (page dated 2026-04-13), repo google-gemini/gemini-cli
`main`: `docs/hooks/reference.md`, `docs/reference/configuration.md`,
`packages/cli/src/config/settings.ts`, `settingsSchema.ts`, `config.ts`, `trustedFolders.ts`,
`packages/core/src/hooks/hookRegistry.ts`, `hookRunner.ts`, `services/environmentSanitization.ts`,
`utils/shell-utils.ts`, `tools/definitions/base-declarations.ts`.

- Docs: hooks live under `hooks` in `settings.json`; precedence project `.gemini/settings.json`,
  user `~/.gemini/settings.json`, system, extensions. Events include `BeforeTool`, `AfterTool`,
  `BeforeAgent`, `SessionStart`. Hook `type` is only `command`. `timeout` in milliseconds, default
  60000. `matcher` is a regex for tool events.
- Docs (`reference.md`): stdin carries `session_id`, `transcript_path`, `cwd`, `hook_event_name`,
  `timestamp`; `BeforeTool` adds `tool_name`, `tool_input` (raw model arguments), optional
  `mcp_context`, `original_request_name`. Exit 0: stdout JSON parsed; exit 2: block, stderr is the
  reason sent to the agent, the turn continues; other codes: warning, tool proceeds. `decision` set
  to `deny` (alias `block`) plus `reason` blocks. `SessionStart` and `BeforeAgent`
  `hookSpecificOutput.additionalContext` inject context; `AfterTool` appends context to the tool
  result; `BeforeTool` has no `additionalContext` in the reference. PR #21146 (merged 2026-03-21)
  adds an `ask` decision for BeforeTool.
- Source (`base-declarations.ts`, `default-legacy.ts`): file tools are `write_file` (params
  `file_path`, `content`) and `replace` (`file_path`, `old_string`, `new_string`, `instruction`).
  The `write_file` description says only "The path to the file to write to." (no absolute
  requirement); the glob tool says absolute. Shell is `run_shell_command` (param `command`).
- Source (`settingsSchema.ts`): every hook event array has `mergeStrategy: CONCAT`, so hooks of
  system, user and workspace settings are concatenated, not replaced. `settings.ts` merge order:
  schema defaults, system defaults, user, workspace (only if trusted), system settings (override).
- Source and docs: `GEMINI_CLI_SYSTEM_SETTINGS_PATH` replaces the system settings file path
  (default `C:\ProgramData\gemini-cli\settings.json` on Windows). `GEMINI_CLI_HOME` moves the user
  `.gemini` dir. `GEMINI_CLI_TRUST_WORKSPACE=true` "trusts the current workspace for the duration of
  the session, bypassing the folder trust check". `GEMINI_CLI_TRUSTED_FOLDERS_PATH` relocates
  `trustedFolders.json`. CLI option `--skip-trust` (trust the current workspace for this session).
  No `--settings` flag found among the yargs options in `config.ts`.
- Source (`settingsSchema.ts`): `security.folderTrust.enabled` default `true`.
- Source (`hookRegistry.ts`, `processHooksFromConfig`): all hooks from the merged settings are
  registered only `if (this.config.isTrustedFolder())`; otherwise it logs that project hooks are
  disabled because the folder is not trusted and registers none (inference: this also drops user and
  system hooks in an untrusted folder; not run). Project hooks in `.gemini/settings.json` are
  fingerprinted by `TrustedHooksManager`; an unseen hook prints a warning and is then marked trusted,
  it does not wait for an answer.
- Source (`hookRunner.ts`, `shell-utils.ts`): the command runs in a shell: Windows uses `$ComSpec` if
  it is a PowerShell, else `pwsh.exe`, else `powershell.exe`, with `-NoProfile -NonInteractive
  -Command`, and appends an exit code check; Unix `bash -c`. Env: `process.env` passed through
  `sanitizeEnvironment`, plus `GEMINI_PROJECT_DIR`, `GEMINI_CWD`, `GEMINI_SESSION_ID`,
  `CLAUDE_PROJECT_DIR`, plus the own `env` of the hook.
- Source (`environmentSanitization.ts`): when `enableEnvironmentVariableRedaction` is on (or in
  GitHub CI), variable names matching TOKEN, SECRET, PASSWORD, KEY, AUTH, CREDENTIAL, CERT, PRIVATE
  and values that look like JWTs, API keys or private keys are removed from the hook environment.
  Default of the setting not read.
- Releases: PR "feat(hooks): enable hooks system by default" #17247, merged 2026-01-21; v0.26.0
  stable is dated 2026-01-27 in the changelog index (assumed to contain it, not checked).
  Notice on geminicli.com: unpaid tier and Google One users were moved to Antigravity CLI on
  2026-06-18.

## GitHub Copilot CLI

Sources: https://docs.github.com/en/copilot/reference/hooks-configuration (same text as
github/docs `content/copilot/reference/hooks-reference.md`),
`content/copilot/reference/copilot-cli-reference/cli-command-reference.md`,
`cli-plugin-reference.md`, `how-tos/copilot-cli/customize-copilot/use-hooks.md`,
`how-tos/copilot-sdk/features/hooks.md`, and the changelog
https://github.com/github/copilot-cli/blob/main/changelog.md (read to 1.0.109).

- Hook files: policy dir (`C:\ProgramData\GitHub\Copilot\policy.d\*.json` on Windows), repo
  `.github/hooks/*.json`, inline `hooks` in `.github/copilot/settings.json` and `settings.local.json`
  and in `.claude/settings*.json`, user `~/.copilot/hooks/*.json` (`$COPILOT_HOME/hooks/` when
  set), inline in `~/.copilot/settings.json`, plugin `hooks.json`. All sources are combined, all
  entries run. A file needs `"version": 1`.
- Command hook fields: `bash`, `powershell`, `command`, and for the CLI `exec` plus `args` (runs an
  executable directly, no shell interpretation), `cwd`, `env`, `timeoutSec` (default 30),
  `matcher` (a regex wrapped as `^(?:PATTERN)$`, tested against `toolName`).
- HTTP hooks: `type` = `http`; the `url` must use `https://` for `preToolUse` and `permissionRequest`
  because the response can grant tool permissions; plain `http://localhost` and loopback only with
  `COPILOT_HOOK_ALLOW_LOCALHOST=1`, and `allowedEnvVars` requires `https://`. So an http hook to a
  loopback Hangar endpoint is not allowed for `preToolUse`.
- `preToolUse` input (camelCase event name): `sessionId`, `timestamp` (number), `cwd`, `toolName`,
  `toolArgs: unknown`. PascalCase `PreToolUse`: `hook_event_name`, `session_id`, `timestamp` (ISO
  string), `cwd`, `tool_name` (Claude names), `tool_input` (parsed from a JSON string when
  possible). The reference defines no file path field. Edit tools: `create`, `edit`; `apply_patch`
  and `str_replace_editor` appear only in the Claude name mapping (`edit`, `str_replace_editor`,
  `apply_patch` map to `Edit`, `create` to `Write`). The SDK hook example reads `toolArgs.path` for
  `read_file`, `write_file`, `edit`.
- Output: stdout JSON `permissionDecision` `allow|deny|ask`, `permissionDecisionReason` (required
  for deny), `modifiedArgs`, `additionalContext`; empty output means default. Exit code 2 denies even
  if stdout says allow; other non-zero is fail-closed for `preToolUse`; a timeout is fail-open for
  every event including `preToolUse` and policy hooks. `ask` becomes deny in autopilot and in the
  cloud agent.
- `additionalContext`: `sessionStart` (joined with a blank line, 10 MiB total), `postToolUse`
  (10 KB), `userPromptSubmitted`, `subagentStart`, `notification`; changelog 1.0.24 (2026-04-10):
  preToolUse hooks respect modifiedArgs/updatedInput and additionalContext fields.
- Per-session mechanisms (CLI reference): `--plugin-dir=DIRECTORY` loads a plugin from a local
  directory (repeatable); a legacy plugin has `plugin.json` with a `hooks` field (path or inline
  object) or `hooks.json` / `hooks/hooks.json`; `copilot plugin disable` notes that a `--plugin-dir`
  mount has no persisted activation. `COPILOT_HOME` (and the deprecated `--config-dir`) relocate the
  whole config and state dir. `--additional-mcp-config` is per session for MCP only.
  `COPILOT_PLUGIN_DIR_ONLY` disables automatic plugin discovery.
- Trust: changelog 1.0.8 (2026-03-18): repo-level hooks load only after folder trust is confirmed.
  `trustedFolders` lives in `config.json`. `--trust-folder` trusts the working directory hooks,
  workspace MCP servers and repo plugin settings for one new non-interactive session (`-p` or piped
  standard input), is not saved, and cannot be combined with `--resume`, `--continue`,
  `--session-id`. `COPILOT_ALLOW_ALL=true` also trusts the working directory without prompting
  (broad: allows all tools). Prompt mode loads repo hooks only if the folder is trusted or
  `GITHUB_COPILOT_PROMPT_MODE_REPO_HOOKS=true`. Policy hooks run regardless of folder trust. The
  reference does not say whether plugin hooks from `--plugin-dir` need folder trust.
- Versions: http hooks 1.0.35 (2026-04-23), `ask` decision 1.0.4 (2026-03-11), user hooks dir
  0.0.422 (2026-03-05), matcher fix 1.0.36, exit code 2 denies in `preToolUse` 1.0.70 (2026-07-09),
  preToolUse errors deny 1.0.57. Version of `exec`/`args` not found in the changelog.
- Windows: `powershell` field and `exec`; the changelog notes BOM-tolerant hook files and no console
  window flash for hook commands.

## Qwen Code

Sources: https://github.com/QwenLM/qwen-code `main`: `docs/users/features/hooks.md`,
`docs/users/configuration/settings.md`, `packages/cli/src/config/settingsSchema.ts`, `config.ts`,
`packages/core/src/tools/tool-names.ts`, `write-file.ts`, `edit.ts`; PR #2827 "feat(hooks): Add
HTTP Hook, Function Hook and Async Hook support" (merged 2026-04-16). License Apache-2.0.

- Qwen Code has its own hook system with Claude-style events and output. Hook types `command`,
  `http`, `function` (SDK only), `prompt`. Settings key `hooks`, per event `CONCAT` merge (schema).
- Config scopes: project `.qwen/settings.json`, user, system (`QWEN_CODE_SYSTEM_SETTINGS_PATH`
  overrides the path; default `C:\ProgramData\qwen-code\settings.json`) and system defaults
  (`QWEN_CODE_SYSTEM_DEFAULTS_PATH`). Docs: hooks in the system settings files (System and
  SystemDefaults) load with the source System and, like user hooks, regardless of folder trust.
  Order within an event: Project, User, System, Extension.
- Project hooks need approval of the exact configuration (stored in `~/.qwen/hookApprovals.json`)
  or explicit folder trust; non-interactive runs never prompt and leave unapproved project hooks
  unloaded. `QWEN_HOME` relocates the global dir (credentials, settings). `--bare`, `--safe-mode`
  and `disableAllHooks` turn hooks off.
- PreToolUse input: common fields (`session_id`, `transcript_path`, `cwd`, `hook_event_name`,
  `timestamp`, `permission_mode`, ...) plus `tool_name` (runtime id such as `write_file`, `edit`),
  `tool_input` (object), `tool_use_id`. The matcher is an unanchored regex on the tool id; Claude
  names (`Write`, `Edit`) are accepted aliases. Tools: `write_file`, `edit` with `file_path`
  (`write-file.ts`, `edit.ts`; resolved with `path.resolve`), `run_shell_command`.
- Output: `hookSpecificOutput.permissionDecision` `allow|deny|ask` (plus reason), `updatedInput`,
  `additionalContext` (appended to the tool result of this call, also when denied). Exit 2 blocks,
  stderr goes to the model; other exit codes are non-blocking. `ask` in headless runs and ACP falls
  back to deny. `SessionStart` and `UserPromptSubmit` `additionalContext` inject context; command
  stdout text is added to the context for those events.
- Timeouts: command hooks default 60 s (values of 1000 or more are read as milliseconds), http hooks
  default 600 s. HTTP hooks: POST of the input JSON, `headers` with variable interpolation limited
  by `allowedEnvVars`, private IPs blocked but loopback (127.0.0.1, ::1) allowed, redirects never
  followed, a non-2xx status is a non-blocking failure, JSON read only with the content type
  application/json. The docs example uses `http://127.0.0.1:8080/hooks/pre-tool-use`.
  `security.allowedHttpHookUrls`: an empty list means allow all.
- `shell` field for command hooks: `bash` or `powershell`.
- Not found: a CLI flag that adds a settings file for one session; the first stable release that
  contains HTTP hooks (PR merged 2026-04-16).
