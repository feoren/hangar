# Raw: Pi 1.5.2 live run with Hangar's gate extension

Date: 2026-11-12. Platform: Windows 11 x64, Node 26.13.1. Approved by the owner (at most three
short runs on their own setup). Paths shortened: `<repo>` is the Hangar checkout, `<probe>` a
scratch folder outside it.

## Setup

- Pi 1.5.2 (`pi --version`), npm `@earendil-works/pi-coding-agent`, started through npm's
  `%APPDATA%\npm\pi.cmd`. The user's settings load two packages (`pi-web-access`,
  `pi-subagents`); they stayed on.
- Throwaway git repository `<repo>\tests\.tmp\pi-measure` (one empty commit), deleted afterwards.
- Extension file `<repo>\tests\.tmp\pi-measure-gate\hangar-gate.ts` =
  `src-tauri/src/gate_plugins/core.js` + `pi.js` exactly as `hook_recipes::pi_extension()` joins
  them. A second, measurement-only extension logged each tool call's name and input keys.
- Endpoint: a Python probe on 127.0.0.1 (random port) in place of Hangar's endpoint, checking
  `Authorization: Bearer <token>`, logging each request (token masked) and answering Claude Code's
  deny (`hookSpecificOutput.permissionDecision: "deny"`, reason "Hangar probe: another session is
  changing this file."). `HANGAR_GATE=<port>:<token>` (64 hex characters) only in the child's
  environment.
- Prompt: `create hello.txt containing hi`, print mode (`-p`).

## Run 1: `--model gpt-5.6-luna --thinking low` (provider openai-codex, the user's default)

```
OAuth refresh failed for openai-codex: OpenAI Codex token refresh failed (401): {
  "error": { "message": "Could not validate your refresh token. Please try signing in again.",
  "type": "invalid_request_error", "param": null, "code": "invalid_refresh_token" } }
```

Exit 1. No tool call, no probe request.

## Run 2: `--provider anthropic --model claude-haiku-5-5 --thinking off`

```
OAuth refresh failed for anthropic: Anthropic token refresh request failed. ...
body={"error": "invalid_grant", "error_description": "Refresh token expired"}
```

Exit 1. No tool call, no probe request. Both stored sign-ins (`auth.json`: `anthropic`,
`openai-codex`, both OAuth) had expired; no local model server was running.

## Run 3: live Pi process, scripted model

Model: a measurement-only extension registered a provider `hangarprobe` (`api:
"openai-completions"`, `baseUrl` = the probe's `/v1`) whose `/chat/completions` answered from a
script: without a tool result in the request, one streamed tool call
`write {"path": "hello.txt", "content": "hi"}` (finish reason `tool_calls`); with one, the text
`done`. Everything else was Pi's own agent loop, tool set and `write` tool.

Command:
`pi.cmd -e <gate file> -e <log file> -e <provider file> --provider hangarprobe --model scripted -p "create hello.txt containing hi"`

Output:

```
exit: 0
--- stdout
done
--- stderr
Warning: Extension package ".../pi-subagents/package.json": Host-provided extension packages must
be declared in peerDependencies ... (unrelated)
--- scripted model calls:
[{"tools": ["read", "bash", "edit", "write", "web_search", "code_search", "fetch_content",
  "get_search_content", "subagent"], "has_tool_result": false},
 {"tools": [same], "has_tool_result": true}]
--- probe requests
{"path": "/v1/pre-tool-use", "host": "127.0.0.1:63400", "token_ok": true, "browser_headers": false,
 "header_names": ["Authorization", "Connection", "Content-Length", "Content-Type", "Host"],
 "body": {"hook_event_name": "PreToolUse", "tool_name": "Write",
          "tool_input": {"file_path": "<repo>\\tests\\.tmp\\pi-measure\\hello.txt"},
          "cwd": "<repo>\\tests\\.tmp\\pi-measure"}}
```

After the run the repository held only `.git`: `hello.txt` was not written. The measurement log
stayed empty: the gate extension (loaded first) blocked the call, and a blocked call does not reach
later `tool_call` handlers.

## Offline check before the runs

The same extension under Node with a fake `pi` object against the probe: `write` and `edit` were
asked about (relative path resolved against `cwd`), `read` was not; deny gave `{"block": true,
"reason": ...}`; an empty answer, a wrong token (401) and no listener all gave `undefined` (the
tool goes ahead). The request carried no `Origin` or `Sec-Fetch-*` header (`node:http`).

For comparison, Node 26.13.1's global `fetch` (POST with a JSON body) against the same probe sent
the headers `accept, accept-encoding, accept-language, connection, content-length, content-type,
host, sec-fetch-mode, user-agent`; Hangar's endpoint refuses any `Sec-Fetch-*` header as a
browser request.
