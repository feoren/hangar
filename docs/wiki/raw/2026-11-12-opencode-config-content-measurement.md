# Raw: OpenCode 1.19.5 loading Hangar's gate plugin from OPENCODE_CONFIG_CONTENT

Date: 2026-11-12. Platform: Windows 11 x64. Approved by the owner (at most three runs, free
model, throwaway repository). Follow-up to `2026-11-12-opencode-gate-live-measurement.md`, which
used `OPENCODE_CONFIG_DIR` and saw OpenCode run an npm install in that folder. Paths shortened:
`<repo>` is the Hangar checkout.

## Setup

- OpenCode 1.19.5, platform binary started directly (the npm shim is broken on this machine),
  model `opencode/deepseek-v4.1-flash-free`, no account.
- Throwaway git repository `<repo>\tests\.tmp\oc2-measure` (one empty commit), deleted afterwards.
- Plugin file `<repo>\tests\.tmp\oc2-gate\hangar-gate.ts` = `gate_plugins/core.js` +
  `opencode.js` as built after the review (own-key tool table, patch keys only, paths as given).
  The folder held nothing else.
- `OPENCODE_CONFIG_CONTENT={"plugin":["file:///<repo>/tests/.tmp/oc2-gate/hangar-gate.ts"]}`;
  `OPENCODE_CONFIG_DIR` not set. `HANGAR_GATE=<port>:<token>` only in the child's environment.
- User plugin: a temporary `%USERPROFILE%\.config\opencode\plugins\hangar-measure-user.js` that
  appended `loaded` and `tool <name>` to a log under `<repo>\tests\.tmp`. The `plugins` folder did
  not exist before and was removed afterwards with the file. The user's `opencode.json` has no
  `plugin` key (`$schema`, `provider` only).
- Endpoint: the same Python probe as before (token check, masked log; deny or empty answer).
- Command: `opencode.exe run --model opencode/deepseek-v4.1-flash-free "create hello.txt containing hi"`

## Run 1 (probe denies)

Probe requests: two, both `token_ok: true`, `browser_headers: false`, body
`{"hook_event_name": "PreToolUse", "tool_name": "Write", "tool_input": {"file_path":
"<repo>\\tests\\.tmp\\oc2-measure\\hello.txt"}, "cwd": "<repo>\\tests\\.tmp\\oc2-measure"}`.

After the run the repository held only `.git`: `hello.txt` was not written. The model then ran
several `bash` commands to look around, listed the neighbouring `oc2-gate` folder and tried to
read `hangar-gate.ts`, which OpenCode refused itself:

```
! permission requested: external_directory (<repo>\tests\.tmp\oc2-gate\*); auto-rejecting
✗ Read <repo>/tests/.tmp/oc2-gate/hangar-gate.ts failed
Error: The user rejected permission to use this specific tool call.
```

User plugin log:

```
loaded
tool write
tool read
tool write
tool bash
tool bash
tool bash
tool bash
tool read
```

Session folder afterwards: `hangar-gate.ts` (8574 bytes) only. `%USERPROFILE%\.config\opencode`:
no file changed (newest modification time 2026-05-22, apart from the test `plugins` folder).

## Run 2 (probe answers "no decision")

```
> build · deepseek-v4.1-flash-free
← Write hello.txt
Wrote file successfully.
```

One probe request, same shape, `token_ok: true`. `hello.txt` contained `hi`. User plugin log:
`loaded`, `tool write`. Session folder afterwards: `hangar-gate.ts` only.

Run 3 was not needed.
