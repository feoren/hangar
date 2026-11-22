---
title: "Measurement: OpenCode 2.3.2 live run with a V2 gate plugin"
type: source
tags: [hooks, gate, opencode, plugin, windows, measurement, live]
sources: []
raw: raw/2026-11-22-opencode2-gate-live-measurement.md
url:
retrieved: 2026-11-22
created: 2026-11-22
updated: 2026-11-22
---

# Measurement: OpenCode 2.3.2 live run with a V2 gate plugin

Six live turns on Windows 11 with OpenCode 2.3.2 and a free model, headless (`opencode run`) and
in the interactive TUI in a ConPTY, after the 1.x recipe had failed on 2.x
([[opencode2-and-gemini-gate-findings]]). The recipe follows the V2 plugin API and the
`--standalone` flag that the code research found. Record:
`raw/2026-11-22-opencode2-gate-live-measurement.md`.

## Key points

- **Three changes make the gate work on 2.x:** a V2 plugin (`export default Plugin.define({ id,
  setup(ctx) })`), named as a **folder** in **`plugins`** (not `plugin`) of
  `OPENCODE_CONFIG_CONTENT` as a `file://` URL, holding `index.ts`, and the start flag
  **`--standalone`**, which runs a private server that inherits the session's environment.
- **Nothing is installed.** `import { Plugin } from "@opencode/plugin"` resolves without a
  package; the folder held only `index.ts` after every run.
- **Hooks:** `ctx.tool.hook("execute.before", ...)` sees every tool call before it runs;
  throwing stops the call and the model gets the message. `ctx.permission.hook("evaluate", ...)`
  sees normalized resources and can set `effect = "deny"` with a `message`
  (`permission.rejected`). Hangar uses `execute.before`, as its 1.x plugin did.
- **Tool shapes in 2.3.2:** `write` `{path, content}` and `edit` `{path, oldString, newString}`
  with absolute paths; the shell tool is `shell` `{command, workdir}` with an absolute `workdir`.
  The model used had no `apply_patch`.
- **Interactive TUI:** loads the plugin the same way; a deny shows Hangar's reason. The TUI takes
  no `-m`; a default model that cannot connect is unrelated to the gate.
- **`HANGAR_GATE` arrives**, and Hangar's plugin (`core.js` + `opencode2.js`) sent `Shell` and
  `Write` requests with the right token and the session folder as `cwd`; deny stopped both, allow
  let both run.

## Significance for Hangar

- OpenCode 2.x is team-capable from 2.3.2: Hangar picks the V2 recipe by the installed version
  (`hook_recipes::opencode_v2`) and prepends `--standalone`. 2.0.0 up to 2.3.1 stay off
  (`version_gap`), 1.x from 1.19.5 keeps its recipe.
- A profile with `--server` gets no gate: that server never sees the session's environment.

> [!question] Unverified
> `apply_patch` and `multiedit` input names in 2.x, how `plugins` from `OPENCODE_CONFIG_CONTENT`
> merges with a `plugins` list in the user's own configuration, and whether 2.0.0 to 2.3.1 run
> the hooks the same way.
