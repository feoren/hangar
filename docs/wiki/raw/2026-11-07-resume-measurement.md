# Measurement: agent CLIs with resume options in an empty folder

- Date: 2026-11-07
- Platform: Windows 11 Pro 10.0.26300, x64, ConPTY via portable-pty 0.9.0, Rust 1.99.1
- Setup: one-off Rust example program against `hangar_lib` (not in the repo). For each agent a fresh,
  empty folder under `%TEMP%`, `SessionManager::create_with(id, ordner, 120x30, resume = true)`
  with the built-in catalog (`src-tauri/src/agent_catalog.toml`, as of commit efaaa2e). The only thing
  sent was the reply `ESC[1;1R` to `ESC[6n` (as xterm.js does), no other input. After the deadline
  `close` (Job Object ends the process tree), then the folder was deleted.
- Deadline: 8 s (claude, codex), 20 s (opencode, pi, aider).
- Follow-up check: no process left with the folder name in its command line, no folders left.
- Not measured: copilot. The only one on PATH is `...\Microsoft VS Code\bin\copilot-shim\copilot.exe`,
  a VS Code shim and not the Copilot CLI; starting it might have offered an installation.
- Not installed: gemini, goose, kimi, cursor-agent, agy, droid, vibe, kiro-cli.

## Results

| Agent | Version | resume_args | Exit within deadline | Fallback | Observation |
|---|---|---|---|---|---|
| claude | 2.1.319 | `--continue` | no (8 s) | no | Shows the working folder prompt ("Accessing workspace: …") and waits for input. |
| codex | 0.156.1 | `resume --last` | no (8 s) | no | Banner, then "Resuming session…", then "Trust this folder?" with the choice "1. Trust and continue / 2. Quit"; waits. |
| opencode | 1.15.7 | `--continue` | no (20 s) | no | Full-screen TUI (alternate screen); queries terminal capabilities (`ESC[>0q`, `ESC[?u`, `OSC 4;0;?`, `ESC[14t`, OSC 99/1337/66). Without replies to these queries no visible content. |
| pi | 0.79.9 | `--continue` | no (20 s) | no | Starts normally (header "pi v0.79.9", input line, update notice), so a new session without errors. |
| aider | 0.86.2 | `--restore-chat-history` | no (20 s) | no | "No git repo found, create one to track aider's changes (recommended)? (Y)es/(N)o [Yes]:"; waits. |

No agent exited quickly with an error in an empty folder. The fallback to a normal start was
therefore not triggered in any case; it is verified only with the test program in `src-tauri/tests/pty.rs`.

## Help texts (local, same day)

```
> claude --version
2.1.319 (Claude Code)
> claude --help   (excerpt)
  -c, --continue                        Continue the most recent conversation in
  -r, --resume [value]                  Resume a conversation by session ID or
  --session-id <uuid>                   Use a specific session ID for the

> codex --version
codex-cli 0.156.1
> codex resume --help   (excerpt)
Resume a previous interactive session (picker by default; use --last to continue the most recent)
Usage: codex resume [OPTIONS] [SESSION_ID] [PROMPT]
      --last
          Continue the most recent session without showing the picker

> opencode --version
1.15.7
> opencode --help   (excerpt)
  -c, --continue     continue the last session                                             [boolean]
  -s, --session      session id to continue                                                 [string]

> pi --version
0.79.9
> pi --help   (excerpt)
  --continue, -c                 Continue previous session
  --resume, -r                   Select a session to resume
  --session-id <id>              Use exact project session ID, creating it if missing

> aider --version
aider 0.86.2
> aider --help   (excerpt)
  --restore-chat-history, --no-restore-chat-history
                          Restore the previous chat history messages (default:
                          False) [env var: AIDER_RESTORE_CHAT_HISTORY]
```

## Research sources for agents that are not installed (carried over, as of 2026-11-07)

- gemini `--resume`: https://raw.githubusercontent.com/google-gemini/gemini-cli/main/docs/cli/session-management.md
- kimi `--continue`: https://raw.githubusercontent.com/MoonshotAI/kimi-cli/main/docs/en/reference/kimi-command.md
- kiro `chat --resume`: https://kiro.dev/docs/cli/chat/session-management/
- goose `session --resume`: https://goose-docs.ai/docs/guides/sessions/session-management
- vibe `--continue`: https://raw.githubusercontent.com/mistralai/mistral-vibe/main/README.md
- droid `--resume`: https://docs.factory.com/cli/configuration/cli-reference
- cursor `--continue`: https://cursor.com/docs/cli/reference/parameters
- copilot `--continue`: https://docs.github.com/en/copilot/how-tos/copilot-cli/chronicle
- antigravity `--continue`: secondary sources only, unverified
- No evidence, therefore no resume_args: qwen, omp, forge, kode, codebuff, amp, crush, cline
