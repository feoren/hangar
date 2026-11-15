# Claude Code `--agent` option (help text)

- Date: 2026-11-15
- Platform: Windows 11 Pro (10.0.26300), Git Bash
- Program: Claude Code 2.1.325 (native install, `claude --version` prints `2.1.325 (Claude Code)`)
- Command: `claude --help | grep -n -i agent`

Output (lines about agents, verbatim):

```
12:  --agent <agent>                       Agent for the current session. Overrides
13:                                        the 'agent' setting.
14:  --agents <json-or-file>               JSON object defining custom agents, or
```

A search of the binary for the name rule of sub-agent definitions found none: the strings
"lowercase letters" appear only for plugin names, marketplace names, hosts and topics, not for
agents.

Not run: a session started with `--agent <name>` (it would start a real conversation).
