# Raw: per-session edit gates for the remaining agent CLIs

Retrieved 2026-11-12. Desk research against vendor docs, GitHub sources, npm and PyPI metadata.
No agent was installed or run. Findings are paraphrased.

## Sources and findings

- Pi: https://pi.dev/docs/latest/extensions and
  https://cdn.jsdelivr.net/npm/@earendil-works/pi-coding-agent@1.5.2/docs/extensions.md. `tool_call`
  blocks with `{ block, reason, terminate }`; `-e`/`--extension`; `project_trust` concerns only
  project-local extensions; `before_agent_start` returns `message` or `systemPrompt`; `edit` input is
  `path` plus `edits`.
- Oh My Pi: https://raw.githubusercontent.com/can1357/oh-my-pi/main/README.md (`--no-extensions`,
  `--trusted-extension`, `--config <file>`); third-party hook docs.
- OpenCode: https://opencode.ai/v1/docs/plugins.md, https://opencode.ai/v1/docs/config/ (env vars,
  merge order), https://api.github.com/repos/anomalyco/opencode/releases/latest (v1.19.5, 2026-10-28).
- Droid: https://docs.factory.com/cli/configuration/hooks-guide,
  https://docs.factory.com/reference/cli-reference (`--settings`),
  https://docs.factory.com/cli/configuration/settings.
- Cline: https://docs.cline.bot/cli/cli-reference.md, https://docs.cline.bot/sdk/plugins.md,
  https://cline.bot/blog/cline-v3-36-hooks ("Windows support is not available").
- Amp: https://ampcode.com/docs/plugin-api, https://ampcode.com/docs/customize/plugins,
  https://ampcode.com/docs/cli/settings.
- Crush: https://raw.githubusercontent.com/charmbracelet/crush/main/docs/hooks/README.md and README.md.
- Vibe: https://docs.mistral.ai/vibe/code/cli/hooks, /configuration, /configuration-reference.
- Goose: https://goose-docs.ai/docs/guides/context-engineering/hooks/, /plugins,
  https://goose-docs.ai/blog/2026/05/14/goose-hooks.
- Kimi: https://moonshotai.github.io/kimi-code/en/customization/hooks, /configuration/config-files,
  /customization/plugins.
- Antigravity: https://antigravity.google/docs/hooks?tab=cli, https://antigravity.google/docs/settings?tab=cli.
- Cursor: https://cursor.com/docs/agent/hooks, https://forum.cursor.com/t/cursor-cli-hooks/148511,
  https://forum.cursor.com/t/hooks-not-firing-cannot-have-guardrails/168407 (Windows BOM, staff reply
  2026-08-14).
- Kiro: https://kiro.dev/docs/hooks/, /docs/hooks/types/, /docs/reference/exit-codes/,
  /docs/reference/cli-commands/, /docs/cli/v3/hooks-migration/.
- Aider: https://aider.chat/docs/usage/lint-test.html.
- ForgeCode, Kode, Codebuff: project READMEs plus Kode `docs/develop/configuration.md` and
  `docs/compatibility.md`.
- Versions: npm registry `latest`, PyPI JSON, GitHub releases.

## Contradictions between sources

- Kiro: the exit-code page calls non-2 exits a warning, another page says they block.
- Cline: older docs say no Windows hooks; current CLI docs name hook folders without a platform note.
- Antigravity: one page says `settings.json` does not load hooks, another lists it as a location.
