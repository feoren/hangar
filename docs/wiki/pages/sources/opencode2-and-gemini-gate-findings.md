---
title: "Measurement: OpenCode 2.3.2 and Gemini CLI 0.70.0 break Hangar's gate recipes"
type: source
tags: [hooks, gate, opencode, gemini, windows, measurement]
sources: []
raw: raw/2026-11-22-opencode2-and-gemini-gate-findings.md
url:
retrieved: 2026-11-22
created: 2026-11-22
updated: 2026-11-22
---

# Measurement: OpenCode 2.3.2 and Gemini CLI 0.70.0 break Hangar's gate recipes

Two runs on Windows 11 with Hangar's recipes unchanged, plus a read of each program's shipped
code. Record: `raw/2026-11-22-opencode2-and-gemini-gate-findings.md`. Affects
[[agent-gate-adapters]] and [[opencode-gate-live-measurement]].

## Key points

- **OpenCode 2.3.2 (`@opencode/cli`) ignores the plugin.** With the plugin named in
  `OPENCODE_CONFIG_CONTENT` and a denying endpoint, `opencode run` wrote the file and the endpoint
  saw no request. The 2.x binary still reads `OPENCODE_CONFIG_CONTENT` but holds no
  `tool.execute.before`; its hook table uses names such as `tool.before` and `extension.before`.
  The 1.x recipe therefore fails open on 2.x.
- **Gemini CLI 0.70.0 skips a user-written system settings file.** Files named by
  `GEMINI_CLI_SYSTEM_SETTINGS_PATH` or `GEMINI_CLI_SYSTEM_DEFAULTS_PATH` load only when the file
  and every ancestor folder are owned by an administrator or SYSTEM and not writable by other
  groups (Windows ACL via PowerShell `Get-Acl`), or owned by root and not group or other writable
  (POSIX). A normal user's app cannot create such a file, so the `gemini_system_settings` recipe
  cannot work. User and workspace settings load without this check.
- The Gemini run itself stopped at the missing sign-in, so no Gemini hook was run.

> [!warning] Contradiction
> [[agent-hooks-per-session-research]] recommends `GEMINI_CLI_SYSTEM_SETTINGS_PATH` for a
> per-session hook. Read from the 0.70.0 bundle: that file is skipped unless an administrator
> owns it and its folders.

The ownership check came with Gemini CLI 0.60.0, and OpenCode dropped V1 plugins with 2.0.0
([[agent-hooks-cursor-cline-gemini-opencode2-research]]).

## Significance for Hangar

- The cause on OpenCode 2.x was found and fixed: V2 plugin API, `plugins` with a folder, and the
  shared background service; with a V2 plugin and `--standalone` the gate holds edits
  ([[opencode2-gate-live-measurement]]). 1.x from 1.19.5 keeps its recipe.
- Gemini CLI stays unmeasured; its recipe needs another per-session mechanism.
