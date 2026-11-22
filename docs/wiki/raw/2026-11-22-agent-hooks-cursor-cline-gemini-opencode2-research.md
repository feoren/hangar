# Research note: per-session edit hooks in OpenCode 2.x, Cursor CLI, Cline CLI and Gemini CLI (2026-11-22)

Read-only research on Windows 11 against the installed programs (bundles and binaries read with
`grep -a`, `--help`) plus vendor docs and forum threads, retrieved 2026-11-22. No agent was run
against a model for this note. Paths under the user's home are written as `~`.

## OpenCode 2.3.2 (`@opencode/cli`)

- Migration guide: V1 plugins do not run in V2; config key `plugin` became `plugins`; configured
  local paths must be directories (https://opencode.ai/docs/migrate-v1/,
  https://opencode.ai/docs/build/plugins/migrate-v1/).
- Binary: `ConfigPluginSource.scan` reads `plugins`; a `file://` entry that is a file logs
  "configured plugin path must be a directory" and is dropped; a directory resolves `server` or
  `index` (`.ts/.tsx/.js`). The virtual `OPENCODE_CONFIG_CONTENT` document is still read.
- V2 shape: `Plugin.define({ id, async setup(ctx) { ctx.permission.hook("evaluate", ...);
  ctx.tool.hook("execute.before", ...) } })` (https://opencode.ai/docs/build/plugins/server/;
  `@opencode/plugin@2.0.0` types `dist/promise/permission.d.ts`, `dist/promise/tool.d.ts`).
- The binary serves `@opencode/plugin` itself (Bun plugin `opencode-plugin-runtime`) and refuses
  copies from a plugin's `node_modules`.
- `opencode --help`: `--standalone  Run with a private server instead of the background service`.
  The background service (`serve --service`) is started with its own service configuration's
  environment; `--standalone` spawns `opencode serve --stdio --port 0` with the client's
  environment (`extendEnv:true`).
- npm: `@opencode/cli` 2.0.0 on 2026-09-11; `@opencode/plugin` 2.0.0 on 2026-09-12; `opencode-ai`
  1.19.6 on 2026-11-17 (1.x still maintained).

## Cursor CLI 2026.11.19 (`cursor-agent`)

- Hook events in the bundle include `preToolUse`, `postToolUse`, `beforeShellExecution`,
  `afterShellExecution`, `beforeMCPExecution`, `beforeReadFile`, `afterFileEdit`,
  `beforeSubmitPrompt`, `stop`, `sessionStart`, `sessionEnd`; no `beforeFileEdit`.
- The CLI runs `preToolUse` with `{tool_name, tool_input, tool_use_id, cwd}` and on
  `permission: "deny"` rejects the tool with `user_message`. The write executor uses
  `toolName: "Write"`, `{file_path, content}`; delete `toolName: "Delete"`, `{file_path}`.
- `--plugin-dir <path>`: "Load a local plugin directory (can be specified multiple times)";
  registered with source `cli-args` only when `enableUserLocalPlugins` (true for a signed-in local
  profile, false when not signed in); a team admin can turn local plugins off
  (`allowUserLocalPluginImports`). Manifest `.cursor-plugin/plugin.json` or `plugin.json`, hooks
  from the manifest or `hooks/hooks.json`.
- Docs (https://cursor.com/docs/agent/hooks): `{version: 1, hooks: {preToolUse: [{command,
  matcher, timeout, failClosed}]}}`; deny with `{"permission":"deny","user_message":..,
  "agent_message":..}` and exit 0, or exit 2; other exit codes, crashes and timeouts fail open
  unless `failClosed`.
- Forum reports: CLI plugin hooks fixed 2026-08-27
  (https://forum.cursor.com/t/cursor-cli-ignores-hooks-from-marketplace-plugin/163890); on Windows
  a hook command is built for PowerShell but run by Git Bash when Git Bash is an ancestor, a silent
  deny, still reproduced on 2026.11.05
  (https://forum.cursor.com/t/hooks-never-run-in-cursor-agent-cli-on-windows-git-bash-the-failure-becomes-a-silent-hard-deny-still-repro-on-2026-09-02/170945).
- Measured on this machine: `cursor-agent status` prints "Not logged in".

## Cline CLI 3.0.75

- Hooks: one executable per event (`PreToolUse` with an extension such as `.ps1`, `.js`, `.py`)
  searched in `~/Documents/Cline/Hooks`, `<CLINE_DIR or ~/.cline>/hooks`,
  `<workspace>/.clinerules/hooks`, `<workspace>/.cline/hooks`. Deny: stdout JSON
  `{"cancel":true,"errorMessage":..}`, which stops the whole run.
- `--hooks-dir` sets `process.env.CLINE_HOOKS_DIR`, but nothing in the binary or `@cline/*/dist`
  reads that variable (one occurrence of `HOOKS_DIR`).
- `--config <dir>` moves the hooks folder together with agents, skills, rules, workflows,
  plugins and tasks.

## Gemini CLI 0.70.0

- The ownership check for system settings came with PR #29115 (merged 2026-09-04,
  https://github.com/google-gemini/gemini-cli/pull/29115); first stable release with it: 0.60.0.
- Workspace `.gemini/settings.json` hooks run only in a trusted folder; new project hooks are
  recorded in `~/.gemini/trusted_hooks.json`; the file would sit in the worktree, visible and
  editable by the agent.
- `GEMINI_CLI_HOME` moves `settings.json`, `oauth_creds.json`, `google_accounts.json`, trust files,
  extensions and history: an OAuth user is signed out.
- Extensions load only from `<home>/.gemini/extensions`; `-e` selects among installed ones.
- `--policy` files hold static rules; an `external` safety checker resolves built-in names only.
