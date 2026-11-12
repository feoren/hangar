# Raw: Codex CLI 0.167.2 live run with Hangar's hook recipe

Measured 2026-11-12 on Windows 11 Pro 10.0.26300, `codex-cli 0.167.2` started through npm's
`codex.cmd` (the first `codex` on PATH), model chosen by the account default (`gpt-6.1-sol`),
`model_reasoning_effort=low`. Three runs, approved by the owner, in a throwaway git repository
under `tests/.tmp/codex-measure/` that was deleted afterwards. Paths below are masked as
`<repo>` and `<user>`.

## Setup

The hook command is a logging probe instead of `hangar gate-hook`: a Python script that appends
its argv, whether `HANGAR_GATE` is in its environment and its stdin to `hook-log.jsonl`, then
answers per mode. Driver argv (Python `subprocess`, list form; the `-c` value carries no `"`, as
Hangar's recipe through a cmd.exe launcher):

```text
codex --dangerously-bypass-hook-trust
  -c "hooks.PreToolUse=[{matcher='apply_patch',hooks=[{type='command',command='<py.exe> -3 -I <repo>\probe.py <mode>',timeout=660}]}]"
  exec --sandbox workspace-write -C <repo> --skip-git-repo-check -c model_reasoning_effort=low
  "Create the file hello.txt in the current folder containing exactly: hi. Use apply_patch. Do nothing else."
```

Environment: `HANGAR_GATE=4711:probetoken` set for the Codex process.

## Run 1: probe answers nothing, exit 0

Exit 0, 38.7 s. stderr showed twice `warning: --dangerously-bypass-hook-trust is enabled...`,
then `hook: PreToolUse`, `hook: PreToolUse Completed`, `apply patch`, `patch: completed`, and the
same for a second patch. `hello.txt` existed with `hi.\n` (the model edited it a second time).
Hook log, two lines, stdin of the first (masked):

```json
{"session_id":"01a1d106-...","turn_id":"01a1d106-...","transcript_path":"C:\\Users\\<user>\\.codex\\sessions\\2026\\11\\12\\rollout-....jsonl","cwd":"<repo>","hook_event_name":"PreToolUse","model":"gpt-6.1-sol","permission_mode":"bypassPermissions","tool_name":"apply_patch","tool_input":{"command":"*** Begin Patch\n*** Add File: hello.txt\n+hi\n*** End Patch"},"tool_use_id":"exec-..."}
```

Second: `"tool_input":{"command":"*** Begin Patch\n*** Update File: hello.txt\n@@\n-hi\n+hi.\n*** End Patch"}`.
Both lines: `"gate_present": true, "gate_value": "4711:probetoken"`.

## Run 2: probe answers a JSON deny on stdout, the reason on stderr, exit 2

stdout of the probe: `{"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": "Hangar: probe denies this edit."}}`.
Codex: exit 0, 13.8 s, stderr `hook: PreToolUse`, `hook: PreToolUse Failed`, `apply patch`,
`patch: completed`. `hello.txt` existed with `hi\n`. The hook was called (log line with
`gate_present: true`), the deny did not stop the write.

## Run 3: probe answers the same JSON deny, nothing on stderr, exit 0

Codex: exit 0, 9.0 s. stderr:

```text
hook: PreToolUse
... ERROR codex_core::tools::router: error=Command blocked by PreToolUse hook: Hangar: probe denies this edit.. Command: *** Begin Patch
*** Add File: hello.txt
+hi
*** End Patch
hook: PreToolUse Blocked
```

Final message of the model on stdout: "Couldn't create `hello.txt`: the apply_patch edit was
blocked by the PreToolUse hook ("Hangar: probe denies this edit")." `hello.txt` did not exist.

## Read

- The `-c` hook layer with `--dangerously-bypass-hook-trust` fires on `apply_patch` in a live
  run, through `codex.cmd` and `cmd.exe /C`.
- `HANGAR_GATE` reaches the hook's environment unchanged.
- The patch text uses LF line ends and paths relative to `cwd` (`*** Add File: hello.txt`).
- A JSON deny with exit 0 blocks the edit and the reason reaches the model. The same JSON with
  exit 2 and the reason on stderr counts as a failed hook and the edit goes ahead.
