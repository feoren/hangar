---
title: Global instructions (HANGAR.md) and the first prompt of a worktree session
type: synthesis
tags: [agents, instructions, worktree, catalog, settings]
sources: ["[[agent-instruction-flags-measurement]]", "[[cmd-launcher-measurement]]"]
created: 2026-11-13
updated: 2026-11-13
---

# Global instructions (HANGAR.md) and the first prompt of a worktree session

## What it does

- **HANGAR.md:** one Markdown file in Hangar's config folder, next to `profiles.toml`. With
  Settings > Programs > "Use HANGAR.md for every agent" on, every agent start (new, restored,
  restarted, resume fallback) gets its content as an extra instruction, the way AGENTS.md or
  CLAUDE.md work, without a file in the user's repository. On by default: the template is one
  HTML comment and sends nothing until the user writes into it. "Open HANGAR.md" creates the file
  from that template and opens it in the default editor (`open_hangar_md`, no path argument).
- **Upstream check:** a session that starts in a worktree Hangar has just created (a team session,
  [[0008-private-and-team-sessions]]) gets a first prompt: run `git fetch`, list commits the base
  branch has upstream that the worktree lacks, ask before pulling, or say in one line that it is
  up to date. On by default, switch in the same section. Restores, restarts and resumes never get it.

## Per agent

The catalog says how each program takes them (`instruction_args`, `prompt_args` in
`src-tauri/src/agent_catalog.toml`); a user profile that starts the same program inherits both.
Everything else gets nothing, and Hangar shows nothing about it (only what works is offered).

| Agent | HANGAR.md | First prompt | Evidence |
| --- | --- | --- | --- |
| Claude Code | `--append-system-prompt=<text>` (one argument, so a text starting with `-` is no option) | positional `[prompt]` | [[agent-instruction-flags-measurement]] (2.1.323) |
| Codex CLI | `-c developer_instructions=<TOML string>` | positional `[PROMPT]` | [[agent-instruction-flags-measurement]] (0.169.0) |
| Aider | `--read <path of HANGAR.md>` | none: `--message` exits after one reply | [[agent-instruction-flags-measurement]] (0.86.2) |
| Gemini CLI | none: `GEMINI_SYSTEM_MD` replaces the whole system prompt | none: not installed, not measured | |
| all others | none | none | not researched |

> [!question] Unverified
> No session was run: that Codex sends `-c developer_instructions` to the model, and that Claude
> Code keeps the appended prompt across `--resume`, come from help texts and binary strings only.

## Rules in the code

- `src-tauri/src/instructions.rs`: reading (UTF-8, at most 256 KiB, BOM and HTML comments
  removed, empty means nothing), placeholders `{text}`, `{toml_text}`, `{file}`, `{prompt}` (one
  per field, checked when the catalog is read), length limit, launcher check.
- `src-tauri/src/manager.rs` (`extra_args`): instructions go before the start arguments (so a
  subcommand such as `codex resume --last` still follows the global `-c`), the first prompt last,
  after the gate's options. Update sessions get neither.
- **Secrets:** the text travels on the agent's command line, which other processes of the same user
  can read. The template and the Settings help say so; `{file}` is used where a program takes a
  file (Aider). Claude Code lists no file form, Codex's `model_instructions_file` replaces the
  built-in instructions instead of adding to them.
- **Windows command line:** the whole start (program, gate options, instructions, start arguments,
  first prompt) is measured as the C runtime quotes it (`command_line_units`: quotes for blanks,
  tabs and quotes, a backslash per quote, backslashes before a quote doubled) against 32 766 UTF-16
  units. Over it, the instructions are left out whole with a logged warning (never cut), then the
  first prompt. The same limit applies on macOS and Linux.
- **cmd.exe launchers** (npm's `claude.cmd`, `codex.cmd`): `{text}` and `{toml_text}` are never
  handed to one, and a `{file}` path only without `% ! & | < > ^ "` ([[cmd-launcher]]), each left
  out with a logged warning instead of failing the start. Settings names such installed programs in
  one quiet line ("Not available for: Codex CLI (npm launcher)", `ProfileView.instructions` is
  `launcher`), hidden when HANGAR.md reaches every program that takes it. The first prompt is
  written without such characters (a catalog test checks it) and passes.
- **Why not start the native exe behind the shim:** npm's `codex.cmd` runs `node codex.js`, not an
  executable. `codex.js` (0.169.0) finds `codex.exe` through `require.resolve` of the platform
  package (`@openai/codex-win32-x64`, nested or hoisted), sets `CODEX_MANAGED_PACKAGE_ROOT` and a
  `CODEX_MANAGED_BY_*` variable after its own package manager detection, and only then spawns it.
  Copying that logic would tie Hangar to an internal script that changes between releases, so a
  launcher start goes without the text and Settings says so.
- The prompt text lives in `[start_prompts]` of the catalog; the window only names it
  (`startPrompt: "worktree"`), it never sends text.
