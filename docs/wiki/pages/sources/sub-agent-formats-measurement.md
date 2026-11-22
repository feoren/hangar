---
title: "Measurement: sub-agent formats of Copilot CLI 1.0.114, OpenCode 2.3.2 and Gemini CLI 0.70.0"
type: source
tags: [agents, sub-agents, copilot, opencode, gemini, windows, measurement]
sources: []
raw: raw/2026-11-22-sub-agent-formats-measurement.md
url: https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference
retrieved: 2026-11-22
created: 2026-11-22
updated: 2026-11-22
---

# Measurement: sub-agent formats of Copilot CLI, OpenCode and Gemini CLI

Each program's agent folder was redirected into a scratch folder of the repository
(`COPILOT_HOME`, `XDG_CONFIG_HOME`, `GEMINI_CLI_HOME`) and a probe file in the shape Hangar
writes was placed there; the program then had to show that it loaded the file. Vendor docs were
read alongside. Windows 11, no account created, no credential entered. Qwen Code is not
installed and was not measured. Record: `raw/2026-11-22-sub-agent-formats-measurement.md`.

## Key points

- **Copilot CLI 1.0.114:** `~/.copilot/agents/<name>.agent.md` (or `.md`), id = file name
  without the ending; frontmatter `description` required, `name` optional. Loaded:
  `copilot --agent does-not-exist -p hi` answers "No such agent: does-not-exist, available:
  hangar-probe", and `copilot --agent hangar-probe -p "Who are you?"` answered as the probe. The
  user folder outranks a project's `.github/agents` and `.claude/agents` (docs).
- **OpenCode 2.3.2:** `~/.config/opencode/agents/<name>.md` (also on Windows), id = file name;
  `description` and `mode: subagent` (without `mode` a new agent is a primary one, docs); body =
  system prompt. Loaded: `opencode debug agents --standalone` lists `hangar-probe` with mode
  `subagent`. Without `--standalone` the command asks the shared background service, which did not
  see the redirected folder.
- **Gemini CLI 0.70.0:** `~/.gemini/agents/*.md`, id = the `name` field; the schema is strict:
  an unknown key such as `mode` fails the file ("Unrecognized key(s) in object: 'mode'"). Files
  starting with `_` are skipped. Headless runs stop at the sign-in before agents load; the TUI
  loads user agents first. Loaded: a second file with the same `name` raised "Duplicate agent
  name 'hangar-probe' detected", which the registry emits only when the first is registered; the
  control run without the first file raised nothing. `/agents list` needs a sign-in.
- **Shared:** a YAML comment line right after the opening `---`, a double-quoted description with
  colons and escaped quotes, and a `.bak` file beside the agent all work in every program.
- **Side effect:** one Gemini TUI run without an auth type started Google's sign-in flow and
  printed its URL; nothing was entered. Later runs set the auth type to an API key without a key.

## Significance for Hangar

- Hangar copies the user's global Claude Code definitions to these three folders
  (`src-tauri/src/md_agents.rs`, [[sub-agent-library]]), from the measured versions on.
- Gemini CLI's strict schema means a copy carries only `name` and `description`; Claude Code's
  `tools`, `model` and `color` would fail the file there.

> [!question] Unverified
> OpenCode 1.x (another folder layout), Copilot CLI's 30 000 character prompt limit when a prompt
> is longer, whether a running OpenCode background service picks up a new agent without
> `opencode reload`, macOS and Linux paths (same home-relative folders expected), and Qwen Code.
