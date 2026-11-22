# Measurement: sub-agent definition formats of Copilot CLI, OpenCode and Gemini CLI

Date: 2026-11-22. Platform: Windows 11 Pro (26300), Git Bash and a ConPTY (pywinpty 160x50 in a
throwaway virtual environment). All files lived in a scratch folder of the repository
(`tests/.tmp/subagent-formats/`); each program's config folder was redirected there, so no user
folder was touched. No account was created and no credential was entered. User names in paths
are masked as `<user>`.

## Installed programs

```
$ where copilot opencode gemini qwen
C:\Users\<user>\AppData\Local\GitHubCopilotCLI\copilot.exe   (plus a VS Code shim)
C:\Users\<user>\AppData\Roaming\npm\opencode(.cmd)
C:\Users\<user>\AppData\Roaming\npm\gemini(.cmd)
INFORMATION: Es konnten keine Dateien mit dem angegebenen Muster gefunden werden.   (qwen)

$ copilot --version   -> GitHub Copilot CLI 1.0.114.
$ opencode --version  -> opencode v2.3.2
$ gemini --version    -> 0.70.0
```

Qwen Code is not installed: nothing measured for it.

## Copilot CLI 1.0.114

Folder `$COPILOT_HOME/agents` (default `~/.copilot/agents`). Files:

`hangar-probe.agent.md`:

```
---
# Managed by Hangar from ~/.claude/agents/hangar-probe.md. sha256:0000
name: hangar-probe
description: "Probe agent: Hangar \"format\" measurement"
---
When asked who you are, answer exactly: HANGAR-PROBE-OK
```

plus `hangar-probe.agent.md.bak` (text `x`). A first round used a file without `name` and a
second file `hangar-named.agent.md` with an HTML comment in the body.

```
$ COPILOT_HOME=<scratch>\copilot-home copilot --agent does-not-exist -p "hi"
No such agent: does-not-exist, available: hangar-probe
(first round: available: hangar-named, hangar-probe)

$ COPILOT_HOME=<scratch>\copilot-home copilot --agent hangar-probe -p "Who are you?" --silent
HANGAR-PROBE-OK
```

The account's existing sign-in served the headless run (it is not kept in `COPILOT_HOME`). The
`.bak` file is not listed. A YAML comment line inside the frontmatter, a quoted description with
a colon and escaped quotes, and an HTML comment in the body all load.

## OpenCode 2.3.2

`opencode debug paths` names the config folder `~/.config/opencode` on Windows; with
`XDG_CONFIG_HOME=<scratch>\xdg` it prints `<scratch>\xdg\opencode`. File
`<scratch>\xdg\opencode\agents\hangar-probe.md`:

```
---
# Managed by Hangar probe comment
description: "Probe agent: Hangar format measurement"
mode: subagent
---
When asked who you are, answer exactly: HANGAR-PROBE-OK
```

plus `hangar-probe.md.bak`. Without `--standalone`, `opencode debug agents` asks the shared
background service, which never saw the redirected variable, and lists only the built-in agents
(build, compaction, explore, general, plan, summary, title). With a private server:

```
$ XDG_CONFIG_HOME=<scratch>\xdg opencode debug agents --standalone    (exit 0)
...
  {
    "id": "hangar-probe",
    "name": "hangar-probe",
    ...
    "system": "When asked who you are, answer exactly: HANGAR-PROBE-OK",
    "description": "Probe agent: Hangar format measurement",
    "mode": "subagent",
    "hidden": false,
...
```

The id and name come from the file name, the body is the system prompt, the YAML comment is
ignored, the `.bak` file is not listed, nothing was installed into the folder. No headless run
was made with the agent.

## Gemini CLI 0.70.0

`GEMINI_CLI_HOME` replaces the home folder (bundle: `homedir()` returns it when set); user agents
come from `<home>/.gemini/agents/*.md`, files starting with `_` are skipped, the frontmatter
schema is strict (`name` slug `^[a-z0-9-_]+$`, `description` non-empty, only `kind`,
`display_name`, `tools`, `mcp_servers`, `model`, `temperature`, `max_turns`, `timeout_mins`
besides). Headless (`gemini -p hi`) stops at "Please set an Auth method ..." before any agent is
loaded. The interactive TUI loads user agents before the sign-in and reports problems on screen.

Run 1, TUI, a broken file `hangar-broken.md` with `mode: subagent`:

```
✕ Agent loading error: Failed to load agent from
 <scratch>\gemini-home\.gemini\agents\hangar-broken.md:
 Validation failed: Agent Definition:
 Unrecognized key(s) in object: 'mode'
```

Run 2 answered the trust question with "Don't trust" and pressed Escape; with no auth type set,
Gemini CLI started its Google sign-in flow and printed an accounts.google.com URL (nothing was
entered; the run was killed). Not repeated: later runs set
`{"security":{"auth":{"selectedType":"gemini-api-key"}}}` in `<scratch>\gemini-home\.gemini\settings.json`
with no key in the environment, which shows a key prompt instead.

Run 3, TUI with `--debug`, files `hangar-probe.md`:

```
---
# Managed by Hangar probe comment
name: hangar-probe
description: "Probe agent: Hangar format measurement"
---
When asked who you are, answer exactly: HANGAR-PROBE-OK
```

and `zz-second.md` (`name: hangar-probe`, `description: second file with the same name`) and
`hangar-probe.md.bak`:

```
ℹ Skipping project agents due to untrusted folder. To enable, ensure that the project root is trusted.
⚠ Duplicate agent name 'hangar-probe' detected. The later definition will be ignored. Rename one of the agents to avoid this conflict.
```

The bundle emits this warning in `registerAgent` only when an agent of that name is already
registered, so `hangar-probe.md` was registered. Run 4 repeated it (same warning) and typed
`/agents list`, which went into the API key prompt ("API key cannot be empty or whitespace
only."); nothing was stored.

Run 5 (control): `hangar-probe.md` moved out of the folder, `zz-second.md` alone:

```
ℹ This folder is untrusted, project settings, hooks, MCPs, and GEMINI.md files will not be applied for this folder.
ℹ Skipping project agents due to untrusted folder. To enable, ensure that the project root is trusted.
```

No duplicate warning and no loading error: the warning in run 3 came from the first file. The
`.bak` file caused no error.

## Vendor documentation (read 2026-11-22)

- GitHub Copilot CLI command reference,
  https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-command-reference,
  "Custom agents reference": locations in priority order: user `~/.copilot/agents/`, project
  `.github/agents/` and `.claude/agents/` from the working folder up to the git root, then
  added roots, plugins and organization agents; "When agents have the same ID, the first agent
  loaded is used." Files end in `.agent.md` or `.md`; "The CLI removes the `.agent.md` or `.md`
  extension" to form the id; `name` is optional and "does not control deduplication";
  `description` is required. Environment table: `COPILOT_HOME` "Override the configuration and
  state directory. Default: `$HOME/.copilot`."
- GitHub custom agents configuration,
  https://docs.github.com/en/copilot/reference/custom-agents-configuration: the Markdown body is
  the prompt, "The prompt can be a maximum of 30,000 characters."
- OpenCode agents, https://opencode.ai/docs/agents/ (V2): global
  `~/.config/opencode/agents/<name>.md`, project `.opencode/agents/<name>.md`; `mode` is
  `primary`, `subagent` or `all` and defaults to `primary` for a new custom agent; the body
  becomes the agent's `system` prompt; "V1 frontmatter fields are deprecated".
- Gemini CLI subagents, https://geminicli.com/docs/core/subagents/: project
  `.gemini/agents/*.md`, user `~/.gemini/agents/*.md`; the file "MUST start with YAML
  frontmatter"; `name` (lowercase letters, numbers, hyphens, underscores) and `description`
  required; "Subagents are enabled by default."
