# Sub-agent models and Codex CLI agent files: research and readings

Date: 2026-11-16. Platform: Windows 11 x64. Programs: Claude Code 2.1.325 (native install),
Codex CLI 0.169.0 (npm install, started through the `codex.cmd` shim). No session with a model
request was started; only help texts, documentation and strings in the binaries were read.

## Documentation

- Claude Code, sub-agents: https://code.claude.com/docs/en/sub-agents
  - Frontmatter `model` takes `sonnet`, `opus`, `haiku`, `fable`, a full model id
    (for example `claude-opus-5-5`) or `inherit`.
  - Order: the model parameter of the call, then the definition's `model`, then the variable
    `CLAUDE_CODE_SUBAGENT_MODEL`, then the model of the main conversation.
- Codex CLI, sub-agents: https://learn.chatgpt.com/docs/agent-configuration/subagents
  - One TOML file per agent in `~/.codex/agents/*.toml` (global) and
    `<project>/.codex/agents/*.toml` (project).
  - Required: `name`, `description`, `developer_instructions`. Optional: `model`,
    `model_reasoning_effort`, `model_reasoning_summary`, `model_verbosity`; a missing field is
    inherited.
  - Defaults in `config.toml` under `[agents]`: `default_subagent_model`,
    `default_subagent_reasoning_effort`.
  - No option starts a session as one of the agents.

## Readings

### `codex --help` (0.169.0)

```
-c, --config <key=value>
        Override a configuration value that would otherwise be loaded from `~/.codex/config.toml`.
        Use a dotted path (`foo.bar.baz`) to override nested values. The `value` portion is parsed
        as TOML. If it fails to parse as TOML, the raw string is used as a literal.
```

### Strings in the Codex CLI 0.169.0 binary

File: `node_modules/@openai/codex/node_modules/@openai/codex-win32-x64/vendor/x86_64-pc-windows-msvc/bin/codex.exe`.
`grep -a -o` found:

- `default_subagent_model` and `default_subagent_reasoning_effort` next to the other config keys
  (`job_max_runtime_seconds`, `interrupt_message`, ...), in the agents section of the config
  struct.
- The effort variants in serde order, three times:
  `noneminimallowmediumhighxhighmaxultrapersistent`, elsewhere
  `minimallowmediumhighxhighmaxultrapersistent`. Read as the effort names `none`, `minimal`,
  `low`, `medium`, `high`, `xhigh`, `max`, `ultra`; `persistent` is taken to be the next string.

### Strings in the Claude Code 2.1.325 binary

File: `~/.local/bin/claude.exe`. `CLAUDE_CODE_SUBAGENT_MODEL` occurs 19 times. Excerpts:

```
function eme(){let e=i.CLAUDE_CODE_SUBAGENT_MODEL;return e&&e!=="inherit"?e:"inherit"}
function ne(n,e){if(i.CLAUDE_CODE_SUBAGENT_MODEL_FORCE)n=void 0;if(n==="inherit")return B(e);
  if(n!==void 0){ ... return n} let s=eme(); if(s!=="inherit"){ ...
```

`n` is the definition's model: a set field is returned before the variable is read. Also present:
`CLAUDE_CODE_SUBAGENT_MODEL_FORCE` (ignores the definitions' field) and
`CLAUDE_CODE_SUBAGENT_EFFORT`, both undocumented.

### Local files (read only)

- `~/.codex/config.toml`: top-level `model` and `model_reasoning_effort`.
- `~/.codex/agents/manager.toml`: a user's own agent file with `name`, `description` and a
  multi-line `developer_instructions`.

## Not checked

Gemini CLI, Qwen Code, OpenCode and GitHub Copilot CLI: not installed or not working on this
machine; neither their agent folders nor their file formats were read.
