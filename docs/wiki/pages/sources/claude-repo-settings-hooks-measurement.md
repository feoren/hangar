---
title: "Measurement: a repository's Claude Code settings switch off --settings hooks"
type: source
tags: [claude-code, hooks, gate, notices, measurement, windows, security]
sources: ["[[claude-http-hook-measurement]]"]
raw: raw/2026-11-24-claude-repo-settings-hooks-measurement.md
url:
retrieved: 2026-11-24
created: 2026-11-24
updated: 2026-11-24
---

# Measurement: a repository's Claude Code settings switch off `--settings` hooks

Nine `claude -p` runs (Claude Code 2.1.331, Windows 11, `--model haiku`) in a throwaway git
repository with a local listener, each with one checked-in project settings file, and the same
http hooks passed through `--settings` as Hangar does ([[claude-http-hook-measurement]]).

## Key points
- **`disableAllHooks: true`** in `.claude/settings.json` or `.claude/settings.local.json` of the
  working folder: none of the `--settings` hooks ran. `false` changed nothing.
- **`allowedHttpHookUrls`**: an empty list or one without a matching pattern stopped every http
  hook; `["http://127.0.0.1:*"]` let them through (`*` is a wildcard).
- **`httpHookAllowedEnvVars: []`**: the hooks still ran, but without the `Authorization` value, so
  Hangar's endpoint answers them 401. A list naming the variable keeps it.
- **Which folders:** with the working folder in a subfolder, a `disableAllHooks` file at the git
  root above it was not applied. Only the working folder's `.claude/` counted in that run.

## Significance for Hangar
- A repository can silently switch off the gate and the notice hooks of [[0016-permission-notices]].
  Hangar now reads these keys before every Claude Code start (`src-tauri/src/repo_hooks.rs`), from
  the working folder up to the git root (more than Claude Code reads, the safe direction), plus the
  user's and the managed settings file. A project file Hangar cannot read without following a
  link (or one too big) counts as a hit, and so does text that is no strict JSON but names one of
  the keys. On a hit the hooks still go in, but a team session shows
  `repoSettings` instead of an active gate, and the session counts as unsure for link groups:
  a group with it starts with "Ask before pasting" on ([[0014-session-links]]).
- `allowManagedHooksOnly` is treated the same way, from the reference only.

> [!question] Unverified
> How Claude Code merges `allowedHttpHookUrls` or `httpHookAllowedEnvVars` across several settings
> levels (one file allowing, another not) was not measured; Hangar counts any one file that would
> stop the hooks. Interactive mode was not measured.
