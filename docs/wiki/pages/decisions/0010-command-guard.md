---
title: "ADR 0010: Command guard in the agent gate"
type: decision
status: accepted
superseded_by:
tags: [gate, hooks, team, shell, security, claude-code, pi, opencode, codex]
sources: ["[[shell-tool-input-shapes]]", "[[opencode-gate-live-measurement]]", "[[agent-hooks-per-session-research]]", "[[codex-gate-live-measurement]]", "[[claude-http-hook-measurement]]"]
created: 2026-11-14
updated: 2026-11-14
---

# ADR 0010: Command guard in the agent gate

## Context
The agent gate ([[team-service-v1]] step B, [[0008-private-and-team-sessions]]) holds an agent's
edit of a file another session is changing. The owner asked for a second use of the same path:
before an agent runs a risky shell command (force push, hard reset, deleting outside the project,
dropping a database, formatting a drive), Hangar asks the person, with remembered "always" and
"never" answers and the visible log of [[gate-rules]].

Starting point, from the code before this change:

- **Which hooks see shell tools.** None. Every recipe matched edit tools only: Claude Code
  `Edit|Write|MultiEdit|NotebookEdit`, Codex `apply_patch`, the Pi and OpenCode extensions a
  table of edit tools, Gemini, Copilot, Qwen and Droid their edit matchers
  ([[agent-gate-adapters]]). The guard therefore needs a wider hook per CLI.
- **Which sessions have hooks.** Team sessions only: `manager.rs` calls the gate's `prepare` for
  `SessionMode::Team` alone, and ADR 0008 decided that a private session has no gate.
- **What is known about the shell tools.** OpenCode's plugin hook sees `bash` calls (measured,
  [[opencode-gate-live-measurement]]: a logging plugin saw `write`, `read` and `bash`, and the
  model wrote a file through `bash` after two denied writes). Codex sends shell commands to
  `PreToolUse` as `Bash` or `exec_command` with the command text ([[agent-hooks-per-session-research]],
  desk research, not run). Claude Code's `Bash` tool input `tool_input.command` comes from its
  hooks reference.

- **Field names.** Read from the installed agents ([[shell-tool-input-shapes]]): Claude Code's
  transcripts show `Bash` and `PowerShell` calls with `input.command`; Pi 1.5.2 has `bash` and
  `powershell`, both `{ command, timeout }`; OpenCode 1.19.5's `bash` takes `command`, `timeout`
  and `workdir`.

> [!question] Unverified
> Not measured live: a shell call through each agent's hook end to end. `tests/gate_hook.rs`
> drives the endpoint and both extensions with these shapes. A wrong field name makes the guard
> see no command (the call goes ahead), never block one.

## Options

1. **A raw regex per rule over the whole command string.** Simple, but `echo "git push --force"`
   and a commit message match, and a path rule cannot tell `./node_modules` from `../other`.
2. **Tokenize the command, unwrap wrappers, match program and argument patterns per call;
   place path arguments against the session's project folder lexically.** More code, but quoting,
   chains and `sh -c` are read the way a shell reads them, and "outside the project" can be
   decided without touching the disk.
3. **Run the command in a sandbox or dry run.** Out of scope (non-goal: Hangar is no sandbox),
   and a dry run of an arbitrary command is not possible.

On reach:

- **A. Team sessions only.** Where the gate already has hooks.
- **B. Also private sessions behind a setting** (a second switch that extends the check to
  private sessions). Installing the gate's hooks into a private session is not supported for any CLI
  today: ADR 0008 keeps private sessions free of the gate, a private session has no worktree,
  the Codex recipe needs `--dangerously-bypass-hook-trust` and the Gemini one a workspace trust,
  both accepted only for team sessions in a worktree the user chose.

## Decision
Option 2 with reach A.

- **Code.** `src-tauri/src/command_split.rs` (pure: one lexer per dialect, wrappers, inline
  scripts, what could not be followed), `src-tauri/src/command_guard.rs` (pure: rules file,
  built-in rules, path placing, matching), `src-tauri/src/command_guard.toml` (the commented
  default file), `gate.rs` (`SHELL_TOOLS`, `CommandText`, `AskRule`, `CommandAsk`,
  `Gate::open_command`, `Gate::accepts`, `Answer::AlwaysCommand`/`NeverCommand`, the command
  reasons), `gate_rules.rs` (`RuleScope::Command`), `gate_service.rs` (`decide_command`,
  `ask_command`, `settle_commands`, `guard_rules`, `place_for`), `gate_plugins/*.js`
  (`decideCommand`). Window: `GateCard.tsx`, `CommandGuardSettings.tsx`, `commandGuard.ts`,
  `gate.ts` (`ruleText`).
- **Hooks.** Claude Code's matcher becomes `Edit|Write|MultiEdit|NotebookEdit|Bash|PowerShell`.
  Pi's extension sends `bash` as `Bash` and `powershell` as `PowerShell`; OpenCode's sends `bash`
  as `Bash`, on Windows as `Shell` (shell not known in advance), with `workdir` resolved against
  the session's folder as the working folder. Every one goes to the same endpoint with
  `tool_input.command`. The shell tools always get the hook in a team session; the setting is
  read per command, so switching it applies to running sessions.
- **Not covered, and why.** Codex: its shell tool name (`Bash` or `exec_command`) and input shape
  are not measured, and a second matcher entry changes the measured `-c` value that must pass
  npm's `codex.cmd` (a `|` is refused there, [[agent-gate-adapters]]); a live run first. Gemini
  CLI, Copilot CLI, Qwen Code, Oh My Pi and Droid are not offered as team sessions yet. Private
  sessions: reach A above.
- **Rules file.** `command-guard.toml` in Hangar's config folder (next to `profiles.toml`),
  written once with the commented defaults when missing, read again when its modification time or
  length changes (taken from the opened file, one look per checked command). Each `[[rule]]` has
  `id`, `description`, `program` (names, `*` wildcard) and optional `subcommand`,
  `value_options`, `any`, `all`, `none` (option patterns: `--long`, `-s` also in groups but not
  in a PowerShell word such as `-Force`, PowerShell `-Word` with abbreviations and `:$false`, cmd
  `/s`; an option word with a brace expansion counts through its variants, or through the part
  before an expansion that cannot be taken apart), `words` (word sequences in an argument,
  ignoring case) and `outside_project`. Unknown fields make a rule invalid (a typo must not widen
  or narrow it silently); ids starting with `hangar-` are reserved; an invalid rule or setting is
  reported (Settings lists it) and the others apply; a file that is not TOML or over 1 MiB keeps
  the default rules and says so. Settings also says when the file holds fewer rules than Hangar
  ships. At most 200 rules.
- **Default rules.** `git push` with `--force` or `-f` (not `--force-with-lease` or
  `--force-if-includes`, which refuse to overwrite unseen work), `git reset --hard`, `git clean`
  with `-f` and without `-n`, `rm -r` (also PowerShell's `rm -Recurse`) / `Remove-Item -Recurse`
  / `del /s` / `rd /s` on a path outside the project, `DROP TABLE`, `DROP DATABASE`,
  `DROP SCHEMA`, `TRUNCATE` through mysql, mariadb, psql, sqlite3, sqlcmd, duckdb or
  clickhouse-client (any argument, which covers `-e`, `-c`, `--execute`, `-Q` and sqlite3's
  positional SQL), and format, diskpart, mkfs, Format-Volume, Clear-Disk.
- **Built-in rules,** not in the file and not switched off there: `hangar-too-complex` (nested
  deeper than 6 levels, more than 512 calls, a quote, substitution or block comment left open,
  an `-EncodedCommand` that does not decode), `hangar-stdin-script` (a shell reading its script
  from stdin: a pipe into `sh`/`bash`/`pwsh`/`cmd`, a here-string, `bash -s`, `pwsh -Command -`),
  `hangar-too-long` (over 64 KiB or with a NUL; the card shows the start) and `hangar-config`
  (below).
- **Reading a command.** The dialect follows the tool: `Bash` POSIX, `PowerShell` PowerShell,
  `Shell` all three with every hit counted. POSIX: quotes, every backslash outside quotes
  escapes (as Git Bash reads it), `$'...'` decoded, `$(...)` and backticks checked as commands of
  their own with an unknown value. PowerShell: backtick escapes, a backslash is plain, doubled
  quotes, typographic quotes and dashes, `$(...)`, `<# #>`, `--%`. cmd: carets removed, double
  quotes, no backslash escape. Separators and comments in each; redirection targets are kept
  apart from the arguments and checked against Hangar's own settings (`> command-guard.toml`).
  A POSIX `$"..."` drops its `$`; `$@`, `$*` and `${...}` inside a word are also read as empty
  (`g$@it`). A PowerShell argument in parentheses (`(Join-Path $HOME x)`) is an unknown value
  and its inside is split as a command of its own; a single plain string in parentheses
  (`(".\build")`) is read as that string.
  Wrappers skipped: assignments, `sudo`, `doas`, `env` (also `-S`), `nohup`, `nice`, `time`,
  `timeout`, `command`, `exec`, shell keywords. Inline scripts split again in their own dialect:
  `sh`/`bash`/`zsh`/... `-c` (also after options with a value: `-o`, `+o`, `-O`, `+O`,
  `--rcfile`, `--init-file`), `pwsh`/`powershell` `-Command`, `-CommandWithArgs`/`-cwa` and
  `-EncodedCommand` (base64 UTF-16LE), Windows PowerShell's positional command, `cmd /c`, `/k`
  and `/r`, `eval`, `Invoke-Expression`/`iex`, `wsl` (`-e` as a program's words, otherwise the
  rest as a POSIX command line). `start` (cmd), `Start-Process` and `saps` are unwrapped too
  (`-FilePath`, `-ArgumentList` with commas, positional values, cmd's `/b` and an empty title):
  agents on Windows reach for them, and the unwrapping is a few lines; each unwrapping counts as
  one level of the depth limit. `--version`, `--help`,
  `pwsh -Version` and `-?` are not taken for a script from stdin. A `cd` moves the working
  folder for the calls after it; `cd -`, a bare `cd` and `popd` leave it unknown.
- **Every hit counts.** All rules that match any call are collected, each once. A remembered deny
  for any of them blocks the command; rules with a remembered allow drop out; the rest go on one
  card. "Always allow" remembers each rule shown (the card repeats them next to the button).
- **Outside the project.** A path argument is placed lexically from the hook's working folder:
  `~`, `$HOME`, `$env:USERPROFILE`, `%USERPROFILE%` are the home folder; drive letters, UNC paths
  and Git Bash's `/c/...`, `/cygdrive/c/...`, `/mnt/c/...` on Windows; `..` resolved. It counts as
  outside unless it lies strictly inside the session's worktree and not in its `.git`; a
  wildcard right in the worktree (`./*`) is inside. A path built from a variable or a
  substitution cannot be placed and counts as outside, and so does every relative path when the
  hook sent no usable working folder (an over-long or NUL `cwd` is dropped for every tool, not
  refused, so the request is still checked): the person is asked rather than not.
- **Hangar's own settings.** Protected are the config folder (the guard's rules, the profiles),
  the gate's folder `<local data>/gate` (hook settings, the extensions' session folders) and the
  two files `command-guard.toml` and `gate-rules.json` themselves, never the local data folder as
  a whole: the team sessions' worktrees live in `<local data>/worktrees`, and a session there is
  an ordinary project. An edit, an argument or a redirection target that lies in one of them is
  held with `hangar-config`. For a path outside the session's project, or one Hangar cannot
  place, names count too: the app identifier (the folder's last part) anywhere in it, and either
  file name or the identifier as a path part, also through a wildcard (`*`, `?`, `[...]`) with
  at least five letters of its own outside a plain extension (`com.f*`, `command-g*.toml`,
  `[c]ommand-guard.toml`; not `*.json` or `com.*`) or a Windows 8.3 short name (`COMFEO~1`,
  `COMMAN~1.TOM`). A wildcard right below the folder Hangar's folder sits in counts whatever it
  spells (`~/.config/c*`, `$env:APPDATA\c*`, `%LOCALAPPDATA%\c*`, `$XDG_CONFIG_HOME/*`,
  `~/Library/Application Support/c*`). A path inside the project counts by where it lies only. The rule applies
  while the guard setting is off, switching the setting off does not release it, and it offers
  no remembered answer. Not solvable by reading text: a name put together at run time
  (`"com." + "feoren.hangar"`, `$a$b`). Residual risk, honestly: the agent runs as the same user, and the
  hook token is in its environment; anything that writes the files without naming them (a
  script, an encoded path) is not seen. This is no protection against a malicious agent, only
  against an agent quietly loosening the person's rules.
- **Decision flow.** A match opens a card like a file conflict: the rules' descriptions (Hangar's
  own rules and unchanged default rules in the window's language, `command_guard::is_builtin`;
  the user's rules verbatim), the program call that matched, and the command as sent (up to
  4000 characters, then "N more characters not shown"; control and invisible characters are
  shown by the window as styled placeholders, so typed `<U+…>` text cannot pose as one), answers
  "Allow once" and "Block", and under "Remember for this project" "Always allow" and "Always
  block" for the rules named there. A remembered answer is a gate rule with scope `command`
  whose `path` is the guard rule's id, in the same `gate-rules.json` and log; it covers every
  command of the project (git common folder) matching that guard rule, and a deny wins over an
  allow. File answers do not fit a command decision and command answers no edit
  (`Answer::fits`, checked before any rule is saved).
- **Fail safe.** No answer within the gate's 10 minutes denies the command with a reason that
  names the rules. A full queue denies it too ("too many decisions waiting, retry"): Claude
  Code's "ask" is not always a person [UNVERIFIED: in its bypass, auto-accept and `-p` modes the
  permission prompt may be skipped or fail], and OpenCode turns Hangar's "ask" into a thrown
  error (the tool is blocked, no prompt). Errors still fail open (endpoint gone, malformed
  answer): the gate stays a coordination aid, and a dead Hangar must not stop every shell
  command.
- **Setting.** On by default, under the gate setting and only effective while the gate is
  (Settings > General); its help text names the agents it works for (Claude Code, Pi, OpenCode).
  Turning it off lets held commands go on, except `hangar-config`.
- **Bounded reading.** Every recursion over the agent's text has a fixed bound: inline scripts
  and substitutions 6 levels (`command_split::MAX_DEPTH`), brace expansion 8 braces per word
  (`command_guard::MAX_BRACE_GROUPS`, more fall back to the prefix rule). Each hook connection
  runs on a thread with an 8 MiB stack (`gate_service.rs`, `CONNECTION_STACK`), since an
  overflow there would end the app and every session in it.
- **Honest limits.** A safety net, not a sandbox. Not recognized: a script file (`bash x.sh`,
  `pwsh -File`), a program in a variable, an alias or function, `xargs`, `find -delete`, a
  refspec force push (`git push origin +main`), tools other than the rules name. Asked about but
  not read: scripts from stdin, encoded or too complex commands. Nothing is enforced when the
  agent's hook fails open.

## Consequences
- Claude Code team sessions started before this version keep their old matcher until restart.
- Each `Bash` call of a Claude Code team session makes one local HTTP round trip, also while the
  guard is off.
- A Hangar older than this version reading `gate-rules.json` skips command rules (unknown scope)
  and drops them on its next save.
- The default rule descriptions in the file are English, as the file is the user's; the card
  shows its own translation as long as the user has not changed them.
- Working on Hangar itself in a team session: commands naming `command-guard.toml` or
  `gate-rules.json` inside the project pass; outside it they ask every time. So does any path
  outside the project that names the app identifier (the last part of Hangar's config folder);
  a search for it inside the project (`rg com.feoren.hangar src-tauri/`) passes, as
  `protected_names` counts only for paths outside the project or not placeable.

## Evidence
- Code and tests: `command_split::tests` (dialects, inline scripts, open quotes, depth, stdin,
  bounds), `command_guard::tests` (every default rule with wrappers and negatives, each
  dialect's quotes and escapes, every hit reported once, what cannot be read, Hangar's own
  settings, built-in wording, an invalid rule among valid ones, a broken file, path placing, flag
  styles), `gate_rules::tests::command_rules_answer_their_guard_rule_and_no_file`,
  `gate_service::tests::a_risky_command_waits_for_the_person_and_harmless_ones_pass`,
  `a_remembered_answer_covers_the_rule_in_the_project`,
  `every_rule_of_a_command_counts_and_only_the_unanswered_ones_are_shown`,
  `a_command_nobody_answers_is_blocked`, `a_full_queue_blocks_a_command_instead_of_handing_it_on`,
  `an_overlong_command_is_asked_about_not_handed_on`,
  `a_working_folder_hangar_cannot_use_still_asks`,
  `hangars_own_settings_are_asked_about_even_with_the_guard_off`,
  `switching_the_guard_off_lets_a_held_command_go_on`,
  `the_rules_file_is_written_once_and_read_again_when_it_changes`, `an_edit_with_a_working_folder_hangar_cannot_use_is_still_checked`, `hangars_worktrees_in_its_data_folder_are_not_its_settings`; `command_guard::tests` also `a_huge_brace_expansion_does_not_exhaust_a_small_stack`, `hangars_own_worktrees_are_ordinary_projects`, `redirections_wildcards_and_short_names_reach_the_settings`, `empty_special_parameters_do_not_hide_a_rule`, `powershell_parenthesized_and_wsl_arguments_are_read`; `command_split::tests::shell_options_with_values_do_not_hide_the_script`, `redirection_targets_and_parenthesized_values_are_kept_apart`; `tests/gate_hook.rs`
  `claude_bash_hook_holds_a_risky_command_until_the_person_answers`,
  `pi_and_opencode_extensions_send_shell_commands_to_the_guard`,
  `extension_shell_tools_carry_their_dialect_and_working_folder`; Vitest `gate.test.ts` ("held
  shell commands"), `commandGuard.test.ts`, `GateCard.test.tsx`.
- Field names: [[shell-tool-input-shapes]]. Hook mechanics: [[claude-http-hook-measurement]],
  [[opencode-gate-live-measurement]], [[codex-gate-live-measurement]],
  [[agent-hooks-per-session-research]].
