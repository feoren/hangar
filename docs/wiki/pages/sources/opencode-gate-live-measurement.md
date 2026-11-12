---
title: "Measurement: OpenCode 1.19.5 live run with Hangar's gate plugin"
type: source
tags: [hooks, gate, opencode, plugin, windows, measurement, live]
sources: []
raw: raw/2026-11-12-opencode-gate-live-measurement.md, raw/2026-11-12-opencode-config-content-measurement.md
url:
retrieved: 2026-11-12
created: 2026-11-12
updated: 2026-11-12
---

# Measurement: OpenCode 1.19.5 live run with Hangar's gate plugin

Three live `opencode run` turns on Windows 11 (OpenCode 1.19.5, the free
`opencode/deepseek-v4.1-flash-free` model, no account), approved by the owner, in a throwaway git
repository that was deleted afterwards. The start used exactly Hangar's recipe:
`OPENCODE_CONFIG_DIR=<folder>` with `plugins/hangar-gate.ts` as `hook_recipes::opencode_plugin()`
writes it, `HANGAR_GATE=<port>:<token>` in the child's environment only. A Python probe stood in
for Hangar's endpoint. Record: `raw/2026-11-12-opencode-gate-live-measurement.md`. Answers
section 6, question 2 of [[agent-hooks-remaining-agents-research]] for [[agent-gate-adapters]].

## Key points

- **The plugin loads from `OPENCODE_CONFIG_DIR/plugins`** and `tool.execute.before` fires before
  the write tool runs: the probe saw the request, then the file was still missing.
- **The token arrives**: `HANGAR_GATE` was readable in the plugin; every request carried the
  right bearer token, `Host: 127.0.0.1:<port>` and no `Origin` or `Sec-Fetch-*` header
  (`node:http` under Bun).
- **A deny stops the write**: with the probe's deny, OpenCode showed `Write hello.txt failed` with
  Hangar's reason as the error, and `hello.txt` did not exist. With an empty answer the write went
  ahead (`Wrote file successfully`).
- **Tool and arguments**: the write tool is `write` with `filePath` (absolute) and `content`;
  the plugin context's `directory` is the session's folder.
- **Shell fallback**: after two denied writes the model wrote the file with a `bash` call
  (PowerShell `Set-Content`). Shell writes are not hooked, as for every agent.
- **Side effect**: on the first start OpenCode installed `@opencode-ai/plugin` 1.19.5 with npm
  into the config folder (`package.json`, `package-lock.json`, `node_modules`), although the
  plugin imports nothing. The npm shim `opencode.cmd` was broken on this machine (postinstall not
  run), so the platform binary was started directly.

## Follow-up: the plugin named in `OPENCODE_CONFIG_CONTENT`

Two more live turns after the security review, same model and setup
(`raw/2026-11-12-opencode-config-content-measurement.md`), with
`OPENCODE_CONFIG_CONTENT={"plugin":["file:///<folder>/hangar-gate.ts"]}` and no
`OPENCODE_CONFIG_DIR`, plus a temporary logging plugin in the user's
`~/.config/opencode/plugins`:

- **The plugin loads from the URL**, the token arrives, and a deny keeps `hello.txt` unwritten
  (two denied `write` calls); with an empty answer the write goes ahead.
- **Nothing is installed**: the session folder held only `hangar-gate.ts` (8574 bytes) after
  both runs; no `package.json`, lock file or `node_modules`, and no file of the user's
  configuration changed.
- **The user's own plugins still load**: the logging plugin reported `loaded` and saw every tool
  call (`write`, `read`, `bash`) next to Hangar's.
- After the denies the model went looking around with `bash`, listed the plugin's folder next to
  the repository and tried to read the plugin file; OpenCode's own `external_directory`
  permission refused that read. In Hangar the folder lives in the app's data folder.

> [!warning] Contradiction
> [[agent-hooks-remaining-agents-research]] recommends `OPENCODE_CONFIG_DIR`. Measured with 1.19.5:
> OpenCode runs an npm install of `@opencode-ai/plugin` in such a folder on start (network access,
> install scripts with the session's environment). `OPENCODE_CONFIG_CONTENT` loads the same plugin
> without it, so Hangar uses that.

## Significance for Hangar

- OpenCode is team-capable from 1.19.5 (`measured = true`, `min_version = "1.19.5"`), with the
  plugin named in `OPENCODE_CONFIG_CONTENT`.
- Not measured: `edit`, `multiedit` and `apply_patch` argument names (the plugin reads the path
  keys and, for the patch tools only, `patchText`, `patch` and `input`), and how
  `OPENCODE_CONFIG_CONTENT` merges with a `plugin` list in the user's own `opencode.json` (the
  measuring machine has none; folder plugins load).
- Where `opencode --version` fails (as with the broken shim here) the version is unknown and
  OpenCode is not offered for team sessions.
