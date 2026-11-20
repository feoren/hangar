---
title: "ADR 0010: Command guard in the agent gate"
type: decision
status: accepted
superseded_by:
tags: [gate, hooks, team, shell, security, claude-code, pi, opencode, codex]
sources: ["[[shell-tool-input-shapes]]", "[[opencode-gate-live-measurement]]", "[[agent-hooks-per-session-research]]", "[[codex-gate-live-measurement]]", "[[claude-http-hook-measurement]]"]
created: 2026-11-14
updated: 2026-11-20
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
- **Hooks.** Claude Code's matcher is `*` (every tool; amended 2026-11-15, it was
  `Edit|Write|MultiEdit|NotebookEdit|Bash|PowerShell`): which tool is an edit
  (`gate::EDIT_TOOLS`, a test keeps every recipe's edit matcher inside it) and which runs a
  command is decided by Hangar. `Bash`, `PowerShell`, `Shell` and any other tool with a string
  `tool_input.command` that is no edit tool are checked by the guard, the unknown ones in all
  three dialects; every other tool passes at once. A named list would let a shell tool Hangar
  does not know (a new Claude Code tool, an MCP tool) through unseen.
  Pi's extension sends `bash` as `Bash` and `powershell` as `PowerShell`; OpenCode's sends `bash`
  as `Bash`, on Windows as `Shell` (shell not known in advance), with `workdir` resolved against
  the session's folder as the working folder. Every one goes to the same endpoint with
  `tool_input.command`. The extensions block a shell command whose question to the endpoint
  fails (unreachable, refused, unreadable answer): the guard fails closed there, edits still
  fail open. The shell tools always get the hook in a team session; the setting is
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
  (below). `hangar-too-long` also holds an edit whose path is over 4 KiB or has a NUL: the
  endpoint keeps such a request and asks instead of refusing it (a refused request makes Claude
  Code go on), and the `gate-hook` helper answers "ask" for it instead of skipping it.
  `hangar-too-complex` also covers cmd's substring and replace expansions (`%x:~0,0%`, also read
  with them removed) and delayed variables (`!x!`), and `git -c alias.*=...`,
  `-c clean.requireForce=...` or `--config-env` for them; `hangar-stdin-script` also a database
  client (mysql, mariadb, psql, sqlite3, sqlcmd, duckdb, clickhouse-client) fed by a pipe without
  an SQL option or argument, or by a here-document built from variables.
- **`hangar-secret-file`** (added 2026-11-20, [[0015-secret-guard]]): `git add` or `git stage`
  naming a file that usually holds a secret (`.env`, `.env.<name>` except the example, sample and
  template files, `*.pem`, `*.key`, `*.p12`, `*.pfx`, SSH private keys, `credentials.json`,
  `.netrc`, `.pgpass`). Read from the words like the other rules; `git add .` or `-A` is not
  asked about, since only the folder would tell.
- **Reading a command.** The dialect follows the tool: `Bash` POSIX, `PowerShell` PowerShell,
  `Shell` all three with every hit counted. POSIX: quotes, every backslash outside quotes
  escapes (as Git Bash reads it), `$'...'` decoded, `$(...)` and backticks checked as commands of
  their own with an unknown value. POSIX here-documents (`<<`, `<<-`): the body is
  read from the next line break outside quotes up to the delimiter line (quotes removed, tabs
  stripped for `<<-`); a quoted delimiter makes it data, an unquoted one runs only its `$(...)`
  and backticks; a missing delimiter line is unclosed. Inside `$(...)` the end of the
  substitution is looked for past such bodies, so a quote or `)` in one is text (the commit
  message an agent writes with `git commit -m "$(cat <<'EOF' ... EOF)"`). Here-strings and here-document bodies are
  what the call reads, matched by `words` (`psql <<< 'DROP TABLE x'`). Process substitutions
  (`<(...)`) are split and name a pipe. `{` and `}` group only as words of their own; inside a
  word they are brace expansion, applied to every word of a POSIX call (`{git,push,--force}`)
  within the bounds below. `$IFS` reads as a word break and `${x:-w}`, `${x-w}`, `${x:=w}`,
  `${x=w}` as `w`, each as one more reading of the call. PowerShell: backtick escapes, a
  backslash is plain, doubled quotes, typographic quotes and dashes, `$(...)`, `<# #>`, `--%`,
  here-strings (`@'...'@` literal, `@"..."@` with `$(...)`, ending only at a line that starts
  with the quote and `@`, unclosed otherwise), `[scriptblock]::Create('...')`,
  `.InvokeScript('...')` and `.NewScriptBlock('...')` with a literal argument read like `iex`. cmd: carets removed, double
  quotes, no backslash escape. Separators and comments in each; redirection targets are kept
  apart from the arguments and checked against Hangar's own settings (`> command-guard.toml`).
  A POSIX `$"..."` drops its `$`; `$@`, `$*` and `${...}` inside a word are also read as empty
  (`g$@it`). A PowerShell argument in parentheses (`(Join-Path $HOME x)`) is an unknown value
  and its inside is split as a command of its own; a single plain string in parentheses
  (`(".\build")`) is read as that string.
  Wrappers skipped: assignments, `sudo`, `doas`, `env` (also `-S`), `nohup`, `nice`, `time`,
  `timeout`, `command`, `exec`, `winpty`, `setsid`, `stdbuf`, `ionice`, `busybox`, shell
  keywords. `find -exec`/`-execdir`/`-ok`/`-okdir` runs its command with `{}` read as
  `<start folder>/*`; `trap 'cmd' SIG` splits its script. A POSIX `.` or `source` is a script
  file, not a wrapper; from `/dev/stdin`, `/dev/fd/*`, `/proc/*/fd/*` or a process substitution
  it is a script from stdin, as is `bash /dev/stdin` and `pwsh -File -`. Inline scripts split again in their own dialect:
  `sh`/`bash`/`zsh`/... `-c` (also after options with a value: `-o`, `+o`, `-O`, `+O`,
  `--rcfile`, `--init-file`), `pwsh`/`powershell` `-Command`, `-CommandWithArgs`/`-cwa` and
  `-EncodedCommand` (base64 UTF-16LE), Windows PowerShell's positional command, `cmd /c`, `/k`
  and `/r`, `eval`, `Invoke-Expression`/`iex`, `wsl` (`-e` as a program's words, otherwise the
  rest as a POSIX command line). `start` (cmd), `Start-Process` and `saps` are unwrapped too
  (`-FilePath`, `-ArgumentList` with commas, positional values, cmd's `/b` and an empty title):
  agents on Windows reach for them, and the unwrapping is a few lines; each unwrapping counts as
  one level of the depth limit. `--version`, `--help`,
  `pwsh -Version` and `-?` are not taken for a script from stdin. A `cd` adds its target to the
  working folders the later calls may run in and keeps the folder before it (a `cd` in a
  subshell, a pipeline or one that fails changes nothing); a path counts as outside when it is
  outside from any of them, at most 16 folders, then unknown. `cd -`, a bare `cd` and `popd` add
  an unknown folder.
- **Every hit counts.** All rules that match any call are collected, each once. A remembered deny
  for any of them blocks the command; rules with a remembered allow drop out; the rest go on one
  card. "Always allow" remembers each rule shown (the card repeats them next to the button).
- **Outside the project.** A path argument is placed lexically from the hook's working folder:
  `~`, `$HOME`, `$env:USERPROFILE`, `%USERPROFILE%` are the home folder; drive letters, UNC paths
  and Git Bash's `/c/...`, `/cygdrive/c/...`, `/mnt/c/...` on Windows; `..` resolved. It counts as
  outside unless it lies strictly inside the session's worktree and not in its `.git`; a
  wildcard right in the worktree (`./*`) is inside. Long options count by any prefix of two
  letters or more (`--ha` for `--hard`, as git and getopt read an unambiguous prefix; an
  ambiguous one asks too). PowerShell's `-Path:C:\x` names a path like a positional argument,
  and a delete with no path of its own in a pipeline takes the paths and switches of the call
  before it (`Get-ChildItem x -Recurse | Remove-Item`), or counts as unplaceable. A path built from a variable or a
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
  `COMMAN~1.TOM`). An edit is also held when the path with its existing part resolved (the same
  resolution the conflict check uses, only on local drives or the worktree's share) lies in one
  of them: a junction or symlink inside the project into Hangar's folder. A wildcard right below the folder Hangar's folder sits in counts whatever it
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
  error (the tool is blocked, no prompt). The extensions block a shell command whose question
  fails (amended 2026-11-15), and since the third audit also one their own handler fails on
  (an input it cannot read): only an edit goes ahead then. Claude Code's `PreToolUse` hook
  carries `"onFailure": "block"` (third audit; documented since Claude Code 2.1.295, installed
  2.1.325, `raw/2026-11-09-claude-hooks-reference.md` in [[claude-http-hook-measurement]]): a hook that cannot connect, times out or gets a non-2xx
  answer blocks the tool call instead of letting it run, so a stopped or overloaded endpoint no
  longer waves commands through. The prompt hook keeps the default ("continue"): a failed fact
  lookup must not hold up the person's prompt. With the gate switched off the endpoint answers
  every tool hook "no decision" (200), also one it cannot read or has no slot for, since an
  error would now block the call. The field is written only for a Claude Code whose installed
  version (the cached version query of [[0003-program-versions]], asked by the session
  manager at each start of a Claude Code team session) is 2.1.295 or later
  (`gate::CLAUDE_ON_FAILURE_SINCE`); an older or unknown version gets the settings without it,
  as before, since whether it ignores the unknown field or rejects the whole `--settings` is
  not known. Such a session keeps the earlier behavior: a failed hook lets the call run. Not
  measured live. The endpoint still keeps errors rare: when every slot for unknown clients is
  taken the oldest is closed for the newcomer once it waited 1 s for its head, instead of
  turning the newcomer away (a hook sends its head at once, so silent clients cannot keep hooks
  out and a burst of parallel hooks is never cut; the head limit stays 5 s, as no measurement
  says how fast the head arrives), and at most 128 threads read heads unless a silent one makes
  room. A known session holds at most 32 connections (128 in all); one more is not left
  unanswered but denied unread with "too many tool calls waiting, retry" while the gate is on,
  since Claude Code hooks every tool and an unanswered hook would let a shell command run
  unchecked. Only a flood past 128 fresh connections a second can still make a hook fail; that
  needs code already running as the user.
  Switching the guard off is read again under the gate's lock right before a card opens, so a
  command checked just before cannot open a card that nothing releases.
- **Setting.** On by default, under the gate setting and only effective while the gate is
  (Settings > General); its help text names the agents it works for (Claude Code, Pi, OpenCode).
  Turning it off lets held commands go on, except `hangar-config`.
- **Fail closed (amended 2026-11-15, second audit).** Where a lexer meets a construct it does
  not model exactly, the command asks with `hangar-too-complex` instead of a guess: a
  here-document delimiter with `$`, a backtick or a carriage return (`<<a$(b)`, `<<'EOF\r'`), a
  `${` or `$[` left open, a `$(...)` nested in quotes more than 32 deep, a brace expansion past
  64 variants, 8 braces or 1024 words, default values nested more than 16 deep, a `find` with
  more than 8 start folders before its `-exec`. Modelled now as bash reads them: `#` comments at
  a word start and `case ... esac` patterns inside `$(...)` (verified with bash 5.3; a
  backtick substitution has no comments, as in bash), arithmetic `((...))`, `$((...))`,
  `${...}` and `$[...]` (where `<<` shifts; the inside of `((...))` is still read as commands,
  so a substitution or the subshells bash may make of it are seen), brace expansion only for a
  call with a brace outside quotes (a quoted JSON text is never expanded). PowerShell: `{` and
  `}` end the word and the command wherever they stand (`& {...}`, `%{...}`, `if (x) {...}`,
  `-ScriptBlock {...}`), except `${env:x}` and the hashtable `@{...}` (one word, its inside read
  as a script). Parameters with `--`, `/` or a typographic dash, `-Name:value` for `-Command`,
  `-EncodedCommand`, `-File`, `-FilePath` and `-ArgumentList`, a `-Command` anywhere later in
  the line, `-ConfigurationFile` and `-EncodedArguments` with their values; `... | iex` is a
  script from stdin. Wrappers take their options as getopt does (`sudo -Eu root`,
  `--user root`, `exec -a x`, `command -p`, `time -p`, `env -S'...'`, `--split-string=`,
  `nohup --`). cmd: `if [/i] [not] <condition>` skips its condition, `for ... in (set) do`
  runs what follows `do`, and a set in single quotes or backticks (`for /f`) is a command. A
  change of folder counts in every reading of the call (`{cd,..}`, `cd${IFS}..`), also
  `Push-Location`, `Set-Location -Path:x` and a drive switch `D:` (an unknown folder). On
  Windows trailing dots and spaces of a path part are dropped (`c:\proj\...` is the project
  folder itself, which is not inside it). SQL words also count with `/* */`, `--` and `#`
  comments taken out, and `psql -f -` (or `/dev/stdin`) fed by a pipe is a script from stdin.
  git's own options before the subcommand that take a value (`-C`, `-c`, `--git-dir`,
  `--work-tree`, `--namespace`, `--super-prefix`, `--config-env`, `--attr-source`) are skipped
  in code for every rule of program `git`, so a rules file written before one was known (the
  file is written once and never migrated) or a user's own git rule still finds the
  subcommand. A piped delete without paths takes the paths of the call before it only when that
  call lists items below them (`Get-ChildItem`, `gci`, `ls`, `dir`, `Get-Item`, `gi`); the
  output of another program (`Get-Content list.txt | Remove-Item -Recurse`) may name any path
  and is asked about. In a piped `ForEach-Object` (`%`) block, `$_`, `$PSItem` and their
  `.FullName` stand for those listed items, so `Get-ChildItem src | % { Remove-Item $_ -Recurse }`
  inside the project does not ask. No built-in rule (`hangar-*`) offers "Always allow". A
  hook request for a known session that does not parse (a lone surrogate escape in
  `tool_input.command`, `cwd`, `file_path` or `tool_name`, broken JSON) is answered 200 with a
  deny while the gate is on, since a refused request lets Claude Code go on. A connection
  leaves the slots a newcomer may take as soon as its head is read.
- **Read as the shells read it (amended 2026-11-15, third audit).** An audit measured 925
  inputs, 350 of which passed (all kept as `src-tauri/tests/data/command_guard_audit.txt`, with 393
  everyday commands in `command_guard_everyday.txt`). Now read:
  - *Joined scripts.* Several words after `pwsh -c`, `powershell` (positional), `cmd /c`,
    `eval`, `wsl` and `ssh host` are read twice: re-quoted as before and joined with blanks as
    the shell joins them (`pwsh -c 'git push' '--force'`, `powershell 'git push --force'`).
    `cmd` also takes `//c` (Git Bash), `/q/c`, `/cgit`, and only `/v` or `/v:on` makes `!x!` a
    variable (plain `echo !PATH!` passes).
  - *PowerShell.* Assignments in front of a pipeline (`$null = git ...`, `[string]$x`,
    `$a, $b =`, `$x.y`, `$x[0]`, `${x}`, `$global:x`, `$env:X`, all operators, chained, also
    glued as `$x=git`), `return` and `throw`; the call operator `&` before a value
    (`& (Get-Command git)`, `& $env:ComSpec /c ...`, `%ComSpec%` in cmd); backtick escapes in
    double quotes (`` `u{2d} ``, `` `t ``, ...); `$()` as nothing; `$('x')`, `@('a','b')`,
    `('a','b')` and `[string]'x'` as their text; a comma list as one word per element for both
    native programs (`git push --force,-v`) and paths (`Remove-Item .\x,..\y`);
    `ExpandString('...')` like `InvokeScript`; `(Join-Path a b)` of plain words and `$PWD`,
    `$(pwd)` as the working folder; `FileSystem::` stripped, any other provider path
    (`Registry::`, `HKCU:`, `function:`) unplaceable.
  - *cmd.* A `)` glued to a word closes its block (`if exist x (git push --force) else (...)`);
    a caret before a blank still parts the words for the program (measured); `start` drops its
    first quoted argument as the title and takes values for `/d`, `/affinity`, `/node`; a
    switch or folder glued to a built-in is split (`rd/s/q`, `del/s`, `cmd/c`, `cd..`, `cd\`,
    `cd/d x`, `cd=x`), and `cd /d` names its folder; a quoted program name ends at its quote
    (`"git"push`); `NAME=value` is no assignment in cmd.
  - *POSIX.* `coproc`, `nocorrect`, `-`, `repeat N`, `unbuffer`, `chronic`, `caffeinate`,
    `arch`, `xcrun`, `watch`, `flock`, `parallel` (with its `:::` values), `script -c` and
    `ssh host cmd` are unwrapped; zsh's `=git` and `=(...)`; extended globs are one word
    (`--@(force|x)`); `bash -c '...' a b` and `pwsh -cwa '...' a` read with `$0`..`$9`, `$@`,
    `$*`, `$args[n]`, `@args` replaced (`shift` or `${@:n}` with words after the script asks);
    the builtins that evaluate subscripts (`let`, `test`, `[`, `[[`, `declare -n`, `read`,
    `printf -v`, `unset`, `mapfile`, ...) have every substitution in their words read, also
    quoted; `${x:+w}`, `${x+w}` and a replacement `${x/a/w}` are one more reading like
    `${x:-w}`; variables whose value a program runs (`GIT_SSH_COMMAND`, `GIT_PAGER`,
    `PROMPT_COMMAND`, ...) have it read as a script, `BASH_ENV=<(...)` is a script from stdin;
    `pushd +1`, `CDPATH`, `cdable_vars` and `autocd` make the folder unknown; `wsl ~` is
    skipped.
  - *Options.* An option word with a wildcard counts as every option it may match
    (`--f*`, `--forc?`, `-[f]`, `--@(force)`); a value glued to a short option is read for
    SQL words (`mysql -e'DROP TABLE x'`, `psql -cDROP\ ...`); `git PUSH` counts as `git push`
    where file names ignore case (Git for Windows finds `git-push`).
  - *Paths.* A wildcard right in the project that may match `.git` (`.*`, `.g*`, `?git`; a
    leading `*` does not match a dot) and a Windows short name right in it (`GIT~1`) count as
    outside; the items of `Get-ChildItem x` lie below `x`, also of a bare `Get-ChildItem`.
  - *Too complex.* bash's `${!name}` and `${x@P}`, zsh's `${(e)x}` and glob qualifiers that
    run code (`*(e:...:)`); git configuration that names a program (`-c core.pager=`,
    `core.sshCommand`, `core.editor`, `core.hooksPath`, drivers, helpers) or `git config`
    storing such a key, an alias or `clean.requireForce`; `--exec`, `--receive-pack`,
    `--upload-pack`; a database client's shell escape (`\!`, `system`, `.shell`, `.system`,
    `COPY ... FROM PROGRAM`); a database client fed by `< <(...)` without SQL is a script from
    stdin. From cmd, `sh -c '...'` with an unclosed quote on one line is not read (sh rejects
    it whole).
  - *Rules.* New default rules, asked like every rule: `git-push-delete` (`--delete`, `-d`,
    `--mirror`, `--prune`, and a refspec `:main`; a refspec `+main` now counts as `--force`),
    `dropdb`, `mysqladmin-drop`; `sql-drop` also covers `Invoke-Sqlcmd`. A rules file written
    before keeps its rules (it is never migrated); Settings says it holds fewer than Hangar
    ships.
- **Fourth audit (2026-11-15).** More ways the shells hide a command are read, and a few false
  alarms are gone.
  - *cmd `@` and `for` sets.* A leading `@` (echo off) is stripped from the first word of every
    cmd command, not only at the line's start, so `do @rd`, `if <cond> @git` and `else @rd` are
    read. A `for` set of literal paths places the loop variable: `for %d in (.git) do rd /s /q
    "%d"` reads `.git`, `for /d /r <root> %d in (<name>) do ...` reads `<root>/<name>` (a
    recursive loop finds entries of that name below the root). cmd splits the set at blanks,
    commas, semicolons and equals signs. A `/f` set (command output), a `/l` set (numbers), a
    wildcard set (`for %i in (*)`) and a set with a quote, caret, percent or any other special
    character keep the variable (unplaceable, asked about).
  - *`find -exec` and `.git`.* `{}` reads as `<start>/*`, which a leading-dot name escapes, so a
    `find` whose entries are not limited by name, whose `-name`/`-path`/`-regex` test can match
    `.git` (ignoring case), or that negates or combines tests (`!`, `-not`, `-o`, `,`), is also
    read with `{}` as `<start>/.git`.
  - *Program launchers and container exec, read at every offset.* `docker|podman exec|run|create`
    (also `docker container exec`), `docker compose exec|run`, `kubectl exec`, `su (-c|--command=)`,
    `runuser ... --`, `gsudo`, `npx`, `pnpm|yarn exec|dlx`, `bunx`, `uvx`, `uv|poetry|pipenv run`,
    `conda run`, `mise exec`, `direnv exec`, `dotenv`, `cross-env`, `op|doppler|infisical run`,
    `aws-vault exec`, `nix-shell --run`, `nix develop -c`, `systemd-run`, `xvfb-run`, `strace`,
    `gtimeout`. The launcher option sets are open, so after the global options (`docker -H`/
    `--context`, `compose -f`/`-p`, `kubectl -n`/`--context`, `uv --directory`, `pnpm -C`,
    `poetry -C`, `gsudo -u`, `strace -o`) and the subcommand, the inner command is read starting at
    every word that is no option (bounded by `MAX_LAUNCH_OFFSETS`), so an option Hangar does not
    know cannot hide the command behind it (`docker run --platform x img git push --force` asks). A
    lone interactive shell in a container (`docker run -it app sh`, `docker exec -it db bash -l`) is
    not read as a stdin script, unless the launcher is fed by a pipe (`echo ... | docker exec -i db
    sh`), which it reads as a script from stdin.
  - *Writes into the config parent.* A write whose argument is the folder that holds Hangar's
    settings folder (its parent, or an `APP_FOLDERS` root such as `$env:APPDATA`, `~/.config`) is
    asked about when another argument names the identifier, a settings file, or a wildcard (also
    inside the project, also in a PowerShell `-Name`), and always for an extractor or mirror tool
    pointed at that parent: `tar` (`-x`, `-C`, old style `tar xf`), `unzip`, `7z x`,
    `Expand-Archive -DestinationPath`, `robocopy`, `xcopy`, `rsync`, with the destination also read
    from a glued option (`-C<dir>`, `-o<dir>`, `--directory=<dir>`) or from the working folder when
    the extractor runs in the parent. A plain listing of the parent passes.
  - *Claude Code hook settings.* A command that touches, or an edit of, a `.claude/settings.json`
    or `.claude/settings.local.json` (in the project or the home folder) is asked about like
    Hangar's own settings, since writing one can turn the gate hook off (`disableAllHooks`,
    `allowedHttpHookUrls`; `raw/2026-11-09-claude-hooks-reference.md`). Any touch asks, reads
    (`cat`, `git diff`) included, because the guard cannot see the file's content and so cannot
    tell a read from a write: it fails closed. The file is recognized through an NTFS data stream
    on the name (`settings.json::$DATA`, `:x`), a Windows 8.3 short name, and a wildcard that could
    match it, and when a `.claude` folder and a settings file are named in two separate arguments
    (`cp settings.json .claude/`).
  - *dotglob.* With `shopt -s dotglob`, zsh `globdots` (any case, with or without underscores)
    or bash's `GLOBIGNORE` (setting it turns dotglob on) in the text a leading `*` may match
    `.git`, so `rm -rf *` is read as reaching it.
  - *False alarms gone.* `git push --dry-run --force` (and `-nf`) passes: `--dry-run`/`-n` is a
    `none` option of the push rules, and a `none` lifts a rule only on a definite match, never a
    wildcard (`git push -?`) or an unexpandable brace, never as the value of the option before it
    (`git push -o -n --force`, `--repo -n`, a group `-on`; `command_guard::LIFT_VALUE_OPTIONS`),
    and never when a negation (`--no-dry-run`, also abbreviated) is given anywhere: each of these
    still force-pushes. A literal `for`/`foreach` loop over folder names places the variable in its
    body, so `for d in build dist; do rm -rf "$d"; done`, the PowerShell `foreach ($d in
    'build','dist')` and `'build','dist' | ForEach-Object { Remove-Item $_ }` pass when the items
    lie inside the project. The variable is bound only when the whole command writes it exactly
    once, by that one literal loop header, and runs no inline script (`sh -c`, `eval`, a sourced
    file): a second write anywhere (another loop, `d=...`, `d+=...`, `export d=`, `read d`, `printf
    -v d`, `mapfile`/`getopts d`, PowerShell `$d = ...`, `$d++`, `Set-Variable d`) leaves the
    variable unplaced, so a later rebinding to an outside path cannot hide behind a safe-looking
    loop (`command_split` records every write it sees in the raw words, also after a reserved
    word such as `do` or `then`). PowerShell names are compared ignoring case and without scope
    or braces (`$D`, `$global:d`, `${d}`), every variable before an assignment operator counts
    (`$d, $e = ...`), and so does any bare word, which may name a variable to a cmdlet
    (`-OutVariable d`). A dot-sourced script or a bash name reference (`declare -n`) may write any
    variable, so no loop variable is bound then. Inside a `.claude` folder any wildcard that can
    match a settings file asks (`.claude/*`), and a glued option value counts as a path
    (`--target-directory=.claude`).
- **Bounded reading.** Every recursion over the agent's text has a fixed bound: inline scripts
  and substitutions 6 levels (`command_split::MAX_DEPTH`), brace expansion 8 braces per word
  (`command_guard::MAX_BRACE_GROUPS`, more fall back to the prefix rule), a `for`-set or loop-body
  expansion at most `MAX_FIND_STARTS`/`MAX_CWDS` readings. Each hook connection runs on a thread
  with an 8 MiB stack (`gate_service.rs`, `CONNECTION_STACK`), since an overflow there would end
  the app and every session in it.
- **Honest limits.** A safety net, not a sandbox. Not recognized: a script file (`bash x.sh`,
  `pwsh -File`, `psql -f`, `\i`, `source`, `.read`), a program in a variable (`$x`,
  `$(echo git)`, `"$(which git)"`, cmd's `%x%` and `for` variables), a value only known at run
  time (`git push $F`, `"$@"` after `set --`, `$(echo --force)`, `${F#x}`, `${F^^}`), an alias
  or function (`alias`, `sal`, `function:`), `xargs`, `find -delete`, .NET calls
  (`[IO.Directory]::Delete`, `.Delete()`), tools other than the rules name (`git checkout -f`,
  `git restore`, `git stash drop`, `git branch -D`, `robocopy /mir`, `DROP VIEW`, `DELETE`),
  dynamic SQL (`EXECUTE 'DR' || 'OP ...'`), a command started by another language
  (`python -c "os.system(...)"`, `node -e`). PowerShell's `*` with `-Force` may match the
  hidden `.git`, but a leading `*` is read as not matching it. cmd with delayed expansion
  switched on in the registry reads `!x!` as a variable [UNVERIFIED]; Hangar assumes the
  default (off) unless `/v` is given. The `LIMIT` rows of the audit file list each gap. Text put together at
  run time is not read: PowerShell string arithmetic (`git push ('--for'+'ce')`), splatting
  (`git @a`), `[scriptblock]::Create($a + $b)` or `& $block`, `Invoke-Command` with a script
  block held in a variable, `$ExecutionContext` calls with a computed argument, git
  configuration from the environment (`GIT_CONFIG_PARAMETERS`, `GIT_CONFIG_COUNT`), an `IFS`
  set to something else than blanks. Brace expansion and default values are an extra reading
  of the call, so a quoted brace may ask without need. Asked about but not read: scripts from
  stdin, encoded or too complex commands. Nothing is enforced when Claude Code's hook fails
  open (the endpoint unreachable or overloaded). The fourth audit leaves these gaps standing:
  wrappers outside the unwrapped set (`proxychains`, `firejail`, `flatpak-spawn`, `distrobox`,
  `toolbox`, `lxc exec`, `vagrant`, `hub`, `gh`), node deletion tools not in the rules (`rimraf`,
  `shx rm`), a database or extraction tool fed from a file the guard cannot read (`prisma db
  execute --stdin`, `sqlite3 app.db < drop.sql`), a copy into the config parent that names no
  identifier (`cp -r .config/. ~/.config/`), a Windows path written unquoted in bash where the
  backslash is stripped before cmd sees it (`cmd /c rd /s /q ..\x` becomes `..x`, a name in the
  folder), cmd `del /s` and PowerShell `Get-ChildItem -Force | Remove-Item` reaching `.git`
  through a runtime wildcard, and `ls -A | xargs rm -rf`.
- **Fail-closed version probe with a short negative cache.** The gate reads Claude Code's version
  to decide whether its hook blocks a call it cannot check (`onFailure: "block"` from 2.1.295,
  `gate::claude_blocks_on_failure`). A version probe that fails (a timeout, no output) is remembered
  as failed only for `version::NEGATIVE_TTL` (60 s), then queried again (`version::cached_version`,
  a `Cached` value with a `retry_after`): the short window keeps a program hanging on `--version`
  from blocking every profile list for the full `TIMEOUT` each time, while the fail-closed step
  comes back on the first list after the window instead of staying off for good behind a cached
  `None`. Surfacing "fail-closed not active" in `GateStatus` is left for the frontend (a new
  `session.rs` variant and i18n keys, not in this change).

## Consequences
- Claude Code team sessions started before this version keep their old matcher until restart.
- Each tool call of a Claude Code team session makes one local HTTP round trip (matcher `*`),
  also while the guard is off; tools that are neither edits nor commands are answered at once.
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
  `the_rules_file_is_written_once_and_read_again_when_it_changes`, `an_edit_with_a_working_folder_hangar_cannot_use_is_still_checked`, `hangars_worktrees_in_its_data_folder_are_not_its_settings`; `command_guard::tests` also `a_huge_brace_expansion_does_not_exhaust_a_small_stack`, `hangars_own_worktrees_are_ordinary_projects`, `redirections_wildcards_and_short_names_reach_the_settings`, `empty_special_parameters_do_not_hide_a_rule`, `powershell_parenthesized_and_wsl_arguments_are_read`; `command_split::tests::shell_options_with_values_do_not_hide_the_script`, `redirection_targets_and_parenthesized_values_are_kept_apart`; `command_split::tests::heredoc_body_is_data_and_keeps_quotes_in_sync`,
  `powershell_here_strings_are_one_word`, `braces_group_only_as_words_of_their_own`,
  `scripts_read_from_stdin_or_a_process_are_reported`,
  `pipelines_tell_each_call_where_its_input_comes_from`,
  `more_wrappers_and_inline_commands_are_unwrapped`,
  `huge_here_documents_and_here_strings_stay_bounded`; `command_guard::tests`
  `heredoc_bodies_do_not_hide_the_commands_after_them`,
  `powershell_here_strings_do_not_hide_the_commands_after_them`,
  `brace_expansion_counts_in_every_position`, `field_separators_and_default_values_are_read`,
  `cmd_substrings_and_delayed_variables_are_read_or_asked_about`,
  `abbreviated_long_options_and_git_overrides_count`,
  `scripts_from_stdin_and_processes_are_asked_about`,
  `powershell_deletes_with_colon_values_and_pipelines_are_placed`,
  `a_cd_that_may_not_apply_keeps_the_old_folder_too`, `sql_on_stdin_is_read_or_asked_about`,
  `more_wrappers_and_inline_commands_are_looked_through`,
  `huge_here_documents_and_here_strings_are_checked_quickly`; `gate::tests`
  `an_edit_path_hangar_cannot_read_is_kept_to_ask_about`,
  `any_tool_with_a_command_that_is_no_edit_is_a_shell_command`,
  `every_recipe_edit_matcher_names_known_edit_tools`; `gate_service::tests`
  `idle_connections_do_not_block_a_new_hook`, `one_session_cannot_take_every_slot`,
  `a_risky_command_among_a_storm_of_parallel_reads_is_never_waved_through`,
  `a_hundred_hooks_of_tools_that_change_nothing_are_answered_quickly`; `command_split::tests`
  `heredoc_inside_a_substitution_keeps_its_quotes_and_parens_to_itself`; `command_guard::tests`
  `everyday_agent_commands_raise_no_question`; `gate_service::tests`
  `an_edit_path_hangar_cannot_read_is_asked_about`,
  `every_tool_reaches_the_endpoint_but_only_edits_and_commands_are_checked`,
  `a_command_found_before_the_guard_was_switched_off_opens_no_card`,
  `an_edit_through_a_link_into_hangars_folder_is_asked_about`;
  `gate_hook::tests::a_path_the_gate_cannot_read_is_asked_about_not_skipped`;
  `gate_rules::tests::the_largest_id_in_a_file_does_not_overflow`; second audit:
  `command_guard::tests` `powershell_script_blocks_glued_to_words_are_read`,
  `substitutions_with_comments_cases_and_arithmetic_are_read_as_bash_does`,
  `here_document_delimiters_bash_reads_differently_are_asked_about`,
  `pipelines_into_invoke_expression_are_scripts_from_stdin`,
  `powershell_parameters_in_every_spelling_are_read`, `wrappers_with_options_are_looked_through`,
  `cmd_conditions_and_for_sets_are_read`, `every_change_of_folder_moves_the_working_folder`,
  `brace_expansions_past_the_caps_are_asked_about`,
  `windows_parts_of_dots_and_spaces_name_the_folder_itself`,
  `a_long_unclosed_bracket_pattern_is_checked_quickly`, `git_attr_source_takes_its_value`,
  `sql_from_stdin_files_and_behind_comments_is_seen`, `no_built_in_rule_can_be_remembered`,
  `a_deeply_nested_default_value_is_asked_about_on_a_small_stack`,
  `a_for_each_block_deletes_the_items_the_call_before_it_lists`,
  `git_options_before_the_subcommand_take_their_value_in_every_rules_file`,
  `adversarial_nesting_is_checked_quickly`; third audit: `gate::tests`
  `claude_pre_tool_use_hook_blocks_on_failure`, `command_guard::tests`
  `third_audit_inputs_ask_or_pass_as_recorded` (the audit file),
  `everyday_agent_commands_raise_no_question` (the everyday file, in each dialect and as
  `Shell`), `scripts_from_several_words_are_read_as_the_shell_joins_them`,
  `powershell_assignments_before_a_command_do_not_hide_it`,
  `a_cmd_paren_glued_to_a_word_ends_the_block`,
  `powershell_comma_lists_name_every_path_and_option`, `coproc_runs_its_command`,
  `builtins_that_evaluate_subscripts_have_their_substitutions_read`,
  `positional_arguments_of_inline_scripts_are_read`,
  `cmd_reads_carets_titles_glued_switches_and_folder_changes`,
  `powershell_values_known_before_the_command_runs_are_read`,
  `wildcards_and_short_names_that_reach_the_projects_git_folder_count`,
  `wsl_home_switch_is_skipped`, `sql_glued_to_short_options_and_shell_escapes_are_seen`,
  `option_words_with_wildcards_count_as_every_option_they_match`,
  `more_wrappers_run_their_command`, `commands_hidden_in_git_configuration_are_asked_about`,
  `remote_branch_deletions_and_refspec_force_pushes_ask`,
  `programs_named_through_lookups_and_folder_stacks_are_read`,
  `third_audit_false_positives_are_gone`; `gate_service::tests`
  `a_tool_call_hangar_cannot_read_is_blocked_not_refused`, `one_session_cannot_take_every_slot`
  (gate off: "no decision"), `only_a_claude_code_that_knows_on_failure_gets_it`; `tests/gate_hook.rs`
  `extension_failures_block_shell_commands_and_unknown_command_tools_are_checked`;
  `command_split::tests`
  `powershell_braces_end_words_except_variables_and_hashtables`,
  `arithmetic_holds_no_here_documents`, `a_substitution_ends_where_bash_ends_it`,
  `find_with_more_start_folders_than_read_is_reported`; `gate_service::tests`
  `a_tool_call_hangar_cannot_read_is_blocked_not_refused`,
  `a_connection_whose_head_is_read_is_never_closed_for_a_newcomer`; `tests/gate_hook.rs`
  `extension_shell_commands_fail_closed_when_the_endpoint_is_unreachable`,
  `claude_bash_hook_holds_a_risky_command_until_the_person_answers`,
  `pi_and_opencode_extensions_send_shell_commands_to_the_guard`,
  `extension_shell_tools_carry_their_dialect_and_working_folder`; Vitest `gate.test.ts` ("held
  shell commands"), `commandGuard.test.ts`, `GateCard.test.tsx`.
- Field names: [[shell-tool-input-shapes]]. Hook mechanics: [[claude-http-hook-measurement]],
  [[opencode-gate-live-measurement]], [[codex-gate-live-measurement]],
  [[agent-hooks-per-session-research]].
