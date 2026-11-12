---
title: "Measurement: Pi 1.5.2 with Hangar's gate extension (live process, scripted model)"
type: source
tags: [hooks, gate, pi, extension, windows, measurement, live]
sources: []
raw: raw/2026-11-12-pi-gate-live-measurement.md
url:
retrieved: 2026-11-12
created: 2026-11-12
updated: 2026-11-12
---

# Measurement: Pi 1.5.2 with Hangar's gate extension (live process, scripted model)

Three approved runs of Pi 1.5.2 on Windows 11 through npm's `pi.cmd`, in a throwaway git
repository that was deleted afterwards, with exactly Hangar's recipe: `-e <file>` with the
extension `hook_recipes::pi_extension()` writes, `HANGAR_GATE=<port>:<token>` in the child's
environment only, the user's own Pi packages left on. A Python probe stood in for Hangar's
endpoint. Record: `raw/2026-11-12-pi-gate-live-measurement.md`. Answers section 6, question 1 of
[[agent-hooks-remaining-agents-research]] for [[agent-gate-adapters]].

## Key points

- **Runs 1 and 2 never reached a model**: both stored sign-ins (`openai-codex`, `anthropic`) had
  expired refresh tokens.
- **Run 3: live Pi, scripted model.** A measurement-only extension registered an
  OpenAI-compatible provider pointing at the probe, which answered with one `write` tool call
  (`path: "hello.txt"`, `content: "hi"`) and then `done`. Pi's own agent loop, tool set and
  `write` tool ran unchanged.
- **The hook fires before the write**: the probe saw one request (`tool_name` `Write`, the path
  resolved against `cwd`), and afterwards the repository held no `hello.txt`.
- **The token arrives**: the request carried the right bearer token, `Host: 127.0.0.1:<port>` and
  no `Origin` or `Sec-Fetch-*` header (`node:http` under Node 26).
- **A deny stops the write**: `{ block: true, reason }` kept the file from being written, and the
  model's next request carried the tool result. A blocked call does not reach later `tool_call`
  handlers (the logging extension saw nothing).
- **No trust prompt** for a `-e` extension, and it loaded next to the user's two packages.

> [!warning] Not a real model
> The tool call came from a script, not from a hosted model. The parts that matter for the gate
> (extension loading, `tool_call`, the `write` tool's `path` field, blocking) are Pi's own code;
> the measurement does not show how a real model reacts to the reason.

## Significance for Hangar

- Pi is team-capable from 1.5.2 (`measured = true`, `min_version = "1.5.2"`).
- Not measured live: `edit` (`path` plus `edits`, from Pi's tool schema in
  `dist/core/tools/edit.js`; the extension's handling is tested under Node in
  `src-tauri/tests/gate_hook.rs`) and the TUI's confirmation for Hangar's "ask".
