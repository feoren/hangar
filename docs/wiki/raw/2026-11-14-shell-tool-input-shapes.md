# Shell tool input shapes of Claude Code, Pi and OpenCode (reading, 2026-11-14)

Read on Windows 11, nothing run. Purpose: which field carries the command line when each agent
calls its shell tool, for Hangar's command guard (ADR 0010).

## Claude Code (transcripts)

Source: the transcripts Claude Code writes on this machine (`.jsonl` files under the user's
Claude projects folder), searched for `tool_use` blocks. Only tool names and field names were
counted; no command text was copied.

```
grep -rhoa '"type":"tool_use","id":"…","name":"Bash","input":{"[a-z_]*"'      -> 136510 x "input":{"command"
grep -rhoa '"type":"tool_use","id":"…","name":"PowerShell","input":{"[a-z_]*"' ->   1031 x "input":{"command"
```

Versions in the files that hold `PowerShell` calls: 2.1.319, 2.1.322, 2.1.323 (most frequent).
The transcript records the model's tool input; the hook's `tool_input` is the same object
according to Claude Code's hooks reference, not measured here.

## Pi 1.5.2 (`@earendil-works/pi-coding-agent`, npm global install)

`dist/core/tools/bash.js`:

```
29:    command: Type.String({ description: "Shell command to execute" }),
30:    timeout: Type.Optional(Type.Number({ description: "Timeout in seconds (optional, no default timeout)" })),
342: const bashToolConfig = {
343:     name: "bash",
345:     shellName: "bash",
```

`dist/core/tools/powershell.js` builds a second tool from the same shell tool definition:

```
29:    name: "powershell",
31:    shellName: "PowerShell",
```

Both take `{ command, timeout }` (`createShellToolDefinition`, `execute(_id, { command, timeout })`).

## OpenCode 1.19.5 (`opencode-ai`, `opencode-windows-x64/bin/opencode.exe`)

Strings in the bundled binary (`grep -a`):

```
var b$="bash", ... i1=p({command:o.annotate({description:"Shell command string to execute"}),
  workdir:o.pipe(c).annotate({description:"Working directory. Defaults to the active Location;
  relative paths resolve from that Location."}), timeout:...
```

and a second definition next to the shell sets `On=new Set(["powershell","pwsh"])`,
`$n=new Set(["cmd"])`:

```
command:o.annotate({description:"The command to execute"}), timeout:..., workdir:c(o).annotate({
  description:"The working directory to run the command in. Defaults to the current directory.
  Use this instead of 'cd' commands."})
```

The tool prompt adds: "AVOID using `cd <directory> && <command>`. Use the `workdir` parameter to
change directories instead." Which shell OpenCode picks on Windows (PowerShell, pwsh or cmd) is
decided at run time and was not read.
