# Measurement record: Cursor CLI 2026.11.19 with a gate plugin (2026-11-22)

Platform: Windows 11 Pro 10.0.26300. Cursor CLI `2026.11.19-f005e9d`, signed in, started through
`cursor-agent.cmd` (cmd.exe, then Windows PowerShell running `cursor-agent.ps1`) or the
`cursor-agent` PowerShell script. Throwaway git repositories under `tests/.tmp/cursor-live/`,
deleted afterwards. Tokens masked. Before signing in, `cursor-agent status` printed
"Not logged in".

## Plugin folder

`<plugin>/.cursor-plugin/plugin.json` (`name`, `version`, `description`) and
`<plugin>/hooks/hooks.json`:

```
{"version":1,"hooks":{"preToolUse":[{"command":"<command>","timeout":60}]}}
```

Started with `--plugin-dir <plugin>`; `HANGAR_GATE=<port>:<token>` in the environment only.

## Runs 1 to 3: logging probe, headless (`-p --force --trust`)

Probe command `py -3 E:/.../hook.py pre` (logs stdin and whether `HANGAR_GATE` is set; in deny
mode prints `{"permission":"deny","user_message":..,"agent_message":..}` and exits 0).

- Run 1 (deny everything): the first call was `Read` and was rejected ("Rejected: Hangar probe:
  claimed by a teammate."). Input: `{"conversation_id":..,"generation_id":..,"model":"default",
  "tool_name":"Read","tool_input":{"file_path":"E:\\...\\repo\\hello.txt"},"tool_use_id":..,
  "session_id":..,"hook_event_name":"preToolUse","cursor_version":"2026.11.19-f005e9d",
  "workspace_roots":["E:\\...\\repo"],"user_email":"<masked>","transcript_path":null}`; no `cwd`
  field. `HANGAR_GATE` present.
- Run 2 (deny all but reads): `Write` `{"file_path":"E:\\...\\repo\\hello.txt","content":"hi"}`
  rejected; `hello.txt` not written.
- Run 3 (allow): create, edit, delete, then `git status`. Hook calls: `Read`, `Write` (create),
  `Write` for the edit of `notes.txt` (whole new content), `Delete` `{"file_path":..}`, `Shell`
  `{"command":"git status","cwd":"E:\\...\\repo","timeout":30000}`. All carried out.

## Run 4: hook command in PowerShell form, interactive in a ConPTY

Command `& '<py.exe>' -3 'E:/.../hook.py' powershell-form`; started as
`cursor-agent.cmd --plugin-dir <plugin> "<prompt>"` in a fresh repository. Cursor first showed
"Workspace Trust Required ... [a] Trust this workspace [q] Quit"; nothing ran before it was
answered. After `a`: the hook ran (PowerShell form works, so the hook shell is PowerShell here),
`Read` then `Write`; the screen showed "Hangar probe: claimed by a teammate. Agent note: Do not
suggest workarounds to the blocked tool." and "Schreibzugriff verweigert: Hangar probe: claimed by
a teammate."; `hello.txt` not written. No trust prompt for the plugin itself.

## Run 5: matcher

`"matcher":"Write|Delete"`, allow, prompt: read `notes.txt`, create `three.txt`, delete
`old2.txt`, run `git status`. The hook saw only `Write` and `Delete`.

## Run 6: Hangar's helper, end to end

Hook command `& '<worktree>\src-tauri\target\debug\hangar.exe' gate-hook cursor pre-tool-use`,
matcher `Write|Delete`, timeout 660, a Python probe as the endpoint answering deny. Prompt: create
`hello.txt`, delete `old.txt`. Endpoint requests (bearer token correct):

```
{"cwd":"E:\\...\\repo3","hook_event_name":"PreToolUse","tool_input":{"file_path":"E:\\...\\repo3\\hello.txt"},"tool_name":"Write"}
{"cwd":"E:\\...\\repo3","hook_event_name":"PreToolUse","tool_input":{"file_path":"E:\\...\\repo3\\old.txt"},"tool_name":"Delete"}
```

Cursor reported "Error: Hangar probe: this file is claimed by a teammate." for `Write` and "File
deletion rejected: Hangar probe: ..." for `Delete`; `hello.txt` was not created and `old.txt`
still existed.
