---
title: "Measurement: Copilot CLI 1.0.114 live run with Hangar's gate plugin"
type: source
tags: [hooks, gate, copilot, plugin, windows, measurement, live]
sources: []
raw: raw/2026-11-22-copilot-gate-live-measurement.md
url:
retrieved: 2026-11-22
created: 2026-11-22
updated: 2026-11-22
---

# Measurement: Copilot CLI 1.0.114 live run with Hangar's gate plugin

Three live turns on Windows 11 with Copilot CLI 1.0.114 and the account's default model, in
throwaway git repositories: two headless (`-p`), one interactive (`-i`) in a ConPTY. The start
used exactly Hangar's recipe (`--plugin-dir <folder>/copilot-plugin`, `HANGAR_GATE` in the
environment only) and the real helper `hangar gate-hook copilot pre-tool-use` of the installed
release; a Python probe stood in for the endpoint. Record:
`raw/2026-11-22-copilot-gate-live-measurement.md`. Answers the Copilot row of
[[agent-gate-adapters]].

## Key points

- **The plugin loads from `--plugin-dir`** and its `preToolUse` hook with `exec` and `args` runs
  without a shell, headless and interactive.
- **No plugin trust prompt.** Interactive, Copilot shows only its usual folder trust dialog for a
  new folder; nothing runs before it is answered, and after "Yes" the hook holds edits.
- **`HANGAR_GATE` arrives** in the hook's environment; the helper's request carried the right
  bearer token.
- **Tool and arguments:** file writes came as `toolName` `apply_patch` with `toolArgs` as the raw
  patch string (LF line ends), paths relative to `cwd` or absolute, one call for several files.
- **A deny stops the write**: Copilot showed "Denied by preToolUse hook: <reason>", the file was
  not written and the model reported the reason. With an empty answer an add and an update in one
  patch went through, and the endpoint got one request per path.
- **Token format matters:** a token with `-` is refused by the helper as malformed (fails open);
  Hangar's tokens are alphanumeric.
- **After a deny** the model stored the text with Copilot's own session state tool (outside the
  repository). Shell writes are not hooked, as for every agent.

## Significance for Hangar

- Copilot CLI is team-capable from 1.0.114 (`measured = true`, `min_version = "1.0.114"`).
- Exit 2 with the reason on stderr and the JSON deny on stdout, as the helper answers Copilot,
  blocks (measured), unlike Codex ([[codex-gate-live-measurement]]).
