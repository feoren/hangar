---
title: "ADR 0023: Hangar's status line inside the agent CLIs, and a Hangar bar for the others"
type: decision
status: accepted
superseded_by:
tags: [agents, status-line, claude-code, qwen-code, codex, settings, terminal]
sources: ["[[status-line-measurement]]"]
created: 2026-11-27
updated: 2026-11-27
---

# ADR 0023: Hangar's status line inside the agent CLIs, and a Hangar bar for the others

## Context
People who work with several agent sessions want the same two lines everywhere: where a session
works (model and effort, account, folder and worktree, branch with ahead, behind, staged,
modified, untracked and conflicts, repository, pull request) and how it is doing (context with a
`/compact` hint from 80 %, prompt cache, changed lines, cost and cost per hour, run time and API
share, the 5 hour and 7 day limits with their resets). Some agent CLIs run a command of the
user's for their status line; most have none.

Facts this decision rests on (docs read 2026-11-27, measurements of the same day):
- Claude Code: `--settings` (inline JSON or a file) applies over user, project and local settings,
  below managed ones, and writes no file; keys it does not set keep their file values.
  `statusLine` is `{type: "command", command, padding, refreshInterval >= 1,
  hideVimModeIndicator}`; the command gets the session JSON on stdin and `COLUMNS` and `LINES`
  in its environment. On Windows it runs through Git Bash, or PowerShell without it; backslashes
  in the command are eaten by Git Bash. Blank until the folder is trusted; `disableAllHooks`
  outside managed settings hides it. Sources: https://code.claude.com/docs/en/settings ,
  https://code.claude.com/docs/en/statusline , https://code.claude.com/docs/en/cli-reference .
  Whether two `--settings` are merged is not documented, so the status line must go into the
  one object Hangar already passes for the gate and the notice hooks.
- Qwen Code: `ui.statusLine` `{type: "command", command, refreshInterval, respectUserColors,
  hideContextIndicator}`, at most two lines, a 5 s timeout, `cmd.exe` on Windows; stdin carries
  `session_id`, `version`, `model.display_name`, `context_window.*`, `workspace.current_dir`,
  `git.branch`, `worktree.*`, `metrics.models.<id>.tokens.*`, `metrics.files.*`, `vim.mode`.
  Source: https://qwenlm.github.io/qwen-code-docs/en/users/features/status-line/ . Not
  installed here; nothing run live.
- Codex CLI has built-in items in `tui.status_line = [ids]`. Measured with 0.172.0: `-c
  tui.status_line=42` fails the bootstrap configuration with "expected a sequence in
  `tui.status_line`", a list is taken, and every id Hangar uses (`model-with-reasoning`,
  `current-dir`, `git-branch`, `branch-changes`, `pull-request-number`, `context-used`,
  `used-tokens`, `five-hour-limit`, `weekly-limit`) is in the installed binary. Source:
  codex-rs `config/src/types.rs`, `tui/src/bottom_pane/status_line_setup.rs`. The rendered TUI
  line was not looked at.
- Gemini CLI's `ui.footer.items` would need the system settings file Hangar's gate recipe writes,
  which Gemini CLI skips since 0.60.0 when an administrator does not own it
  ([[opencode2-and-gemini-gate-findings]]). Not offered.
- Copilot, Cursor, Kimi, Antigravity and Kode take a status line only from the user's own file;
  Hermes, Kiro, Goose, Aider, Cline, Crush, Vibe, Amp, Forge, Codebuff and OpenClaw have none. Pi
  and Oh My Pi could draw a footer from an extension (`ctx.ui.setFooter`); left for later.

> [!question] Unverified
> Claude Code and Qwen Code rendering Hangar's line in a live session, and Codex's rendered
> TUI line, were not looked at; the recipes follow the docs and the measurement above.

## Options
- Write the line into the user's own settings files: changes files outside Hangar and every
  session outside it; refused.
- A second `--settings` next to the gate's: merging is undocumented, so one object it is.
- A separate small binary like `hangar-send`: smaller start, but a second program to bundle and
  find; the subcommand reuses `git::Git` and is already where `gate-hook` lives.
- Draw the line only in Hangar's window for every program: loses what only the CLI knows
  (cost, limits, cache, pull request) for the programs that can draw it themselves.

## Decision
1. **Renderer as a subcommand.** `hangar status-line <claude|qwen>` (`status_line.rs`, dispatched
   in `main.rs` before Tauri like `gate-hook`) reads at most 1 MiB of stdin, prints two ANSI lines
   and always exits 0. Input that is not a JSON object prints nothing; a segment without data is
   left out; with `COLUMNS` the least important segments go first (version, countdowns, API share,
   plan, cache, rate, session name, repository, ... last model, context and cost), a glued
   segment with the one it belongs to. Every text from the input or from git passes `clean`:
   controls (C0, DEL, C1) and direction or zero width characters become `?`. Links (OSC 8) only
   for Claude Code and only for `https` URLs of printable ASCII. The look follows the owner's own
   Claude Code status line (amended 2026-11-27): `[model]`, emoji labels (folder, branch, link,
   cost, time, limits), ` | ` between segments; model, context, cost and time always show for
   Claude Code. A cold prompt cache reads "cold" in the user's interface language
   (`GetUserDefaultUILanguage` on Windows, `LC_ALL`, `LC_MESSAGES` or `LANG` elsewhere), and a
   cache miss shows its causes for 10 minutes.
2. **Data beyond stdin.** `git status --porcelain=v2 --branch` through `git::Git` (no hooks, filter
   drivers neutralized, no network, 2 s for all calls, folder checks of `check_folder`) and, for
   Claude Code, the display name and plan from `oauthAccount` in `~/.claude.json` (or
   `CLAUDE_CONFIG_DIR`), with the e-mail as the owner's line shows it (amended 2026-11-27: the
   first version left it out); never any key, nothing logged. Both are cached per session
   id for 5 s in `<local data>/status-line/`, named by `HANGAR_STATUS_DIR`; files untouched for 24
   hours are removed on a cache miss. Measured on the debug build: 385 ms with a fresh git
   reading, 26 ms from the cache.
3. **Where it goes.** `GateService` owns the settings Hangar hands an agent start, so it adds the
   status line to the same `--settings` object as the gate's or the notice hooks (Claude Code),
   to the gate's system settings file (Qwen Code) or as a `-c` override after the gate's (Codex).
   A start without either gets it alone through `LaunchGate::status_only`: inline or, for a
   cmd.exe launcher, `claude-status-line.json`; Qwen Code a `qwen-status-line.json` named by
   `QWEN_CODE_SYSTEM_SETTINGS_PATH`, never where an administrator's system settings exist. A
   profile that brings its own `--settings`, Qwen system settings or `tui.status_line` keeps them.
4. **Command line.** Forward slashes on Windows. Claude Code's two shells read no quoting alike,
   so its command needs a path without blanks; a path with blanks falls back to its 8.3 short
   name (`GetShortPathNameW`), and without one the session gets no status line rather than a
   broken one. Qwen Code (cmd.exe) and POSIX shells get a double quoted path. Paths with `$`,
   `"`, parentheses or other characters a shell reads are refused.
5. **Setting.** `statusLine` in the protected settings (`protected-settings.json`, on by default,
   an older file reads as on), applied to `GateService` at start and when saved; it takes effect
   on the next start. `SessionInfo.statusLine` says whether the current process carries it.
6. **Hangar bar.** Every other agent session shows a slim bar above the terminal (`HangarBar.tsx`)
   with what the window already knows: model and effort, folder and worktree, branch and changed
   files (from the claims), context, output rate, cost and run time; container queries drop the
   least important segments. A display setting `hangarBar`, on by default.

## Consequences
- Sessions from Hangar show the same lines whatever the user's own status line is; the user's
  files stay untouched, and turning the setting off restores the program's own line on the next
  start.
- Each refresh starts Hangar's executable (event driven plus every 30 s); the cache keeps git off
  most of them.
- Open: Claude Code and Qwen Code rendered live in a Hangar session; Codex's rendered TUI line;
  a Pi and Oh My Pi footer; ahead and behind in the Hangar bar (no frontend source yet).

## Evidence
- [[status-line-measurement]]: Codex `-c tui.status_line`, its item ids, the renderer's timing,
  short names.
- Docs as listed under Context (read 2026-11-27).
- Tests: `src-tauri/src/status_line.rs` (segments, width, missing data, broken JSON, controls,
  cache, command lines, Codex arguments), `src-tauri/src/gate_service.rs` (settings with and
  without the gate, cmd.exe file, Qwen, Codex), `src-tauri/tests/pty.rs`
  (`a_start_without_hooks_gets_the_status_line_alone`), `src/HangarBar.test.tsx`.
