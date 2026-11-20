# Reading: settings files that turn agent prompts off

Retrieved 2026-11-20 from the official docs (nothing run). Notes in English, key names verbatim.

## Claude Code
Sources: https://code.claude.com/docs/en/settings, https://code.claude.com/docs/en/permissions,
https://code.claude.com/docs/en/permission-modes, https://code.claude.com/docs/en/managed-settings

- Precedence, high to low: managed, `--settings`, `.claude/settings.local.json`,
  `.claude/settings.json`, `~/.claude/settings.json`. Lists merge, scalars come from the highest level.
- `CLAUDE_CONFIG_DIR` moves `~/.claude`, settings included.
- Managed file: Windows `C:\Program Files\ClaudeCode\managed-settings.json` (the old
  `C:\ProgramData\ClaudeCode` is no longer read), macOS
  `/Library/Application Support/ClaudeCode/managed-settings.json`, Linux and WSL
  `/etc/claude-code/managed-settings.json`. Also a `managed-settings.d` drop-in folder, a macOS
  profile, the Windows registry and server managed settings.
- `permissions.defaultMode`: `default`, `acceptEdits`, `plan`, `auto`, `dontAsk`, `bypassPermissions`.
- `bypassPermissions` runs without prompts; deny rules still hold, `ask` rules can still ask.
- `dontAsk` does not ask but denies every call that would have asked; it runs reads, read-only
  Bash, `allow` rules and hook approvals.
- Since v2.1.257 `bypassPermissions` and `auto` from `.claude/settings.json` and
  `.claude/settings.local.json` are ignored; they take effect from the user file, `--settings` or
  managed only. Older versions took them from project files.
- Start mode: `--permission-mode` or `--dangerously-skip-permissions`, then `permissions.defaultMode`,
  then the default (`auto` in the interactive terminal since v2.1.283).
- `permissions.disableBypassPermissionsMode: "disable"` blocks `bypassPermissions` from every scope.

## Codex CLI
Sources: https://learn.chatgpt.com/docs/config-file/config-basic,
https://learn.chatgpt.com/docs/config-file/config-reference, source `codex-rs/core/src/config/mod.rs`

- User file `$CODEX_HOME/config.toml`, default `~/.codex/config.toml`; `CODEX_HOME` is in the source.
- Precedence, high to low: flags and `-c`, project `.codex/config.toml` from the project root to the
  working folder (trusted projects only), profile file via `--profile`, user file, cloud managed,
  `/etc/codex/config.toml`, defaults.
- `approval_policy`: `on-request`, `never`, a `granular` table (`untrusted` removed, `on-failure`
  deprecated). `sandbox_mode`: `read-only`, `workspace-write`, `danger-full-access`.
- Profiles are files `$CODEX_HOME/<name>.config.toml` chosen with `--profile <name>`. The older
  `[profiles.x]` with `profile = "x"` appears only in third-party pages (unverified for current
  versions). A project file may not set `profile` or `profiles`.

## Gemini CLI
Sources: https://raw.githubusercontent.com/google-gemini/gemini-cli/main/docs/reference/configuration.md,
`packages/cli/src/config/settingsSchema.ts`

- `general.defaultApprovalMode` takes only `default`, `auto_edit`, `plan`. YOLO comes only from
  `--yolo` or `--approval-mode=yolo`, not from a file. `security.disableYoloMode` blocks it.

## Aider
Sources: https://aider.chat/docs/config/aider_conf.html, https://aider.chat/docs/config/dotenv.html

- `.aider.conf.yml` in the home folder, the git root and the working folder, loaded in this order,
  later wins; `--config <file>` loads only that file. Key `yes-always: true`.
- Every option is also `AIDER_<OPTION>` in the environment (`AIDER_YES_ALWAYS`); `.env` files in
  the home folder, the git root, the working folder, then `--env-file`.
- Precedence between arguments, environment and YAML is not stated on these pages.

## GitHub Copilot CLI
Sources: https://docs.github.com/en/copilot/how-tos/copilot-cli/set-up-copilot-cli/configure-copilot-cli,
https://docs.github.com/en/copilot/reference/cli-command-reference

- `~/.copilot/config.json` (`COPILOT_HOME`), `settings.json`, `.github/copilot/settings(.local).json`.
  No key for allowing all tools by default is documented; only `--allow-all`, `--yolo`,
  `--allow-all-tools` or `/yolo` at run time.
