# Measurement: `agent_id` and `agent_type` in Claude Code's `PreToolUse` input

- Date: 2026-11-22
- Platform: Windows 11 Pro 10.0.26300, x64
- Claude Code: 2.1.330 (`claude --version`)
- Codex CLI: 0.171.0 installed, not run (see the end)
- Model: `--model haiku`, print mode, one short prompt per run
- Paths below are masked: `<work>` is the test folder `tests/.tmp/agent-id-probe/work` inside a
  Hangar checkout, `<probe>` its parent, `<projects>` Claude Code's projects folder. The run's
  session, prompt and tool use ids are masked as `<session>`, `<prompt-id>` and `<tool-use-n>`;
  the sub-agent ids are kept, they name nothing outside the run.

## Setup

Probe hook (`<probe>/probe.cjs`, CommonJS because the checkout's `package.json` says
`"type": "module"`; the first run with a `.js` file failed with
`ReferenceError: require is not defined in ES module scope` on every hook call):

```js
const fs = require("fs");
const path = require("path");
let data = "";
process.stdin.on("data", (c) => (data += c));
process.stdin.on("end", () => {
  fs.appendFileSync(path.join(__dirname, "hook-log.jsonl"), data.trim() + "\n");
});
```

Settings for run 1 (`<probe>/settings.json`):

```json
{ "hooks": { "PreToolUse": [ { "matcher": "*", "hooks": [
  { "type": "command", "command": "node \"<probe>/probe.cjs\"" } ] } ] } }
```

Settings for run 2 (`<probe>/settings-http.json`), with a Node listener on 127.0.0.1:47613 that
appended each POST body to `http-log.jsonl` and answered an empty 200:

```json
{ "hooks": { "PreToolUse": [ { "matcher": "*", "hooks": [
  { "type": "http", "url": "http://127.0.0.1:47613/pre-tool-use", "timeout": 10 } ] } ] } }
```

## Run 1: command hook

```
cd <work>
claude -p "Use the Agent tool to start one general-purpose sub-agent with this task: write the file probe.txt in the current folder with the single word marker. Wait for it. Do nothing else yourself." --model haiku --settings ../settings.json --permission-mode acceptEdits --max-turns 6 --output-format text
```

`hook-log.jsonl`, two lines (the `Write` content and the `Agent` prompt replaced by placeholders):

```json
{"session_id":"<session>","transcript_path":"<projects>\\<encoded work>\\<session>.jsonl","cwd":"<work>","prompt_id":"<prompt-id>","permission_mode":"acceptEdits","effort":{"level":"medium"},"hook_event_name":"PreToolUse","tool_name":"Agent","tool_input":{"description":"probe.txt mit Marker schreiben","prompt":"<prompt>","subagent_type":"general-purpose","run_in_background":false},"tool_use_id":"<tool-use-1>"}
{"session_id":"<session>","transcript_path":"<projects>\\<encoded work>\\<session>.jsonl","cwd":"<work>","prompt_id":"<prompt-id>","permission_mode":"acceptEdits","agent_id":"aaafc28bb29131913","agent_type":"general-purpose","effort":{"level":"medium"},"hook_event_name":"PreToolUse","tool_name":"Write","tool_input":{"file_path":"<work>\\probe.txt","content":"<content>"},"tool_use_id":"<tool-use-2>"}
```

Sub-agent files of that session:

```
<projects>/<encoded work>/<session>/subagents/
agent-aaafc28bb29131913.jsonl
agent-aaafc28bb29131913.meta.json
```

## Run 2: http hook

```
claude -p "Use the Agent tool to start one Explore sub-agent with this task: write the file probe2.txt in the current folder with the single word marker using the Write tool. If Explore cannot write, use a general-purpose sub-agent instead. Wait for it. Do nothing else yourself." --model haiku --settings ../settings-http.json --permission-mode acceptEdits --max-turns 8 --output-format text
```

Keys of each POST body, in order:

```
{"keys":["session_id","transcript_path","cwd","prompt_id","permission_mode","effort","hook_event_name","tool_name","tool_input","tool_use_id"],"tool":"Agent"}
{"keys":["session_id","transcript_path","cwd","prompt_id","permission_mode","effort","hook_event_name","tool_name","tool_input","tool_use_id"],"tool":"Agent"}
{"keys":["session_id","transcript_path","cwd","prompt_id","permission_mode","agent_id","agent_type","effort","hook_event_name","tool_name","tool_input","tool_use_id"],"tool":"Write","agent_id":"a40f5d22eb3cb20e8","agent_type":"general-purpose","file":"<work>\\probe2.txt"}
```

The Explore sub-agent made no tool call (it has no `Write`); the main agent then started a
general-purpose one, whose `Write` carried the fields.

## Docs read the same day

- https://code.claude.com/docs/en/hooks, common input fields: "`agent_id`: Unique identifier for
  the subagent or in-process teammate the hook fires in." "`agent_type`: Agent name (for example,
  `"Explore"` or `"security-reviewer"`). Present when the session uses `--agent` or the hook fires
  inside a subagent. For subagents, the subagent's type takes precedence over the session's
  `--agent` value." And: "When a subagent calls a tool, tool events such as `PreToolUse` and
  `PostToolUse` fire the same configured hooks as in the main conversation, and the input carries
  the `agent_id` and `agent_type` common input fields that identify the subagent."
- https://learn.chatgpt.com/docs/hooks (Codex, redirected from developers.openai.com/codex/hooks;
  no version stated): `PreToolUse` input is `session_id`, `transcript_path`, `cwd`,
  `hook_event_name`, `model`, `turn_id`, `tool_name`, `tool_use_id`, `tool_input` (plus
  `permission_mode`); "Subagent hooks use the parent session id." `agent_id` and `agent_type`
  appear only for `SubagentStart`, `SubagentStop` and the model request events, not for
  `PreToolUse`. Codex was not run.
