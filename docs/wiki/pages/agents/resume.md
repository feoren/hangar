---
title: Resuming a conversation (resume_args)
type: agent
tags: [agents, resume, catalog]
sources: ["[[resume-measurement]]", "[[resume-id-measurement]]", "[[agent-clis-research]]", "[[subagent-transcripts]]"]
created: 2026-11-07
updated: 2026-11-08
---

# Resuming a conversation (resume_args)

When Hangar restores a session after a restart, it launches the agent with `resume_args`
instead of `args` (a field in `src-tauri/src/agent_catalog.toml` and `profiles.toml`). The list **replaces**
`args`, because some CLIs need a subcommand up front (`codex resume --last`,
`goose session --resume`). Flow and contract: [[session-restore]].

## Conversation ids (session_id_args, resume_id_args)

Since 0.3.0 Hangar gives every session of an agent that accepts one a conversation id of its own
(`src-tauri/src/cli_session.rs`), configured in the catalog, not in code:

| Field | Claude value | Use |
|---|---|---|
| `session_id_args` | `["--session-id", "{id}"]` | APPENDED to `args` on every normal start, with a new UUID v4 |
| `resume_id_args` | `["--resume", "{id}"]` | REPLACES `args` when a session with a stored id is restored |

- The id is part of `SessionInfo` (`cliSessionId`) and of the workspace entry (`cliSessionId`,
  validated as a UUID, optional for older files). Flow and contract: [[session-restore]].
- Restore with a stored id: `claude --resume <id>`. If that exits within 5 s with a nonzero code,
  the fallback starts `claude <args> --session-id <same id>`: measured, `--resume` with an unknown
  id exits with code 1 after about 1 s, before the trust prompt, and creates nothing
  ([[resume-id-measurement]]).
- Restore without a stored id (workspace from before 0.3.0): `resume_args` (`--continue`) as before;
  its fallback starts with a new id. While `--continue` runs, Hangar does not know the id, so the
  sub-agent view is unavailable for that session until it is restarted.
- One conversation id never runs twice: `save_workspace` refuses a workspace in which two
  entries carry the same `cliSessionId`, and a restore whose id a session that has not exited
  already uses starts fresh under a new id instead of resuming.
- A user profile with the catalog's id and program inherits `session_id_args` and
  `resume_id_args` only if its own `args` contain none of `--session-id`, `--resume`,
  `--continue`, `-c`, `-r` (also as `--option=value`); `session_id_args = []` turns the ids off.
- Restart (`restart_session`) starts a fresh conversation with a new id; the old id may already name
  a conversation.
- `{id}` must appear in every non-empty list (checked when the catalog and profiles.toml load).
- Only claude has the fields so far (`claude --help` 2.1.319 lists `--session-id <uuid>` and
  `--resume [id]`, [[subagent-transcripts]]). pi supports `--session-id` too, without evidence for
  resuming by id; not added.

> [!question] Unverified
> That `claude --resume <id>` keeps writing under the same id rather than a new one (the existence
> of `--fork-session`, "create a new session ID", suggests so).

- `--session-id` refuses an id whose transcript exists (`Error: Session ID <id> is already in use.`,
  read from the 2.1.319 executable, [[resume-id-measurement]]). Reusing the id in the fallback is
  therefore safe only when the conversation is unknown; when `--resume` fails on an existing but
  unreadable conversation, the fallback exits with that error and a restart starts fresh. The
  same rule is why a restart never reuses the id with `--session-id`.
- Restart deliberately starts a new conversation: "Restart" has always meant "start the program
  again" with `args`, never `resume_args`, and a restart is mostly used after the program ended on
  purpose (`/exit`) or failed. The previous conversation stays available in claude's own
  `--resume` picker. Continuing it from Hangar would be a separate action.

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
  of the same agent run in the same folder, both resume the same (most recent) one after the restart.
  Solved for claude by conversation ids (above); still true for every other agent and for claude
  sessions saved before 0.3.0.
- A user profile with a catalog `id` only takes over `resume_args` from the catalog if it has none
  of its own and launches the same program name (`src-tauri/src/agent_catalog.rs`, `merge`).
