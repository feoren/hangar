# Measurement: project settings against `--settings` http hooks (Claude Code 2.1.331)

Date: 2026-11-24. Platform: Windows 11 Pro (10.0.26300), Git Bash. Claude Code 2.1.331.

## Setup

- A throwaway git repository (`git init`) under `tests/.tmp/hook-measure-<case>/repo` with one
  harmless marker file and, per case, one `.claude/settings.json` (or `.claude/settings.local.json`).
- A local Python listener on `127.0.0.1:47811` that logs path, `hook_event_name` and whether an
  `Authorization: Bearer marker` header arrived, and answers an empty 200.
- One run per case, from the repository root:

```
HANGAR_HOOK_TOKEN=marker claude -p "Reply with the single word ok." --model haiku --settings '{"hooks":{"UserPromptSubmit":[{"hooks":[{"type":"http","url":"http://127.0.0.1:47811/v1/user-prompt-submit","timeout":5,"headers":{"Authorization":"Bearer $HANGAR_HOOK_TOKEN"},"allowedEnvVars":["HANGAR_HOOK_TOKEN"]}]}],"Stop":[{"hooks":[{"type":"http","url":"http://127.0.0.1:47811/v1/stop","timeout":5}]}]}}'
```

## Results (listener log per case; every run printed `ok`)

| Case | Project file content | Hooks that reached the listener |
| --- | --- | --- |
| baseline | none | `/v1/user-prompt-submit UserPromptSubmit auth`, `/v1/stop Stop noauth` |
| disable | `settings.json`: `{"disableAllHooks":true}` | none |
| local-disable | `settings.local.json`: `{"disableAllHooks":true}` | none |
| disable-false | `settings.json`: `{"disableAllHooks":false}` | both, with auth |
| urls-empty | `settings.json`: `{"allowedHttpHookUrls":[]}` | none |
| urls-other | `settings.json`: `{"allowedHttpHookUrls":["https://example.invalid/*"]}` | none |
| urls-match | `settings.json`: `{"allowedHttpHookUrls":["http://127.0.0.1:*"]}` | both, with auth |
| envvars-empty | `settings.json`: `{"httpHookAllowedEnvVars":[]}` | both, **without** auth (`noauth`) |
| envvars-match | `settings.json`: `{"httpHookAllowedEnvVars":["HANGAR_HOOK_TOKEN"]}` | both, with auth |

One more run with the working folder in a subfolder (`repo/app`) and
`repo/.claude/settings.json` = `{"disableAllHooks":true}` at the repository root: the
`UserPromptSubmit` hook reached the listener. Claude Code 2.1.331 read the project settings of the
working folder only in that run, not of the git root above it.

The `Stop` hook had no `headers`, hence `noauth` in every case.
