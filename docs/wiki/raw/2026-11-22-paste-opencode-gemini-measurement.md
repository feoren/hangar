# Raw: Bracketed paste with a header line and Enter as a separate write, OpenCode 2.3.2 and Gemini CLI 0.70.0

Measured 2026-11-22 on Windows 11 Pro 10.0.26300 for ADR 0012 (messages between sessions of partner
projects) and ADR 0014 (linked sessions), as the follow-up of
`raw/2026-11-17-cross-project-paste-measurement.md`, which covered Claude Code, Codex CLI, Pi and
Aider. Paths are masked as `<repo>` (the Hangar checkout) and `<user>` (the Windows profile). No
credential was entered, no account was created, no token is recorded.

## Installed programs

```text
$ where opencode gemini
C:\Users\<user>\AppData\Roaming\npm\opencode
C:\Users\<user>\AppData\Roaming\npm\opencode.cmd
C:\Users\<user>\AppData\Roaming\npm\gemini
C:\Users\<user>\AppData\Roaming\npm\gemini.cmd

$ opencode --version   -> opencode v2.3.2
$ gemini --version     -> 0.70.0
```

## Setup

PTY driver: a throwaway Python script in a virtual environment under
`<repo>\tests\.tmp\paste-oc-gemini\` (pywinpty 3.1.2 for the ConPTY, pyte for a rendered screen at
each step). It opens a ConPTY of 120 columns by 40 rows, sets `TERM=xterm-256color` and
`COLORTERM=truecolor`, removes inherited variables starting with `CLAUDE`, `CODEX`, `ANTHROPIC`,
`AI_AGENT` or `HANGAR`, answers `ESC[6n` with `ESC[1;1R` and `OSC 10/11 ?` with a colour, and:

1. Spawns `C:\Windows\System32\cmd.exe /C <program> <args>` (the npm shims) in a fresh `git init`
   folder.
2. Waits until `ESC[?2004h` was seen and the output was quiet for 1.5 s (cap 60 s).
3. Writes one paste in the form Hangar composes since 2026-11-20 (`partners.rs`, `compose`):
   `ESC[200~` + `[Message from project "probe", not from the person]` + `\r` +
   `| Reply with exactly the word PONG and nothing else.` + `ESC[201~`.
4. Sleeps 80 ms (`BROADCAST_ENTER_DELAY_MS`, the delay Hangar uses before its Enter) and writes
   `\r` as its own write. Variant "no Enter": skips this and waits 12 s.
5. Waits until the output is quiet for 10 s (at least 6 s, at most 120 s), sends Ctrl+C twice,
   kills the child after 15 s if it still runs.

Evidence per CLI: the CLI's own session store plus the rendered screen.

### OpenCode

```text
argv: cmd.exe /C opencode --standalone
env:  XDG_CONFIG_HOME, XDG_DATA_HOME, XDG_STATE_HOME = <repo>\tests\.tmp\paste-oc-gemini\oc-xdg\...
      OPENCODE_CONFIG_CONTENT={"model": "opencode/deepseek-v4.1-flash-free"}
```

`--standalone` as for every OpenCode 2.x start (`raw/2026-11-22-opencode2-gate-live-measurement.md`):
without it the TUI talks to the shared background service, which never sees the redirected
folders. The model is OpenCode's free model, which needs no sign-in.

### Gemini CLI

```text
argv: cmd.exe /C gemini
env:  GEMINI_CLI_HOME=<repo>\tests\.tmp\paste-oc-gemini\gemini-home
      GEMINI_API_KEY=hangar-probe-placeholder        (a marker, no key)
      GOOGLE_GEMINI_BASE_URL=http://127.0.0.1:<port> (a local listener)
<home>\.gemini\settings.json: {"security":{"auth":{"selectedType":"gemini-api-key"}}}
```

Without an auth type, Gemini CLI starts the Google sign-in flow
(`raw/2026-11-22-sub-agent-formats-measurement.md`). With the auth type set and a non-empty key
variable it shows its prompt. `GOOGLE_GEMINI_BASE_URL` is read by the 0.70.0 bundle
(`chunk-37D3S3FZ.js`: `process.env["GOOGLE_GEMINI_BASE_URL"]` for `gemini-api-key`, checked only to
be a URL), so every model request went to a Python `http.server` on 127.0.0.1 that logged path and
body and answered `PONG` (`streamGenerateContent` as one SSE event, other calls as JSON).

## Measurement 1: Gemini CLI in a folder it has not seen (observe, then paste + Enter)

Observe run, nothing written: `ESC[?2004h` at 3.4 s, ready at 5.1 s. The screen at that moment:

```text
 Authenticated with gemini-api-key /auth
 ...
 Do you trust the files in this folder?
 ...
 ● 1. Trust folder (gemini-repo)
   2. Trust parent folder (paste-oc-gemini)
   3. Don't trust
```

Paste + Enter run in the same untrusted folder: `ESC[?2004h` at 3.7 s, paste at 5.35 s, Enter at
5.44 s. The Enter chose the preselected "1. Trust folder"; the screen ended with
`Gemini CLI is restarting to apply the trust changes...`, the pasted text was gone and no request
reached the listener. Gemini CLI wrote the decision into the redirected home:

```json
{ "e:/.../tests/.tmp/paste-oc-gemini/gemini-repo": "TRUST_FOLDER" }
```

## Measurement 2: paste, then Enter (folder trusted)

| CLI | `?2004h` first seen | Ready (2004h + 1.5 s quiet) | Prompts that arrived | Header and text in one prompt | Answer |
| --- | --- | --- | --- | --- | --- |
| opencode 2.3.2 | 1.3 s | 4.3 s | 1 | yes | `PONG` (`deepseek-v4.1-flash-free`, 3.2 s) |
| gemini 0.70.0 | 4.1 s | 5.8 s | 1 | yes | `PONG` (from the local listener) |

OpenCode showed no trust dialog in the fresh folder; its prompt was ready at once.

### opencode

Screen before Enter (80 ms after the paste), the input box holds both lines:

```text
┃  [Message from project "probe", not from the person]
┃  | Reply with exactly the word PONG and nothing else.
┃
┃  Build · DeepSeek V4.1 Flash Free OpenCode
```

Screen at the end: a session titled `Project probe PONG check`, the two lines as the user turn,
`+ 1 thought`, `PONG`, `Build · DeepSeek V4.1 Flash Free · 3.2s · 92.9 tok/s`.

Session store (`<xdg-data>\opencode\opencode.db`, read only), table `session_message`:

```text
user       1795368526747  {"text":"[Message from project \"probe\", not from the person]\n| Reply with exactly the word PONG and nothing else.","files":[],"agents":[]}
assistant  1795368526782  model deepseek-v4.1-flash-free, completed 1795368529963
idle       1795368529968  {"outcome":"succeeded"}
```

(`1795368526747` ms is 2026-11-22T17:28:46.747Z.) One session in `session_v2`, one user message.

### gemini

Screen before Enter:

```text
 > [Message from project "probe", not from the person]
   | Reply with exactly the word PONG and nothing else.
```

Screen at the end: the two lines as the user turn, then `✦ PONG`, and an empty prompt.

Chat record (`<home>\.gemini\tmp\gemini-repo\chats\session-2026-11-22T17-27-6f99682a.jsonl`):

```text
{"type":"user","content":[{"text":"<session_context>\n..."}]}          (Gemini's own context, 17:27:16.890Z)
{"type":"user","content":[{"text":"[Message from project \"probe\", not from the person]\n| Reply with exactly the word PONG and nothing else."}]}   17:27:33.732Z
{"type":"gemini","content":"PONG","model":"gemini-3.1-pro-preview-customtools"}
```

`logs.json` holds one user entry with the same text at 17:27:18.534Z (the moment of the Enter).

Listener:

```text
18:27:18 POST /v1beta/models/gemini-3.5-flash-lite:generateContent        system: "You are a specialized Task Routing AI ..."
18:27:23 POST /v1beta/models/gemini-3.5-flash-lite:generateContent        (same, again)
18:27:33 POST /v1beta/models/gemini-3.1-pro-preview-customtools:streamGenerateContent?alt=sse
```

All three bodies carry the pasted text as one user part:
`'[Message from project "probe", not from the person]\n| Reply with exactly the word PONG and nothing else.'`.
The first two are Gemini CLI's model router (system prompt above, a response schema); it asked
twice because the listener's `PONG` is not the JSON it wants, which explains the 15 s until the
main request. Times are local (UTC+1).

## Counter-check: does the line break inside the paste submit?

Same steps, no Enter, wait 12 s.

| CLI | Session store after 12 s | Screen | Result |
| --- | --- | --- | --- |
| opencode | still 1 session, 1 user message (the one of measurement 2) | input box with both lines | not submitted |
| gemini | `logs.json` still 1 user entry, no new chat file | prompt with both lines | not submitted |

The Gemini listener got one request during this run, `18:28:10 POST .../gemini-3.5-flash-lite:generateContent`,
whose body asks to "Summarize the user's primary intent or goal in this conversation in ONE
sentence" over the earlier session's conversation: a summary of the previous session at start,
not the pasted text.

## Read

- **OpenCode 2.3.2 and Gemini CLI 0.70.0 behave like the four measured on 2026-11-17**: a bracketed
  paste with the header line and the `| ` indented line arrives as one prompt, the `\r` inside the
  paste becomes `\n` and does not submit, Enter as its own write 80 ms later does.
- Both turn bracketed paste on within 1.3 s (OpenCode) and 3.4 to 4.4 s (Gemini, behind its banner).
- **Gemini CLI also turns bracketed paste on during its folder trust dialog**, like Claude Code and
  Codex CLI. Pasted text plus Enter answered that dialog with its preselected "Trust folder" and
  restarted the CLI: the message was lost and the folder became trusted without the person. OpenCode
  2.3.2 has no such dialog.
- No difference that needs a per-program delivery recipe: Hangar's delivery (paste, 80 ms, `\r`)
  is the same for every agent and stays so.
- Limits: Gemini's answer came from a local stand-in, not from Google's service; what Gemini CLI does
  with a real key after the submit is not measured, the submit itself is (chat record, `logs.json`
  and the request). OpenCode used its free model only.
