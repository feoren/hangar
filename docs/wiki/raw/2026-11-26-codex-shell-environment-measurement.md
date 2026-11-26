# Measurement: Codex CLI shell environment policy and HANGAR_SEND (2026-11-26)

Platform: Windows 11 Pro (build 26300), Git Bash and PowerShell 7. Codex CLI 0.172.0, npm install
(`codex.cmd` launcher and the sh shim of the same package). User config without a
`shell_environment_policy` table. Token values below are dummies set for the test only.

Trigger: in a Codex CLI session started by Hangar, `hangar-send` answered "This is no Hangar
session that can send (HANGAR_SEND is not set)", although Hangar sets `HANGAR_SEND` in the
session's environment and `hangar-send` was found on `PATH`.

## 1. Default policy

```
export HANGAR_SEND=1:abc HANGAR_GATE=2:def
codex sandbox -- powershell -NoProfile -Command 'echo "send=$env:HANGAR_SEND gate=$env:HANGAR_GATE"'
```

Output:

```
send= gate=
```

Variables visible inside with the default policy (`Get-ChildItem env: | ForEach-Object Name`):

```
API_TIMEOUT_MS APPDATA COMSPEC GIT_CONFIG_COUNT GIT_CONFIG_KEY_0 GIT_CONFIG_KEY_1 GIT_CONFIG_VALUE_0
GIT_CONFIG_VALUE_1 GIT_PAGER HOMEDRIVE HOMEPATH LESS LOCALAPPDATA PAGER PATH PATHEXT ProgramData
PROGRAMFILES ProgramFiles(x86) ProgramW6432 PSMODULEPATH SHELL SYSTEMDRIVE SYSTEMROOT TEMP TMP
USERDOMAIN USERNAME USERPROFILE WINDIR
```

## 2. `inherit=all` alone

```
export HANGAR_SEND=1:abc HANGAR_GATE=2:def HANGAR_HOOK_TOKEN=x HANGAR_NOTIFY_TOKEN=y
codex sandbox -c shell_environment_policy.inherit=all -- powershell -NoProfile -Command \
  'echo "send=$env:HANGAR_SEND gate=$env:HANGAR_GATE hook=$env:HANGAR_HOOK_TOKEN notify=$env:HANGAR_NOTIFY_TOKEN"'
```

Output:

```
send=1:abc gate=2:def hook=x notify=y
```

The names with TOKEN arrived: with `inherit=all` alone, 0.172.0 did not apply the default
excludes (`*KEY*`, `*SECRET*`, `*TOKEN*`).

## 3. `inherit=all`, default excludes back on, explicit exclude

```
export HANGAR_SEND=1:abc HANGAR_GATE=2:def HANGAR_HOOK_TOKEN=x HANGAR_NOTIFY_TOKEN=y \
  MY_API_KEY=k MY_SECRET=s OTHER_VAR=o
codex sandbox -c shell_environment_policy.inherit=all \
  -c shell_environment_policy.ignore_default_excludes=false \
  -c "shell_environment_policy.exclude=['HANGAR_GATE']" -- powershell -NoProfile -Command '...'
```

Output:

```
send=1:abc gate= hook= notify= key= secret= other=o path=2096
```

## 4. Through the npm `codex.cmd` launcher (cmd.exe), from PowerShell

```
$env:HANGAR_SEND='1:abc'; $env:HANGAR_HOOK_TOKEN='x'; $env:MY_AUTH='a'; $env:OTHER_VAR='o'
& <npm>\codex.cmd sandbox -c shell_environment_policy.inherit=all `
  -c shell_environment_policy.ignore_default_excludes=false `
  -c "shell_environment_policy.exclude=['MY_AUTH']" -- cmd /c set | Select-String '^(HANGAR_|MY_AUTH|OTHER_VAR)'
```

Output:

```
HANGAR_SEND=1:abc
OTHER_VAR=o
```

The bare values (`all`, `false`) and the TOML literal-string list (`['MY_AUTH']`, no `"`) pass
cmd.exe unchanged and mean what they say.

## Not measured

- A live agent turn (model call) running `hangar-send` inside a Hangar session; `codex sandbox`
  was used as the stand-in for the agent's command runner.
- Whether Codex hooks get their environment through the same policy (the gate helper needs
  `HANGAR_GATE`; an earlier measurement with 0.167.2 saw it arrive in the hook).
- Linux and macOS.

## Correction (added the same day, after a review)

The diagnosis above is wrong; the measured outputs stand. Sections 1 to 4 ran with the owner's
own user config, which holds (only this part shown):

```
[shell_environment_policy]
inherit = "core"

[shell_environment_policy.set]
API_TIMEOUT_MS = "1800000"
```

So section 1 shows that policy, not Codex's default, and section 2 shows `inherit=all` overriding
it with `ignore_default_excludes` left at its default. Codex 0.172.0's own default, from its
source (`codex-rs/config/src/shell_environment_policy.rs`, `From<ShellEnvironmentPolicyToml>`;
`codex-rs/protocol/src/config_types.rs`, `impl Default for ShellEnvironmentPolicy`), is
`inherit = "all"` with `ignore_default_excludes = true`: without a policy table `HANGAR_SEND`
arrives, and so do names with KEY, SECRET or TOKEN. Filter order
(`codex-rs/protocol/src/shell_environment.rs`, `populate_env`): inherit, default excludes,
exclude, set, include_only (which filters set values too); `exclude` and `include_only` patterns
are case-insensitive globs.

## Further measurements, with a CODEX_HOME under tests/.tmp

Same machine and version, PowerShell 7, `$env:CODEX_HOME` set to a test folder,
`codex sandbox -- cmd /c set` filtered to the names shown. Dummy values only. `core` below is the
list `'PATH','PATHEXT','SHELL','COMSPEC','SYSTEMROOT','WINDIR','SYSTEMDRIVE','USERNAME',
'USERDOMAIN','USERPROFILE','HOMEDRIVE','HOMEPATH','PROGRAMFILES*','PROGRAMW6432','PROGRAMDATA',
'LOCALAPPDATA','APPDATA','TEMP','TMP','TMPDIR','POWERSHELL','PWSH'`.

A. Config `inherit = "core"` plus `set.API_TIMEOUT_MS`, no override:
   `API_TIMEOUT_MS`, `PATH`, `ProgramFiles`, `ProgramFiles(x86)`; no `HANGAR_SEND`.
B. Same config, `-c shell_environment_policy.inherit=all -c "shell_environment_policy.include_only=[core,'HANGAR_SEND','API_TIMEOUT_MS']"`:
   `API_TIMEOUT_MS`, `HANGAR_SEND`, `PATH`, `ProgramFiles`, `ProgramFiles(x86)`; no `HANGAR_GATE`,
   no `HANGAR_HOOK_TOKEN`, no `OTHER_VAR`. The `-c` keys merge into the file's table (its `set` stays).
C. As B without `'API_TIMEOUT_MS'` in the list: `API_TIMEOUT_MS` is gone (include_only filters set).
D. Empty config, no override: `HANGAR_GATE`, `HANGAR_HOOK_TOKEN`, `HANGAR_NOTIFY_TOKEN`,
   `HANGAR_SEND`, `MY_AUTH`, `OTHER_VAR` all arrive.
E. Empty config, `-c "shell_environment_policy.exclude=['HANGAR_GATE','HANGAR_HOOK_TOKEN','HANGAR_NOTIFY_TOKEN','MY_AUTH']"`:
   `HANGAR_SEND` and `OTHER_VAR` only.
F. Config `exclude = ['OTHER_VAR']`, `-c "shell_environment_policy.exclude=['HANGAR_GATE']"`:
   `OTHER_VAR` is back. A `-c` array replaces the file's array.
G. B through the npm `codex.cmd` launcher (cmd.exe): same result as B.
P. A profile file `work.config.toml` with `inherit = "core"`, `codex sandbox -p work`: no
   `HANGAR_SEND`; with the root `-c` overrides of B: `HANGAR_SEND` arrives (root `-c` beats the
   profile file).
L. A legacy `profile = "work"` or `[profiles.work]` in `config.toml`: Codex refuses to start
   (output cut after 60 characters: "Error: legacy `profile = "work"` config is no longer
   support"; with `-p work`: "--profile `work` cannot be used while ... contains legacy
   `profile = "work"` or `[profiles.work]` config; move those settings into ...work.config.toml").
M. Root `inherit = "core"` plus a profile file with only `set.PROFILE_SET`, `-p work`:
   `PROFILE_SET` arrives, `HANGAR_SEND` and `OTHER_VAR` do not: the profile's table merges key by
   key over the user config's.

Still not measured: a live agent turn running `hangar-send`, Linux and macOS. Codex hooks take a
snapshot of the Codex process environment (`codex-rs/hooks/src/registry.rs`), not the policy
(reviewer's reading of the source, not measured here).

Not measured either: how a project's `.codex/config.toml` policy layers over the user config.
Hangar does not evaluate such files; one that holds the policy, or cannot be read, means no
sending for the session.
