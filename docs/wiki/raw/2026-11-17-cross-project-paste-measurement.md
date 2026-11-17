# Raw: Bracketed paste with a header line and Enter as a separate write, plus loopback from the agent sandboxes

Measured 2026-11-17 on Windows 11 Pro 10.0.26300 for ADR 0012 (messages between sessions of partner
projects), at the owner's explicit order, including a few tiny real prompts. Paths are masked as
`<repo>` (the Hangar checkout) and `<user>` (the Windows profile). No tokens or secrets are recorded.

Versions: `claude 2.1.326` (`<user>\.local\bin\claude.exe`), `codex-cli 0.169.0` (npm `codex.cmd`),
`pi 1.5.2` (npm `pi.cmd`), `aider 0.86.2` (`<user>\.local\bin\aider.exe`).
Not measured: OpenCode (its npm install is broken: "postinstall script was not run"; not repaired on
purpose) and Gemini CLI (not installed; not installed on purpose).

## Setup

PTY driver: a throwaway Rust program (own Cargo project under `<repo>\tests\.tmp\adr0012\driver`,
`portable-pty 0.9`, the version in `src-tauri/Cargo.toml`, `cargo build --offline`). It opens a ConPTY
of 120 columns by 40 rows, sets `TERM=xterm-256color` and `COLORTERM=truecolor`, removes every
inherited variable starting with `CLAUDE`, `CODEX`, `ANTHROPIC` or `AI_AGENT` (the measuring agent
itself runs inside a Claude Code session), writes all output raw to a file and does this:

1. Spawn the CLI (argv list) in a fresh `git init` folder `<repo>\tests\.tmp\adr0012\<cli>`.
2. Answer `ESC[6n` (cursor position request, sent by ConPTY at startup) with `ESC[1;1R`. Without
   this, `claude` printed only `ESC[6n` and nothing else for 60 s.
3. Wait until `ESC[?2004h` was seen and the output has been quiet for 1.5 s (cap 60 s).
4. Write one paste: `ESC[200~` + `[Message from project "probe"]` + `\r` + `Reply with exactly the word PONG and nothing else.` + `ESC[201~`.
   The line break inside the paste is `\r` (0x0D), as xterm sends it.
5. Sleep 300 ms, write `\r` as its own write (variant "no Enter": skip this and wait 12 s instead).
6. Wait until the output is quiet for 10 s (at least 6 s, at most 120 s), send the exit keys
   (`/exit\r` or Ctrl+C twice), wait up to 15 s, otherwise kill the child.

Evidence per CLI: the CLI's own transcript plus the screen output.
Argv (the driver gets `<out> <cwd> <enter> <exit keys> <pre keys> <program> <args...>`):

```text
claude : <user>\.local\bin\claude.exe --model haiku               (CLAUDE_CODE_FORCE_SESSION_PERSISTENCE=1 set, see Notes)
codex  : C:\Windows\System32\cmd.exe /C codex -c model_reasoning_effort=low
pi     : C:\Windows\System32\cmd.exe /C pi --model openai-codex/gpt-6-luna --thinking low
aider  : <user>\.local\bin\aider.exe --no-gitignore --no-auto-commits --no-show-release-notes
```

## Measurement 1: paste, then Enter

| CLI | `?2004h` first seen | Ready (2004h + 1.5 s quiet) | Prompts that arrived | Header and text in one prompt | Answer |
| --- | --- | --- | --- | --- | --- |
| claude 2.1.326 | 0.5 to 1.6 s | 2.0 to 5.3 s | 1 | yes | `PONG` (model `claude-haiku-5-5`, 3.8 s after Enter) |
| codex 0.169.0 | 0.8 s (during a trust dialog, see below) | 3.1 s | 1 | yes | `PONG` (model `gpt-6.1-sol`, effort low, 1.6 s after submit) |
| pi 1.5.2 | 0.75 s | 2.9 s | 1 | yes | none: login of both configured providers is expired |
| aider 0.86.2 | 1.3 s | 2.8 s | 1 | yes | none: the configured local model server (127.0.0.1:1234) was not running |

All four turn bracketed paste on (`ESC[?2004h`), so the "wait for 2004h and a pause" rule of the
handover works for the four. All four treated the paste as one prompt: the `\r` inside the paste was
turned into `\n` and did not submit.

### claude

Transcript (`<user>\.claude\projects\...\<session>.jsonl`), the only user line of the session:

```text
user      2026-11-17T00:35:09.393Z  '[Message from project "probe"]\nReply with exactly the word PONG and nothing else.'
assistant 2026-11-17T00:35:13.159Z  claude-haiku-5-5  (thinking)
assistant 2026-11-17T00:35:13.192Z  claude-haiku-5-5  ['PONG']
```

Screen after Enter: the input box showed the two lines, then `Pouncing...` and `PONG`. Exit with
`/exit\r` worked (exit code 0).

### codex

Rollout (`<user>\.codex\sessions\2026\11\17\rollout-...jsonl`), `cwd` is the test folder,
`turn_context` has `model gpt-6.1-sol`, `effort low`. Besides the injected AGENTS.md instructions
(from the owner's global file) there is exactly one user message of ours:

```text
user      00:36:40.991Z  '[Message from project "probe"]\nReply with exactly the word PONG and nothing else.'
assistant 00:36:42.569Z  'PONG'
```

Screen: composer showed `> [Message from project "probe"]` and `  Reply with exactly the word PONG and
nothing else.` on two lines before Enter. `/exit\r` did not quit (the slash popup swallowed it, the
driver killed the child); Ctrl+C twice quit with exit code 0.

### pi

Session file (`<user>\.pi\agent\sessions\--<repo path>--\*.jsonl`), the paste arrived as one user
message, then the call failed on authentication:

```text
{"type":"message","message":{"role":"user","content":[{"type":"text","text":"[Message from project \"probe\"]\nReply with exactly the word PONG and nothing else."}]}}
{"type":"message","message":{"role":"assistant","content":[],"stopReason":"error","errorMessage":"OAuth refresh failed for openai-codex: ... 401 ... invalid_refresh_token"}}
```

The first attempt with the default model (`anthropic`) failed the same way (`invalid_grant`,
"Refresh token expired"); the prompt was submitted there too. Nothing was re-authenticated. Exit
with Ctrl+C twice, exit code 0.

### aider

`.aider.chat.history.md` in the test folder, one prompt (aider writes each line with `####`):

```text
#### [Message from project "probe"]
#### Reply with exactly the word PONG and nothing else.
> litellm.InternalServerError: InternalServerError: OpenAIException - Connection error.
> Retrying in 0.2 seconds...
```

The model server configured in the owner's `.aider.conf.yml` (a local LM Studio) was not running, so
aider retried with growing waits until the driver's 120 s cap and was killed. The `?2004h` appears
once at 1.3 s.

## Counter-check: does the line break inside the paste submit?

Same steps, but no Enter. Wait 12 s, then exit.

| CLI | Transcript after 12 s | Screen | Result |
| --- | --- | --- | --- |
| claude | no session file, no user line | input box with the text, hint `ctrl+g to edit in Notepad` (multi-line input) | not submitted |
| codex | no rollout file created (only the one of the Enter run exists) | composer shows both lines | not submitted |
| pi | session directory empty (file is created at the first message) | editor shows both lines | not submitted |
| aider | no `####` line in `.aider.chat.history.md` | text shown after the `>` prompt | not submitted |

So for all four, the paste alone only fills the input; Enter as a separate write is what submits.

## Notes and pitfalls found on the way

- **`?2004h` also comes with the trust dialog.** In a folder the CLI has not seen yet, `claude`
  (0.5 s) and `codex` (0.8 s) switch bracketed paste on while a "do you trust this folder" dialog is
  on screen. The first two claude runs pasted into that dialog; the following Enter answered it with
  "No", and claude exited with code 1 without answering. Codex's dialog was answered with `\r` ("Yes")
  once; both CLIs store the decision, which added an entry for the throwaway folder to the owner's
  `.claude.json` and `.codex/config.toml` (not reverted; harmless, the folder is deleted). For
  partner projects this is the first message into a folder the person never opened with that CLI.
- **ConPTY needs the cursor position answer.** The terminal host must answer `ESC[6n`; the driver
  did, as the real terminal (xterm.js) does.
- **Claude Code inside Claude Code.** The measuring session is itself a Claude Code session. Without
  removing the inherited `CLAUDE*` variables and setting `CLAUDE_CODE_FORCE_SESSION_PERSISTENCE=1`
  the child showed "Transcript saving is off - inherited CLAUDE_CODE_CHILD_SESSION marker". This
  variable is only for the measurement. A session started by Hangar does not inherit the marker
  unless Hangar itself runs inside Claude Code.
- A first run started with a wrong working directory (a shell quoting mistake of the driver call), so
  claude ran in the profile folder; its transcripts there were removed with the others.
- The thinking and model choice are only for cost: `claude --model haiku`, `codex` effort `low`,
  `pi` thinking `low`.
- Pasting a line break as `\r` turned into `\n` in every transcript that exists (claude, codex, pi).

## Measurement 2: loopback from the agent sandboxes

Listener: Python `http.server` on `127.0.0.1` with an OS-chosen free port (62583 in these runs),
logging every request. A self-test with `curl.exe` showed `GET /selftest from 127.0.0.1`.

### Codex CLI, `workspace-write`

```text
codex exec --sandbox workspace-write -C <repo>\tests\.tmp\adr0012\codex --skip-git-repo-check
  -c model_reasoning_effort=low "Run this exact command and print its output: curl -s http://127.0.0.1:62583/ping"
```

Header of the run: `sandbox: workspace-write [workdir, /tmp, $TMPDIR]`, `approval: never`,
`model: gpt-6.1-sol`. Codex ran it through `pwsh.exe -Command`, where `curl` is the alias of
`Invoke-WebRequest`: exit 1, "mandatory parameter Uri is missing". No request reached the listener.
This is an alias effect, not the sandbox. Repeated with the real binary:

```text
... "Run this exact command and print its output: curl.exe -s http://127.0.0.1:62583/ping"
```

Result: `succeeded in 30ms`, output `PONG-FROM-LISTENER`, listener log
`01:42:37 GET /ping from 127.0.0.1 ua=curl/8.23.0`. The loopback request passed the sandbox without
`sandbox_workspace_write.network_access=true`, so the second variant was not needed.

### Claude Code, `claude -p`

```text
cd <repo>\tests\.tmp\adr0012\claude
claude -p --model haiku "Run this exact command with the Bash tool and print its output: curl.exe -s http://127.0.0.1:62583/ping"
```

(inherited `CLAUDE*` variables unset for the child.) Result with the owner's settings
(`permissions.defaultMode` is `auto`): the command ran, output `PONG-FROM-LISTENER`, listener log
`01:42:50 GET /ping from 127.0.0.1`. With `--permission-mode default` the same prompt was not
executed: the Bash tool answered "This command requires approval", the listener received nothing.
So with the standard permissions the person (or an allow rule such as `Bash(curl.exe:*)`) has to
approve each `hangar-send` call; there is no OS sandbox in this mode that would block loopback.

## Read

- **Bracketed paste plus separate Enter works for claude, codex, pi and aider** (the four measured):
  the text arrives as one prompt including the header line, a line break inside the paste does not
  submit, Enter as its own write does. For claude and codex the model also answered the pasted
  instruction.
- **Readiness**: `?2004h` appears within 0.5 to 1.6 s for all four, but at least claude and codex
  also set it while a trust dialog is open. Waiting for `?2004h` and a pause is necessary but not
  sufficient on the first start in a folder; ADR 0012's "target session is idle" check cannot tell
  a dialog from the prompt. Conclusion: keep the person's "Paste" confirmation, and do not press
  Enter automatically unless the session shows the normal prompt.
- **Loopback from the sandbox**: Codex `workspace-write` on Windows allows a request to
  `127.0.0.1` (listener saw it). `claude -p` can reach it, but only after an approval; with an allow
  rule it is silent. Not measured: the Linux and macOS sandboxes of Codex, Claude Code's optional
  OS sandbox, `danger`-free read-only mode, pi and aider (they run commands without a sandbox of
  their own [UNVERIFIED]).
- The `hangar-send` client should be called by its real executable name (`curl.exe` showed that a
  PowerShell alias can swallow a name); a `.exe` on `PATH` is not affected.
- Not measured: OpenCode (install broken), Gemini CLI (not installed).
