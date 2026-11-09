# Measurement: Claude Code --settings with a file path, and "ask" in print mode (2026-11-09)

Platform: Windows 11 Pro 10.0.26300, Claude Code 2.1.319 (`claude --version`, installed as
`claude.exe`), `--model haiku`, `--permission-mode acceptEdits`, non-interactive (`claude -p`).
Throwaway git repository under `tests/.tmp/settingsrepo` (deleted afterwards). Listener: a small
Python `http.server` on 127.0.0.1:47833 that logged route, tool, file, `Host` header and whether
the `Authorization` header matched the harmless marker token set as `HANGAR_HOOK_TOKEN` in the
environment of `claude`.

## Command

`claude -p --model haiku --permission-mode acceptEdits --settings "<tests/.tmp>\hook settings\gate hooks.json" "<prompt>"`

The settings file path contains spaces. File content (UTF-8 without BOM):

```json
{"hooks":{
 "UserPromptSubmit":[{"hooks":[{"type":"http","url":"http://127.0.0.1:47833/v1/user-prompt-submit",
   "allowedEnvVars":["HANGAR_HOOK_TOKEN"],"headers":{"Authorization":"Bearer $HANGAR_HOOK_TOKEN"},"timeout":10}]}],
 "PreToolUse":[{"matcher":"Edit|Write|MultiEdit|NotebookEdit","hooks":[{"type":"http",
   "url":"http://127.0.0.1:47833/v1/pre-tool-use","allowedEnvVars":["HANGAR_HOOK_TOKEN"],
   "headers":{"Authorization":"Bearer $HANGAR_HOOK_TOKEN"},"timeout":60}]}]}}
```

Prompt: write `a.txt`, `b.txt`, `c.txt`; report which were created, the exact error texts, and
whether the context holds a string starting with `CONTEXT-MARKER`. Listener answers: empty 200
for `a.txt`; `permissionDecision: "deny"` with reason `GATE-MARKER-9: b.txt is being changed
elsewhere` for `b.txt`; `permissionDecision: "ask"` with reason `ASK-MARKER-4` for `c.txt`;
`additionalContext` `CONTEXT-MARKER-8: facts only` for the prompt.

## Listener log (verbatim, paths as logged)

```
{"route": "/v1/user-prompt-submit", "auth_ok": true, "tool": null, "file": "", "host": "127.0.0.1:47833"}
{"route": "/v1/pre-tool-use", "auth_ok": true, "tool": "Write", "file": "E:\\...\\tests\\.tmp\\settingsrepo\\a.txt", "host": "127.0.0.1:47833"}
{"route": "/v1/pre-tool-use", "auth_ok": true, "tool": "Write", "file": "E:\\...\\tests\\.tmp\\settingsrepo\\b.txt", "host": "127.0.0.1:47833"}
{"route": "/v1/pre-tool-use", "auth_ok": true, "tool": "Write", "file": "E:\\...\\tests\\.tmp\\settingsrepo\\c.txt", "host": "127.0.0.1:47833"}
```

## Model answer (verbatim)

```
Results of the three writes:

- **Created:** `a.txt` (content `one`), in `E:\...\tests\.tmp\settingsrepo\`.
- **Failed:** `b.txt` was blocked by a PreToolUse hook with the error text `PreToolUse:Write hook error: GATE-MARKER-9: b.txt is being changed elsewhere`.
- **Failed:** `c.txt` was blocked by a PreToolUse hook with the error text `ASK-MARKER-4`.

I did not retry or work around either hook. The hook messages don't say what the gate is waiting for, so `b.txt` and `c.txt` still need writing once that is resolved.

Yes, my context contains a string starting with CONTEXT-MARKER: `CONTEXT-MARKER-8: facts only`. It appeared as additional context from a UserPromptSubmit hook.
```

Files afterwards: `a.txt`, `base.txt`. Wall time 9.3 s.

## Findings

- `--settings <path to a JSON file>` works like inline JSON, also with spaces in the path: both
  hooks ran, the token was interpolated from the environment, the context reached the model.
- The `Host` header is `127.0.0.1:<port>`, exactly as in the hook URL.
- `permissionDecision: "ask"` in print mode (no person to ask) did not write the file; the reason
  reached the model. In interactive mode the reference says "ask" prompts the user to confirm
  (not measured here).
