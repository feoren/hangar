# Measurement record: OpenCode 2.3.2 with a V2 gate plugin (2026-11-22)

Platform: Windows 11 Pro 10.0.26300. OpenCode 2.3.2 (npm `@opencode/cli`, `bin/opencode.exe`),
free model `opencode/deepseek-v4.1-flash-free`. Throwaway git repositories under
`tests/.tmp/gate-live/`, deleted afterwards. A background service `opencode serve --service` was
running on the machine (started by an earlier OpenCode use). Tokens masked.

## Setup

Plugin folder `<session>/hangar-gate/` with `index.ts`; environment
`OPENCODE_CONFIG_CONTENT={"plugins":["file:///E:/.../hangar-gate"]}` (folder URL, no trailing
slash) and `HANGAR_GATE=<port>:<token>`; every start with `--standalone`.

## Runs 1 to 3: logging probe plugin

`index.ts`: `import { Plugin } from "@opencode/plugin"`, `export default Plugin.define({ id:
"hangar.gate", async setup(ctx) { ... } })`, registering `ctx.permission.hook("evaluate", ...)`
(sets `effect = "deny"` and `message` for `action === "edit"` in deny mode) and
`ctx.tool.hook("execute.before", ...)` (logs only). Log lines record whether `HANGAR_GATE` is set.

Run 1, `opencode run --standalone -m <model> "Use your write tool to create hello.txt ..."`, deny:

```
✗ Write hello.txt failed
Error: Hangar probe: this file is claimed by a teammate.
{"type":"permission.rejected","message":"Hangar probe: this file is claimed by a teammate."}
```

Log: `setup` (gate true); `execute.before` `{"tool":"write","input":{"path":"hello.txt","content":"hi"}}`;
`evaluate` `{"action":"edit","resources":["hello.txt"],"effect":"allow"}`. `hello.txt` not written.
The plugin folder still held only `index.ts` afterwards (no `package.json`, no `node_modules`).

Run 2, allow: create `hello.txt`, edit `notes.txt` (beta to gamma), create `sub/deep.txt`. All
written. `execute.before`: `write` and `edit` with absolute `path` (Windows separators), `edit`
also `oldString`, `newString`. `evaluate` for `edit`: `["hello.txt"]`, `["notes.txt"]`,
`["sub/deep.txt"]` (relative, `/`). Reads arrive as `read` / action `read`.

Run 3, allow: shell and patch. The model had no `apply_patch` tool ("In meinem Werkzeugsatz gibt
es nur edit und write"). Shell calls: `execute.before`
`{"tool":"shell","input":{"command":"git status","workdir":"E:\\...\\oc2-repo\\sub"}}`; `evaluate`
`{"action":"shell","resources":["git status"]}` (a pipeline is split into one resource per
command).

## Run 4: interactive TUI in a ConPTY

`opencode.exe --standalone --prompt "Use your write tool to create hello.txt ..."` (the TUI has no
`-m`; the model came from `"model"` in the same `OPENCODE_CONFIG_CONTENT`). Without that key the
TUI's default model failed with `ECONNREFUSED: Unable to connect` (not related to the plugin). With
it: `setup` logged, `evaluate` `edit` `["hello.txt"]`, the screen showed
`Grund: permission.rejected Hangar probe: this file is claimed by a ...`, `hello.txt` not written.

## Runs 5 and 6: Hangar's plugin (`core.js` + `opencode2.js`) against the probe endpoint

`index.ts` = `src-tauri/src/gate_plugins/core.js` + `opencode2.js` (a `tool` hook
`execute.before` that asks the endpoint for edit tools and `shell`, and throws on deny).

Run 5, endpoint denies everything: prompt "run 'git status', then create hello.txt". The endpoint
got (bearer token correct):

```
{"hook_event_name":"PreToolUse","tool_name":"Shell","tool_input":{"command":"git status"},"cwd":"E:\\...\\oc2b-repo"}
{"hook_event_name":"PreToolUse","tool_name":"Write","tool_input":{"file_path":"E:\\...\\oc2b-repo\\hello.txt"},"cwd":"E:\\...\\oc2b-repo"}
```

Both calls failed with `Hangar probe: this file is claimed by a teammate.`; `hello.txt` not
written; the folder held only `index.ts`.

Run 6, endpoint allows: the same prompt; `git status` ran, `hello.txt` was written; the endpoint
saw `Shell`, `Write`.

## Without `--standalone` (earlier the same day)

With the 1.x recipe (`plugin` key, a `.ts` file) and no `--standalone`, the write went through and
the endpoint saw nothing (`raw/2026-11-22-opencode2-and-gemini-gate-findings.md`).
