# Measurement: how agent CLIs take extra instructions and a first prompt

Date: 2026-11-13. Platform: Windows 11 Pro (build 26300), Git Bash. Only `--help` output and the
strings of one executable were read; no agent session was started and no model was called.

## Installed versions

```
$ claude --version
2.1.323 (Claude Code)
$ codex --version
codex-cli 0.169.0
$ aider --version
aider 0.86.2
$ gemini --version
bash: gemini: command not found
```

## Claude Code 2.1.323 (`claude --help`, excerpts verbatim)

```
Usage: claude [options] [command] [prompt]

Claude Code - starts an interactive session by default, use -p/--print for
...
  --append-system-prompt <prompt>       Append a system prompt to the default
                                        system prompt
...
                                        via: --system-prompt[-file],
                                        --append-system-prompt[-file], --add-dir
```

`--append-system-prompt-file` is only mentioned in the description of `--bare`, not listed as an
option of its own.

## Codex CLI 0.169.0 (`codex --help`, excerpts verbatim)

```
Usage: codex [OPTIONS] [PROMPT]
       codex [OPTIONS] <COMMAND> [ARGS]
...
Arguments:
  [PROMPT]
          Optional user prompt to start the session

Options:
  -c, --config <key=value>
          Override a configuration value that would otherwise be loaded from `~/.codex/config.toml`.
          Use a dotted path (`foo.bar.baz`) to override nested values. The `value` portion is parsed
          as TOML. If it fails to parse as TOML, the raw string is used as a literal.
```

Strings of `%LOCALAPPDATA%\Programs\OpenAI\Codex\bin\codex.exe` (0.169.0), searched with
`grep -a -o "developer_instructions[a-z_]*"`: `developer_instructions` occurs 15 times on its own
and inside the concatenated field list of the config struct, directly next to
`model_instructions_file`, `compact_prompt`, `forced_login_method`, `mcp_servers` and the other
top-level keys of `config.toml`. `model_instructions_file` also exists (15 times).

`which -a codex` lists the npm shim (`%APPDATA%\npm\codex`, a `codex.cmd` on Windows) before the
native `codex.exe` on this machine's PATH.

## Aider 0.86.2 (`aider --help`, excerpts verbatim)

```
  --message COMMAND, --msg COMMAND, -m COMMAND
                        Specify a single message to send the LLM, process
                        reply then exit (disables chat mode) [env var:
                        AIDER_MESSAGE]
...
  --read FILE           specify a read-only file (can be used multiple times)
                        [env var: AIDER_READ]
```

## Gemini CLI

Not installed; nothing measured.
