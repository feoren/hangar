---
title: "Measurement: Codex CLI shell environment policy and HANGAR_SEND"
type: source
tags: [measurement, codex, environment, sandbox, hangar-send, windows, cmd-launcher, profiles]
sources: []
raw: raw/2026-11-26-codex-shell-environment-measurement.md
url: https://github.com/openai/codex/tree/rust-v0.172.0/codex-rs
retrieved: 2026-11-26
created: 2026-11-26
updated: 2026-11-26
---

# Measurement: Codex CLI shell environment policy and HANGAR_SEND

Codex CLI 0.172.0 on Windows 11, with `codex sandbox` standing in for the commands an agent runs,
plus a reading of the policy code at tag `rust-v0.172.0`. Record:
`raw/2026-11-26-codex-shell-environment-measurement.md` (its first sections ran with the owner's
own config and carry a correction).

## Key points

- **Codex's default passes everything.** Without a `[shell_environment_policy]` table the policy is
  `inherit = "all"` with `ignore_default_excludes = true` (source, measurement D): `HANGAR_SEND`
  arrives, but so do `HANGAR_GATE`, `HANGAR_HOOK_TOKEN`, `HANGAR_NOTIFY_TOKEN` and API keys.
- **The failure came from a user policy.** `inherit = "core"` (here with a `set` table) keeps only
  Codex's core list (`PATH`, `PATHEXT`, `COMSPEC`, `SYSTEMROOT`, `USERPROFILE`, `APPDATA`, `TEMP`
  and others on Windows; `PATH`, `HOME`, `SHELL`, `USER`, `LANG` and others elsewhere). `PATH`
  survives, so `hangar-send` is found but answers "HANGAR_SEND is not set".
- **Filter order:** inherit, default excludes (`*KEY*`, `*SECRET*`, `*TOKEN*`), `exclude`, `set`,
  `include_only`. `include_only` filters `set` values too (C). Patterns are case-insensitive globs.
  `inherit = "core"` plus `include_only` can never add a variable.
- **Layering:** a `-c` key merges into the file's table, so a `set` stays (B); a `-c` array replaces
  the file's array (F). Root `-c` overrides beat a profile file (P); `<name>.config.toml` merges key
  by key over `config.toml` (M). Legacy `profile = ...` or `[profiles.<name>]` make 0.172.0 refuse to
  start (L).
- **Core plus one variable works:** `inherit=all` with `include_only=[core list, 'HANGAR_SEND',
  set names]` gives what the person chose plus `HANGAR_SEND` (B), also through the npm `codex.cmd`
  launcher (G). `'PROGRAMFILES*'` stands for `PROGRAMFILES` and `PROGRAMFILES(X86)`, since a cmd.exe
  launcher refuses parentheses.
- **Hooks are outside the policy:** they get a snapshot of the Codex process environment
  (`codex-rs/hooks/src/registry.rs`, reviewer's reading), so excluding `HANGAR_GATE` from the
  agent's commands leaves the gate helper intact.

## Significance for Hangar

`src-tauri/src/codex_shell_env.rs` decides per Codex start, from `<CODEX_HOME or ~/.codex>/config.toml`
and the profile named by `--profile`/`-p`:

- No policy: only `-c shell_environment_policy.exclude=[Hangar's token variables, the profile's
  secret_env names]`.
- `inherit = "all"` that lets `HANGAR_SEND` through: the person's `exclude` patterns again plus
  those names, when they can pass a cmd.exe launcher; else nothing.
- `inherit = "core"` without its own `include_only`: `inherit=all` and `include_only=[core list,
  'HANGAR_SEND', set names]`.
- Anything else (`none`, a policy that drops it, a policy in the start arguments, a file that
  cannot be read or evaluated): the session is not offered sending at all.
- A project's `.codex/config.toml` between the working folder and the repository root (or every
  parent without a repository) that holds the policy, at the top or in a profile table, or that
  cannot be read as TOML: no sending either. How Codex trusts and layers project files is not
  modelled, so Hangar fails closed instead of evaluating them.

The overrides sit next to the gate's options and are never dropped for command line length. The
token never goes into an argument. Decision record: [[0012-cross-project-messages]].

> [!question] Unverified
> A live agent turn running `hangar-send` inside a Hangar session (only `codex sandbox` was run);
> the hook snapshot was read in the source, not measured; how a project's `.codex/config.toml`
> policy layers (not measured; Hangar offers no sending when one is there); Linux and macOS.
