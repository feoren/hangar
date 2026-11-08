---
title: Resuming a conversation (resume_args)
type: agent
tags: [agents, resume, catalog]
sources: ["[[resume-measurement]]", "[[agent-clis-research]]"]
created: 2026-11-07
updated: 2026-11-07
---

# Resuming a conversation (resume_args)

When Hangar restores a session after a restart, it launches the agent with `resume_args`
instead of `args` (a field in `src-tauri/src/agent_catalog.toml` and `profiles.toml`). The list **replaces**
`args`, because some CLIs need a subcommand up front (`codex resume --last`,
`goose session --resume`). Flow and contract: [[session-restore]].

## Values in the catalog (as of 2026-11-07)

| id | Program | resume_args | Evidence |
|---|---|---|---|
| claude | claude | `--continue` | local `claude --help` 2.1.319 ([[resume-measurement]]) |
| codex | codex | `resume --last` | local `codex resume --help` 0.156.1 ([[resume-measurement]]) |
| gemini | gemini | `--resume` | [Gemini CLI docs, session management](https://raw.githubusercontent.com/google-gemini/gemini-cli/main/docs/cli/session-management.md) |
| antigravity | agy | `--continue` | secondary sources only |
| copilot | copilot | `--continue` | [GitHub docs, Copilot CLI](https://docs.github.com/en/copilot/how-tos/copilot-cli/chronicle) |
| cursor | cursor-agent, agent | `--continue` | [Cursor CLI parameters](https://cursor.com/docs/cli/reference/parameters) |
| kimi | kimi | `--continue` | [kimi-command.md](https://raw.githubusercontent.com/MoonshotAI/kimi-cli/main/docs/en/reference/kimi-command.md) |
| kiro | kiro-cli | `chat --resume` | [Kiro session management](https://kiro.dev/docs/cli/chat/session-management/) |
| opencode | opencode | `--continue` | local `opencode --help` 1.15.7 ([[resume-measurement]]) |
| goose | goose | `session --resume` (args: `session`) | [Goose session management](https://goose-docs.ai/docs/guides/sessions/session-management) |
| pi | pi | `--continue` | local `pi --help` 0.79.9 ([[resume-measurement]]) |
| vibe | vibe | `--continue` | [Mistral Vibe README](https://raw.githubusercontent.com/mistralai/mistral-vibe/main/README.md) |
| droid | droid | `--resume` | [Factory CLI reference](https://docs.factory.com/cli/configuration/cli-reference) |
| aider | aider | `--restore-chat-history` | local `aider --help` 0.86.2 ([[resume-measurement]]) |

No evidence and therefore no `resume_args`: qwen, omp, forge, kode, codebuff, amp, crush, cline.

> [!question] Unverified
> antigravity (`agy --continue`) is backed only by secondary sources. The web evidence for the other agents
> that are not installed comes from the research of 2026-11-07 and has not been tried locally.

## Measured behavior without a previous conversation
In a fresh empty folder none of the installed agents exited quickly
([[resume-measurement]]): claude and codex wait at the folder trust prompt, aider at the
question about a Git repository, pi starts a new session, opencode starts its TUI.

> [!question] Unverified
> How `claude --continue` and `codex resume --last` behave in an already trusted folder with no
> earlier conversation. If they exit with an error there, the fallback to a normal launch takes over.

## Limits
- `--continue`/`--last` pick the **most recent** conversation in the working folder. If two sessions
  of the same agent run in the same folder, both resume the same (most recent) one after the restart. A fix
  would be a fixed session ID per Hangar session (`claude --session-id <uuid>` at launch,
  `claude --resume <uuid>` on resume; pi supports `--session-id`). Not implemented.
- A user profile with a catalog `id` only takes over `resume_args` from the catalog if it has none
  of its own and launches the same program name (`src-tauri/src/agent_catalog.rs`, `merge`).
