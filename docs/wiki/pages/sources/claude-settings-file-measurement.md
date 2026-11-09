---
title: "Measurement: Claude Code --settings with a file path, and \"ask\" in print mode"
type: source
tags: [claude-code, hooks, gate, measurement, windows]
sources: []
raw: raw/2026-11-09-claude-settings-file-measurement.md
url: https://code.claude.com/docs/en/hooks
retrieved: 2026-11-09
created: 2026-11-09
updated: 2026-11-09
---

# Measurement: Claude Code `--settings <file>` and `"ask"`

One run of `claude -p` (Claude Code 2.1.319, Windows 11, `--model haiku`,
`--permission-mode acceptEdits`) in a throwaway git repository with a local Python listener; the
hooks came from a JSON file whose path contains spaces.

## Key points
- **File path works:** `--settings "<folder with spaces>\gate hooks.json"` installed both http
  hooks exactly like inline JSON: `PreToolUse` reached the listener for every `Write`, the
  `UserPromptSubmit` context reached the model, and `Authorization: Bearer $HANGAR_HOOK_TOKEN`
  was filled from the environment.
- **Host header:** `Host: 127.0.0.1:<port>`, as in the hook URL.
- **Deny:** the reason reached the model, prefixed with `PreToolUse:Write hook error:`; the file
  was not written.
- **Ask in print mode:** `permissionDecision: "ask"` did not write the file and the reason
  reached the model; with nobody to ask, print mode treats it like a refusal. In interactive
  mode the hooks reference says "ask" prompts the user to confirm
  ([[claude-http-hook-measurement]]); that was not measured.

## Significance for Hangar
- The agent gate ([[team-service-v1]], step B) gives npm's `claude.cmd` its hooks through
  `--settings <file>`, since inline JSON cannot pass cmd.exe safely; the token stays in the
  environment.
- The endpoint requires `Host` to be exactly `127.0.0.1:<port>`.
- When Hangar cannot hold one more edit it answers `"ask"`, so Claude Code's own prompt asks
  instead of the edit passing silently.

> [!question] Unverified
> `"ask"` in interactive mode, and the same flags through an actual `claude.cmd` launcher (the
> installed Claude Code is `claude.exe`; the launcher only passes argv on).
