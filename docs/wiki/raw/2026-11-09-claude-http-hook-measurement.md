# Measurement: Claude Code http hooks through --settings (2026-11-09)

Platform: Windows 11 Pro 10.0.26300, Claude Code 2.1.319 (`claude --version`), model flag
`--model haiku`, `--permission-mode acceptEdits`, non-interactive (`claude -p`). Throwaway git
repository under `tests/.tmp/hookrepo` (deleted afterwards) with a project
`.claude/settings.json` holding one command hook (PreToolUse, matcher `Write`, appends a line to
`project-hook.log`). The user settings file has no `hooks` key. Listener: a small Python
`http.server` on 127.0.0.1 that logged each request (Authorization header masked, only whether
it matched the expected marker token) and answered per route. The token value was a harmless
marker, set as `HANGAR_HOOK_TOKEN` in the environment of `claude`.

## Run 1: deny one file, no decision for another, SessionStart http hook

`--session-id 0f8fad5b-d9cb-469f-a165-70867728950e --settings '<JSON>'` with:

```json
{"hooks":{
 "PreToolUse":[{"matcher":"Edit|Write|MultiEdit|NotebookEdit","hooks":[{"type":"http",
   "url":"http://127.0.0.1:47811/pre-tool-use","timeout":60,
   "headers":{"Authorization":"Bearer $HANGAR_HOOK_TOKEN"},"allowedEnvVars":["HANGAR_HOOK_TOKEN"]}]}],
 "SessionStart":[{"hooks":[{"type":"http","url":"http://127.0.0.1:47811/session-start",
   "timeout":10,"headers":{"Authorization":"Bearer $HANGAR_HOOK_TOKEN"},
   "allowedEnvVars":["HANGAR_HOOK_TOKEN"]}]}]}}
```

Prompt: write `a.txt`, then `b.txt`, report the block reason and whether the context mentions
`CONTEXT-MARKER`. Listener: empty 200 body for `a.txt`; for `b.txt`
`{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny",
"permissionDecisionReason":"GATE-MARKER-7: b.txt is being changed by session X"}}`; for
`/session-start` an `additionalContext` with `CONTEXT-MARKER-3`.

Request logged for each Write (body, transcript path shortened here):

```json
{"session_id":"0f8fad5b-d9cb-469f-a165-70867728950e","transcript_path":"C:\Users\...\0f8fad5b-....jsonl",
 "cwd":"E:\...\tests\.tmp\hookrepo","prompt_id":"dfebbb05-...","permission_mode":"acceptEdits",
 "effort":{"level":"medium"},"hook_event_name":"PreToolUse","tool_name":"Write",
 "tool_input":{"file_path":"E:\...\tests\.tmp\hookrepo\a.txt","content":"one"},
 "tool_use_id":"toolu_01..."}
```

Headers: `Content-Type: application/json`, `User-Agent: axios/1.15.2`, `Connection: keep-alive`,
`Content-Length: 687`, Authorization matched `Bearer <marker>` (interpolated from the env var).
No `Transfer-Encoding: chunked`.

Result (model answer, verbatim, German because of the user's own instructions):

```
Erstellt wurde nur `a.txt` im Arbeitsverzeichnis `hookrepo` mit dem Inhalt `one`. `b.txt` wurde
nicht erstellt: Das Write-Tool hat mit dem Fehler `GATE-MARKER-7: b.txt is being changed by
session X` abgebrochen. Ich habe nicht erneut versucht, weil die Meldung besagt, dass eine andere
Session an der Datei arbeitet. Ob die Sperre noch besteht, kann ich von hier aus nicht prüfen.
Mein Kontext enthält kein `CONTEXT-MARKER`; ich habe dieses Wort in keiner der Anweisungen oder
Hinweise gefunden.
```

- Files afterwards: `a.txt`, `base.txt`, `project-hook.log` (two lines: the project command hook
  ran for both writes next to the `--settings` http hook).
- The listener never received a `/session-start` request. The hooks reference says "Only
  `type: "command"` and `type: "mcp_tool"` hooks are supported" for SessionStart.
- `file_path` is absolute, with backslashes on Windows. `session_id` equals `--session-id`.
- Wall time 10.6 s.

## Run 2: UserPromptSubmit context, a hook that times out, an unreachable hook

Same flags without `--session-id`. Settings: `UserPromptSubmit` http hook to the listener
(answers `{"hookSpecificOutput":{"hookEventName":"UserPromptSubmit","additionalContext":
"CONTEXT-MARKER-5: another session is changing notes.md"}}`), and two PreToolUse http hooks on
the same matcher: one to the listener with `"timeout":5` while the listener sleeps 30 s before
answering, one to `http://127.0.0.1:47899` where nothing listens (`"timeout":5`). No
`onFailure` key (default `continue`).

Result (verbatim):

```
Der Write hat funktioniert: `a.txt` mit dem Inhalt `one` wurde im Arbeitsverzeichnis
`tests/.tmp/hookrepo` angelegt, ohne Fehler.
Mein Kontext enthält `CONTEXT-MARKER-5`, und zwar mit dem Text „another session is changing
notes.md“ (aus einem UserPromptSubmit-Hook).
```

- `a.txt` was created: a timed-out and an unreachable http hook both fail open.
- The UserPromptSubmit request carried `cwd, hook_event_name, permission_mode, prompt, prompt_id,
  session_id, transcript_path`; its `additionalContext` reached the model.
- Wall time 20.9 s, of which about 5 s the PreToolUse hook timeout (requests logged at
  t=...163.9 for the prompt and t=...168.7 for the tool call).

## Not measured
- Interactive mode (the TUI while a hook holds the request; Esc during a waiting hook).
- Sub-agent tool calls (the reference says hooks in sub-agents carry `agent_id` and `agent_type`).
- A user settings file that has hooks of its own (only project hooks were present).
