# Measurement record: OpenCode 2.3.2 and Gemini CLI 0.70.0 with Hangar's gate recipes (2026-11-22)

Platform: Windows 11 Pro 10.0.26300. Throwaway git repositories under `tests/.tmp/gate-live/`,
deleted afterwards. Python probe as the endpoint, as in
`raw/2026-11-22-copilot-gate-live-measurement.md`. Tokens masked.

## OpenCode 2.3.2 (npm `@opencode/cli`, compiled `bin/opencode.exe`)

Plugin file = `src-tauri/src/gate_plugins/core.js` + `opencode.js` (as
`hook_recipes::opencode_plugin()`), named in
`OPENCODE_CONFIG_CONTENT={"plugin":["file:///E:/.../opencode-session/hangar-gate.ts"]}`,
`HANGAR_GATE=<port>:<token>`, probe answering deny.

```
opencode run -m opencode/deepseek-v4.1-flash-free "Use your write tool to create hello.txt ..."
> build · deepseek-v4.1-flash-free
← Write hello.txt
Created file successfully: hello.txt
```

The endpoint received no request; `hello.txt` was written. The plugin did not hold the write.

Binary strings (`grep -a` on `opencode.exe`): `OPENCODE_CONFIG_CONTENT` is still read;
`tool.execute.before` occurs 0 times; hook names such as `"tool.before"`, `"tool.after"`,
`"extension.before"`, `"extension.after"` occur in an Effect-style internal hook table.

## Gemini CLI 0.70.0 (npm `@google/gemini-cli`)

Settings file as `hook_recipes::gemini_settings()` writes it, in a folder under the user's
project tree, `GEMINI_CLI_SYSTEM_SETTINGS_PATH=<that file>`, `GEMINI_CLI_TRUST_WORKSPACE=true`.

```
gemini --approval-mode yolo -p "Use your write_file tool ..."
Security Warning: Skipping system settings file '<file>': Parent directory '<folder>' is insecure: A security check for '<folder>' failed and could not be completed. Please file a bug report. Original error: Der Befehl "Get-Acl" wurde im Modul "Microsoft.PowerShell.Security" gefunden, das Modul konnte aber nicht geladen werden. ...
Please set an Auth method in your C:\Users\<user>\.gemini\settings.json or specify one of the following environment variables before running: GEMINI_API_KEY, GOOGLE_GENAI_USE_VERTEXAI, GOOGLE_GENAI_USE_GCA
```

(exit code 41; the machine has no Gemini sign-in, so no turn ran.)

Bundle source (`bundle/chunk-GFBMTAPD.js`, `_doLoadSettings`): the system settings file and the
system defaults file (`GEMINI_CLI_SYSTEM_DEFAULTS_PATH`) both go through `loadSystemFile`, which
skips the file unless `isFileAndDirectorySecureSync` passes. `bundle/chunk-37D3S3FZ.js`: on
Windows the file and every ancestor folder are checked with a PowerShell `Get-Acl` script; a
result is insecure when the owner is not "a trusted administrator or SYSTEM account" or when other
groups have write permission. On POSIX: owner must be uid 0 and the mode must not be group or
other writable ("To fix this, run: sudo chown root:root"). Any failure of the check counts as
insecure. User settings (`~/.gemini/settings.json`) and workspace settings
(`<workspace>/.gemini/settings.json`) are loaded with the plain `load` without this check.
