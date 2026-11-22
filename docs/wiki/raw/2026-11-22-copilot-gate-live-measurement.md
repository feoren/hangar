# Measurement record: Copilot CLI 1.0.114 with Hangar's gate plugin (2026-11-22)

Platform: Windows 11 Pro 10.0.26300. Copilot CLI `GitHub Copilot CLI 1.0.114.` (`copilot.exe`
from VS Code's `bin/copilot-shim`), the account's default model (Copilot picked "MAI Code 2
Flash"). Throwaway git repositories under `tests/.tmp/gate-live/`, deleted afterwards. Hangar's
helper: the installed release `hangar.exe` 0.13.0 (`gate-hook copilot pre-tool-use`). A Python
probe stood in for the endpoint (`127.0.0.1:<port>`, `POST /v1/pre-tool-use`, answers Claude
Code's `hookSpecificOutput` deny, or an empty 200 for "go ahead"). Tokens below are masked.

## Setup

Plugin folder as `hook_recipes::copilot_manifest()` and `copilot_hooks()` write it
(`plugin.json` with `"hooks": "hooks.json"`; `hooks.json` version 1, `preToolUse`, `exec` =
the Hangar executable, `args` = `["gate-hook","copilot","pre-tool-use"]`, matcher
`edit|create|apply_patch`, `timeoutSec` 660). A second `preToolUse` entry ahead of it ran a
logging probe (`py -3 logstdin.py <log>`) that records stdin and whether `HANGAR_GATE` is set.
Environment: `HANGAR_GATE=<port>:<token>` only.

## Run 1: headless, deny

```
copilot --plugin-dir <folder>/copilot-plugin --allow-all-tools -p "Use your file creation tool to create the file hello.txt ..."
```

Note: a first attempt used a token with `-`; the helper refused it as malformed
(`HANGAR_GATE is missing or malformed`, the edit went ahead). Hangar's tokens are alphanumeric;
the run was repeated with an alphanumeric token.

Output (excerpt):

```
 ✗ Edit  Create hello.txt  Denied by preToolUse hook: Hangar probe: this file is claimed by a teamm…
   Denied by preToolUse hook: Hangar probe: this file is claimed by a teammate.
Refused: Hangar probe: this file is claimed by a teammate.
```

`hello.txt` did not exist afterwards. Logged hook input:

```
{"gate_env": true, "stdin": "{\"sessionId\":\"...\",\"timestamp\":1795346517817,\"cwd\":\"E:\\\\...\\\\copilot-repo\",\"toolCallId\":\"custom_call_...\",\"toolName\":\"apply_patch\",\"toolArgs\":\"*** Begin Patch\\n*** Add File: hello.txt\\n+hi\\n*** End Patch\\n\"}"}
```

Endpoint saw (bearer token correct, `Host: 127.0.0.1:<port>`):

```
{"cwd":"E:\\...\\copilot-repo","hook_event_name":"PreToolUse","tool_input":{"file_path":"hello.txt"},"tool_name":"apply_patch"}
```

## Run 2: headless, allow, add and update

Prompt: create `hello.txt` with `hi`, and in the existing `notes.txt` replace `beta` with `gamma`.
Copilot made one `apply_patch` call with absolute paths:

```
*** Begin Patch\n*** Add File: E:\\...\\copilot-repo\\hello.txt\n+hi\n*** Update File: E:\\...\\copilot-repo\\notes.txt\n@@\n-alpha\n-beta\n+alpha\n+gamma\n*** End Patch\n
```

The endpoint got two requests, one per path (`...\\hello.txt`, `...\\notes.txt`), answered
empty; both files were written (`Changes +2 -1`).

## Run 3: interactive in a ConPTY, deny

Driven with pywinpty (160x50), fresh repository never opened by Copilot:

```
copilot --plugin-dir <folder>/copilot-plugin --allow-all-tools -i "Use your file creation tool to create hello.txt with the content hi. No shell commands."
```

Copilot first showed its usual folder trust dialog ("Confirm folder trust ... Do you trust the
files in this folder? 1. Yes 2. Yes, and remember ... 3. No (Esc)"). Nothing ran before it was
answered (a 90 s run without an answer produced no hook call). After Enter (option 1):

```
 ● ○ Edit  Create hello.txt
 ◎ ✗Denied by preToolUse hook: Hangar probe: this file is claimed by a teammate.
```

`hello.txt` was not written. The model then stored the content with Copilot's own "session
checkpoint artifacts" tool (`notes/hello.txt` inside Copilot's session state, not the repository).
No plugin-specific trust prompt appeared.
