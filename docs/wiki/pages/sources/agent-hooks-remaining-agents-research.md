---
title: "Research: per-session edit gates for the remaining agent CLIs in the catalog"
type: source
tags: [hooks, gate, per-session, trust, windows, research, opencode, pi, omp, droid, cline, amp, crush, vibe, goose, kimi, cursor, kiro, antigravity, aider]
sources: []
raw: raw/2026-11-12-agent-hooks-remaining-agents-research.md
url: https://pi.dev/docs/latest/extensions
retrieved: 2026-11-12
created: 2026-11-12
updated: 2026-11-12
---

# Research: per-session edit gates for the remaining agent CLIs in the catalog

Follow-up to [[agent-hooks-per-session-research]] (Codex, Gemini, Copilot, Qwen). Same criteria:
(a) pre_edit_block, (b) context_inject, (c) per_session_config (argv, env or a file in a Hangar
directory, no change to user or repository files, no blocking trust prompt). Desk research on
2026-11-12; **no hook was run against a live model and no agent was installed**. Every "yes" below
means docs or source say so; the measurements in section 6 come first. Context: [[team-service-v1]]
and `src-tauri/src/agent_catalog.toml`.

Versions on the day (npm, PyPI, GitHub releases): OpenCode 1.19.5 (the 2.x line `@opencode/cli` is a
separate package), Pi 1.5.2, Oh My Pi 18.19.0, Cline 3.0.74, Crush 0.101.0, Kimi Code 2.6.0, Amp
0.0.1794499251-g8dc9ce, Kode 2.2.1, Codebuff 1.0.688, Mistral Vibe 3.1.2, Aider 0.86.2, Goose 1.59.0.
Not determinable without installing: Droid, Cursor CLI (2026.08.11 in forum reports), Kiro CLI,
ForgeCode, Antigravity CLI (1.4.10 in third-party guides [UNVERIFIED]). Docs for those were read as
published on the day.

## 1. Result per agent

| Agent | Version | pre_edit_block | context_inject | per_session_config | Mechanism | Evidence |
| --- | --- | --- | --- | --- | --- | --- |
| Pi | 1.5.2 | yes: `tool_call` returns `{ block: true, reason }`; `write` input field names unverified | yes: `before_agent_start` `message` or `systemPrompt` | yes: `pi -e <file.ts>` (argv); CLI extensions are not project-local, so no project trust prompt | In-process TypeScript extension (jiti) | pi.dev/docs/latest/extensions; npm `@earendil-works/pi-coding-agent` docs/extensions.md |
| Oh My Pi | 18.19.0 | yes [UNVERIFIED for 18.x: legacy `tool_call` hook returns `{ block, reason }`] | [UNVERIFIED] | yes [UNVERIFIED]: `omp --extension <file>` plus `--trusted-extension <abs path>`, `--no-extensions` for ambient ones | Pi-derived extension API | omp README (trust flags), third-party omp hook docs |
| OpenCode | 1.19.5 | yes: plugin `tool.execute.before` throws to block; `edit`/`write` args shape undocumented | partial [UNVERIFIED]: system-prompt transform hook not in the docs read | yes: `OPENCODE_CONFIG_DIR` (plugins folder) or `OPENCODE_CONFIG_CONTENT` with a `plugin` entry | In-process Bun plugin | opencode.ai/v1/docs/plugins, opencode.ai/v1/docs/config |
| Droid (Factory) | n/a | yes: `PreToolUse`, matcher `Edit`/`Create`/`ApplyPatch`, `permissionDecision` deny or exit 2 | yes: `SessionStart`, `UserPromptSubmit`, `PostToolUse` | probable: `--settings <path>` "applies a settings file on top ... for the current process only"; docs show it only with `droid exec` and do not say it carries `hooks` | Claude-style command hooks | docs.factory.com hooks-guide, reference/cli-reference |
| Cline CLI | 3.0.74 | probable: `PreToolUse` hook returns `cancel`; the current docs moved hooks to "SDK plugins" and no longer describe the file contract | yes: `contextModification` / plugin `appendContext` | probable: `--hooks-dir` or `CLINE_HOOKS_DIR` | Executable per event, or SDK plugin | docs.cline.bot/cli/cli-reference, sdk/plugins, v3.36 blog |
| Amp | 0.0.1794499251-g8dc9ce | yes: plugin `tool.call` returns `reject-and-continue` (helper `filesModifiedByToolCall` gives paths); plugins are experimental | yes: `agent.start` returns a message | not documented: plugins load from `.amp/plugins/` (repo) or the system folder; `--settings-file` has no plugin key | Bun plugin | ampcode.com/docs/plugin-api, docs/customize/plugins, docs/cli/settings |
| Crush | 0.101.0 | yes: `PreToolUse` exit 2 or `decision: deny`; path in `CRUSH_TOOL_INPUT_FILE_PATH` | yes: `context` field | partial: hooks in `crushrc`; `CRUSH_GLOBAL_CONFIG` moves the whole user config | Command hooks, embedded POSIX shell on Windows | charmbracelet/crush docs/hooks/README.md, README |
| Mistral Vibe | 3.1.2 | yes: `pre_tool` `decision: deny`; `write_file`, `edit`; path key undocumented | partial: `post_tool` `additional_context` only | partial: `VIBE_HOME` moves config, `.env` keys, plugins, logs; project `hooks.toml` needs folder trust | Command hooks, Git Bash or PowerShell | docs.mistral.ai/vibe/code/cli/hooks, configuration |
| Goose | 1.59.0 | yes: `PreToolUse` exit 2 or `{"decision":"block"}` | no | no: only `~/.agents/plugins/<n>/` and `<project>/.agents/plugins/<n>/` | Open Plugins hooks, `sh -c` | goose-docs.ai context-engineering/hooks, plugins |
| Kimi Code | 2.6.0 | yes: `PreToolUse` exit 2 or `permissionDecision` deny; path field undocumented | yes: stdout on exit 0 | no: only `$KIMI_CODE_HOME/config.toml` or installed plugins | Command hooks | moonshotai.github.io/kimi-code customization/hooks, plugins |
| Antigravity CLI | n/a | yes: `PreToolUse` `decision: deny`, path in `TargetFile` | yes: `injectSteps` | no: `~/.gemini/config/hooks.json` or `<ws>/.agents/hooks.json` only; flags override settings, not hooks | Command hooks | antigravity.google/docs/hooks, docs/settings |
| Cursor CLI | 2026.08.11 | yes on paper: `preToolUse` `permission: deny`, `Write` matcher; `file_path` undocumented; CLI reliability poor | yes: `sessionStart` `additional_context` | no: `~/.cursor/hooks.json`, project `.cursor/hooks.json` | Command hooks | cursor.com/docs/agent/hooks, forum threads (raw record) |
| Kiro CLI | n/a | yes: `PreToolUse` exit 2; matcher `write` = `fs_write`; payload path unverified | yes: stdout of `SessionStart`/`agentSpawn` | no: `.kiro/hooks/`, `~/.kiro/hooks/`, agent files; no Windows CLI documented | Command hooks | kiro.dev/docs/hooks, reference/exit-codes |
| Aider | 0.86.2 | no | no | n/a | only `--lint-cmd` and `--test-cmd` after an edit | aider.chat/docs/usage/lint-test.html |
| ForgeCode | n/a | no hooks documented [UNVERIFIED] | no | n/a (`FORGE_CONFIG` moves the base dir) | none | forge README |
| Kode | 2.2.1 | no hooks documented [UNVERIFIED]; only `permissions` allow/deny/ask rules | no | partial: `KODE_CONFIG_DIR` | rules, not hooks | Kode-CLI README, docs/develop/configuration.md |
| Codebuff | 1.0.688 | no hooks documented [UNVERIFIED] | no | n/a | none found | codebuff README |

## 2. Recipes for the feasible agents

Shared design: the gate request is the Claude-shaped `/v1/pre-tool-use` call from [[team-service-v1]]
(`tool_name` mapped to `Edit` or `Write`, `tool_input.file_path`, bearer `HANGAR_HOOK_TOKEN` from the
child environment). Never write the token into a session file. Hangar chooses the fail policy:
in-process agents (Pi, OMP, OpenCode) can be made fail closed on a fetch error; command-hook agents
are fail open on crash or timeout.

### Pi (1st)
- File: `<data>/gate/<session>/hangar-gate.ts`. It registers `pi.on("tool_call", ...)` for `write`
  and `edit`, calls `fetch("http://127.0.0.1:<port>/v1/pre-tool-use", ...)` and returns
  `{ block: true, reason }` on deny. A `before_agent_start` handler returns a `message` for context.
- argv: `pi -e <abs>/hangar-gate.ts` plus the user arguments. Env: `HANGAR_HOOK_TOKEN`.
- Direct HTTP: yes, no helper. Port and session id may go into the file; the token only via env.
- Pitfalls: field names of `write` input unknown (`edit` is `path` plus `edits[]`); resolve `path`
  against `cwd`; `-e` is documented as a quick-test flag; shell tools write without a path. Fail
  behaviour of a throwing `tool_call` handler is undocumented: catch errors in the extension.

### Oh My Pi (2nd)
- Same extension file, argv `omp --extension <file> --trusted-extension <file>` [UNVERIFIED that
  `--trusted-extension` accepts the same file and that no prompt remains]. The legacy `HookAPI` and
  the newer `ExtensionAPI` both exist; the version-matched docs ship with the installed omp.
- Direct HTTP: yes. Windows: native binary, no WSL.

### OpenCode (3rd)
- File: `<data>/gate/<session>/opencode/plugins/hangar-gate.ts` exporting a plugin with
  `tool.execute.before` (`input.tool`, `output.args.filePath`; throw `Error` to deny).
- Env: `OPENCODE_CONFIG_DIR=<data>/gate/<session>/opencode` (searched for plugins, loaded after the
  global config, so the user config stays) or `OPENCODE_CONFIG_CONTENT={"plugin":["file:///..."]}`.
  Whether `plugin` arrays of several sources concatenate is undocumented [UNVERIFIED].
- Direct HTTP: yes (Bun `fetch`). Keep the plugin free of dependencies: local plugins with
  dependencies trigger `bun install` at startup.
- Pitfalls: the 1.x (`opencode-ai`) and 2.x (`@opencode/cli`) docs differ; the catalog follows 1.x.

### Droid (4th)
- File: `<data>/gate/<session>/droid-settings.json` with `hooks.PreToolUse` = one group, matcher
  `Edit|Create|ApplyPatch`, one command hook with `timeout` 660.
- argv: `droid --settings <file>` plus user arguments. [UNVERIFIED] that the flag works in the
  interactive REPL and that its `hooks` key merges with user hooks (the doc example is `droid exec`).
- Helper needed: `hangar gate-hook droid pre-tool-use`. Deny: exit 2 plus stderr, or
  `hookSpecificOutput.permissionDecision: deny`. `allowManagedHooksOnly` in an enterprise setup drops
  the hook; other non-zero exits do not block.

### Conditional candidates (not before a measurement)
- **Cline:** `CLINE_HOOKS_DIR=<dir>` with an executable `PreToolUse`; the v3.36 blog says "Windows
  support is not available", the current CLI docs only mention `.cline/hooks`.
- **Amp:** plugin `tool.call` is clean, but plugins load only from the project `.amp/plugins/` or
  the system folder; changing `XDG_CONFIG_HOME` per session would also move the user settings.
- **Crush:** `CRUSH_GLOBAL_CONFIG=<dir>` replaces the whole user config, so the user would lose
  providers and keys unless they come from env vars.
- **Vibe:** `VIBE_HOME=<dir>` also moves `.env` (API keys) and agents, so the user is signed out.

## 3. Ranking, easiest first

1. **Pi**: one argv flag, direct HTTP, no helper, no trust prompt for CLI extensions.
2. **Oh My Pi**: same shape, explicit trust flags, less documented.
3. **OpenCode**: one env var, direct HTTP; version line and array merging to measure.
4. **Droid**: one argv flag and Claude-style hooks, but a helper command and an unverified flag scope.
5. Cline, 6. Amp, 7. Crush, 8. Vibe: only after a measurement, each with the caveat above.

## 4. Not possible (today)

| Agent | Reason |
| --- | --- |
| Aider | No hook of any kind before an edit; `--lint-cmd` and `--test-cmd` run after it. |
| Goose | `PreToolUse` can block, but hooks are discovered only in `~/.agents/plugins` and `<project>/.agents/plugins`; no env or flag for one session. |
| Kimi Code | Hooks only in `~/.kimi-code/config.toml` or installed plugins; `KIMI_CODE_HOME` moves login and sessions too. |
| Antigravity CLI | Hooks only in `~/.gemini/config/hooks.json` or `<ws>/.agents/hooks.json`; flags override settings, not hooks. |
| Cursor CLI | Only user and project `hooks.json`; CLI reports of hooks not firing (Linux 2026.08.11, Windows BOM on stdin). |
| Kiro CLI | Hooks in `.kiro/hooks/`, `~/.kiro/hooks/`, agent files; no per-run path; no documented Windows CLI. |
| ForgeCode | No hook mechanism documented. |
| Kode | No hooks documented; only permission rules. |
| Codebuff | No hook or permission mechanism documented. |

## 5. Risks

- Hooks see tool calls, not file writes: shell tools write files without a path. The file watcher
  stays the second line, as in [[team-service-v1]].
- Fail open: Droid, Crush and Kimi treat crashes and timeouts as non-blocking; Vibe only with
  `strict = false`; Goose denies only with `on_failure: "block"`. Pi, OMP and OpenCode: the plugin decides.
- Home relocation (`VIBE_HOME`, `KIMI_CODE_HOME`, `KIRO_HOME`, `CLINE_DIR`, `CRUSH_GLOBAL_CONFIG`)
  moves credentials and history; never use it as a gate carrier.
- Moving targets: OpenCode split into 1.x and 2.x; Cline replaced file hooks with SDK plugins; Amp
  plugins are experimental.
- Sources disagree on Kiro exit codes (warning or block), Cline Windows support and whether
  Antigravity loads hooks from `settings.json`; the page follows the official vendor pages.

## 6. Open questions (each needs a measurement)

1. Pi and OMP: real `write` and `edit` input fields; does a thrown error in `tool_call` block or
   pass; no trust prompt with `-e`?
2. OpenCode: does `OPENCODE_CONFIG_DIR/plugins` load next to user plugins; `edit` and `write` arg
   names; 1.x only?
3. Droid: does `--settings` carry `hooks` in the REPL; payload for `Edit`; Windows hook shell.
4. Cline: hook file contract and Windows support in 3.x. Amp: any per-session plugin path.
5. Catalog: the feasible agents need new hook formats (`pi_extension`, `opencode_plugin`,
   `droid_settings`) in `agent_catalog.toml` and `gate_service.rs`.

## Significance for Hangar

- Pi, Oh My Pi, OpenCode and Droid can become team-capable; Pi, OMP and OpenCode without the helper
  command. Following the rule that Hangar offers only what works, every agent in section 4 and the
  conditional candidates stay off the Team list until a measurement says otherwise.
