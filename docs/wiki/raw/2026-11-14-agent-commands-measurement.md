# Measurement: slash commands of Claude Code, Codex CLI and Gemini CLI

Date: 2026-11-14. Platform: Windows 11 Pro (build 26300), Git Bash. Only the installed executables
were searched for strings (`grep -a`); no session was started, no command was typed and no model
was called. Gemini CLI is not installed; its commands come from the vendor's command reference.
Paths are masked: `%USERPROFILE%` for the home folder, `%APPDATA%\npm` for npm's global folder.

## Installed versions

```
$ claude --version
2.1.325 (Claude Code)
$ codex --version
codex-cli 0.169.0
$ which gemini
which: no gemini in (...)
```

Claude Code is the native install (`%USERPROFILE%\.local\bin\claude.exe`, a Bun executable with
the JavaScript bundle inside). Codex is the npm install; its program is
`%APPDATA%\npm\node_modules\@openai\codex\node_modules\@openai\codex-win32-x64\vendor\x86_64-pc-windows-msvc\bin\codex.exe`.

## Claude Code 2.1.325: command table in the bundle (excerpts verbatim)

`grep -aoE 'name:"<name>",.{0,260}' claude.exe`, cut after the description:

```
name:"compact",description:"Free up context by summarizing the conversation so far",isEnabled:()=>!Pe(process.env.DISABLE_COMPACT),supportsNonInteractive:!0,argumentHint:"<optional custom summarization instructions>",thinClientDispatch:"post-text",...
name:"clear",description:"Start a new session with empty context; previous session stays on disk (resumable with /resume)",argumentHint:"[name]",aliases:["reset","new"],supportsNonInteractive:!0,thinClientDispatch:"post-text",...
name:"usage",aliases:["cost","stats"],description:"Show session cost, plan usage, and activity stats",thinClientDispatch:"control-request",immediate:!0,requires:{ink:!0}},...
name:"context",description:"Visualize current context usage as a colored grid",argumentHint:"[all]",isEnabled:()=>!ge()&&!tAn(),type:"local-jsx",thinClientDispatch:"control-request"},...
```

`/model` (two entries, local and local-jsx):

```
var yft={type:"local",name:"model"
... load:()=>import("B:/~BUN/root/chunk-55dehfz1.js")},_ft={type:"local-jsx",name:"model"
```

Notes: `/cost` is an alias of `/usage` in this version, so `/usage` is the command name. `/compact`
is disabled when the environment variable `DISABLE_COMPACT` is set.

## Codex CLI 0.169.0: command names and texts in `codex.exe` (excerpts verbatim)

The name table of the slash commands (one run of concatenated names, cut at the next string):

```
...importnewworktreeappinitcompactrecapplansidebtwexportrawmentiondaemoncdpwdusagedebug-configstatuslinepetslogoutquitexitrolloutpscleartest-approvalsubagentsdebug-m-dropdebug-m-update
```

Descriptions of the command popup (one run of concatenated texts, excerpt):

```
...start a new chat during a conversation...summarize conversation to prevent hitting the context limit...show current session configuration and token usage...
```

The built-in help text:

```
commands:/init - create an AGENTS.md file with instructions for Codex/status - show current session configuration/permissions - choose what Codex is allowed to do/model - choose what model and reasoning effort to use/review - review any changes and find issues.
```

Other user-facing texts that name the commands:

```
Start a fresh idea with **/new**; the previous session stays in history.
Use /new to start a chat with the selected provider.
) limit left. Run /status for a breakdown.
Check the spelling or use /model to choose another model.
```

## Gemini CLI: vendor command reference

Source: https://raw.githubusercontent.com/google-gemini/gemini-cli/main/docs/reference/commands.md,
retrieved 2026-11-14 (main branch; no installed version to compare). Quoted descriptions:

- `/compress`: "Replace the entire chat context with a summary."
- `/clear`: "Clear the agent conversation context (active conversation history) and start a new
  session", in addition to clearing the visible terminal screen, history and scrollback. Ctrl+L
  only clears the display and keeps the context.
- `/stats`: "Display detailed statistics for the current Gemini CLI session."
- `/model`: "Manage model configuration."

No alias is listed for these four.

## Addendum (same day): behavior while the agent works

Strings searched in the same executables (`grep -aoE`), no session run.

Codex CLI 0.169.0 (`codex.exe`), excerpt verbatim, non-text bytes shown as `…` (the command name is filled in at run time):

```
'/…' is disabled while a task is in progress. Unrecognized command '/…'. Type "/" for a list of supp
Ctrl+L is disabled while a task is in progress.
```

Claude Code 2.1.325 (`claude.exe`), excerpts verbatim:

```
Hit Enter to queue up additional messages while Claude is working.
describe("@internal Fate of a queued command (slash command or queued user prompt). 'queued' when the inbound message enters the command queue; 
```

Reading: Codex refuses some slash commands during a turn (which ones is decided in code, not in
the strings); Claude Code queues slash commands and prompts typed while it works.
