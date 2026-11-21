---
title: Sub-agent library and figures
type: synthesis
tags: [agents, claude-code, sub-agents, settings, figures]
sources: [[claude-agent-flag-measurement]], [[claude-agent-frontmatter-reading]], [[sub-agent-models-research]]
created: 2026-11-15
updated: 2026-11-21
---

# Sub-agent library and figures

Specification and contract of Claude Code's sub-agent definitions in Hangar: listing them,
adding one from pasted text or a file, giving each a figure and starting Claude Code as one.
The view of running sub-agents is a different feature ([[agent-workflow-features]], item 23); it
only shows the figure of a known definition next to a running sub-agent of that type.

## Scope

- **Definitions:** Markdown files with a frontmatter (`name`, `description`, optional `model`,
  `color`, `tools`) in `~/.claude/agents` (global) and `<project>/.claude/agents` (project).
- **Settings > Sub-agents:** one row per name with figure, name, description, where it is
  defined (Global, Project <folder>, "from project <folder>, replaces the global one") and
  model; for a Hangar agent the whole figure (96 px high) is the button that opens the figure
  picker, any other row shows its initial without a picker; actions Open
  (default editor) and Delete (global definitions only, asked inline). The picker names the
  built-in figures in words ("Developer 2") and marks only a figure the user chose, not one
  derived from the name. "New sub-agent" takes pasted text or a loaded `.md`
  file and saves it as a global definition.
- **Figures:** only Hangar's own sub-agents wear figures (`isHangarAgent`: the name starts with
  `hangar-`). Every other agent (the user's own, a project's, a Codex CLI definition, a built-in
  type such as `Explore` in the crew column) shows its initial on its frontmatter color, else on a
  color its name picks (`initialFigure`), and has no figure picker; a figure stored for such a
  name is ignored, not deleted. A Hangar agent takes a built-in figure, an own image (PNG, GIF,
  WebP, JPEG, also animated, at most 2 MiB) chosen from a file or pasted from the clipboard, or
  the default. Decided by the owner: a figure says the agent came with Hangar.
- **Starting:** Claude Code profiles (`takesAgent`) start as a definition with `--agent <name>`:
  from a small chevron beside the program in the sidebar's program list, shown only with at
  least one definition for the start target folder, which opens a menu (`agentStartItems`,
  the popup menu's keyboard: arrows, Enter, Escape) with each definition's face, name, origin
  and the first line of its description; and from quick access ("Start Claude Code as backend
  in hangar" per start place, a project definition's origin in its detail).
- **Session look:** a session started as a definition shows the figure in place of the
  program badge, with the badge small (42 %) on its lower right corner and a tooltip naming
  program and agent (`SessionLook.tsx`). Where there is room the whole figure stands there
  (`crop="full"`, a 2 : 3 portrait box): sidebar rows in the avatar form (30 px high, 42 px on a
  card), the rail (34 px), quick access and the broadcast dialog (28 px). Where there is not, the
  face takes the badge's size (`crop="head"`): the 14 px title line of a program group, search
  results (16 px), the split view's pane headers, the peek, the overview tiles and the drag ghost
  (18 px). Group headers keep the program badge: they stand for the program, not one agent.
- **Running sub-agents:** the sub-agent tree shows the whole figure (36 px high) of a running
  sub-agent whose `agentType` names a listed definition (an initial for any but Hangar's own).
  The crew column ([[crew-column]]) shows every running sub-agent, a built-in type without a
  definition with its initial.

- **Codex CLI definitions:** TOML files in `~/.codex/agents` and `<project>/.codex/agents`
  (`codex_agents.rs`), listed in Settings with a "Codex CLI" badge, model and effort. They never
  start as an agent (Codex CLI has no option for it, [[sub-agent-models-research]]) and are never
  created from Hangar's form.
- **Models:** per definition and as a default per program for every session Hangar starts
  (section "Models").
- **Copies in other programs:** the user's global Claude Code definitions are handed to every
  other installed program whose format is known, so far Codex CLI (section "Provision to other
  programs").

## Backend (`src-tauri/src/agent_defs.rs`, `agent_figures.rs`)

### Reading definitions

- The frontmatter is read leniently, not as strict YAML: the user's own files hold values like
  `description: Backend: logic, APIs` that strict YAML refuses. Keys start a line; indented lines
  continue a value; `|` and `>` blocks, quoted values, `[a, b]` and `- a` lists are understood;
  `tools` may be a comma separated text or a list. The first of two equal keys wins.
- Limits: 256 KiB per file, 256 files per folder (the rest is one `tooMany` problem), 32 project
  folders per listing, 2000 characters of description to the window.
- A broken file becomes a problem entry (`tooLarge`, `notText`, `noFrontmatter`, `missingName`,
  `invalidName`, `missingDescription`, `duplicate`, `link`, `unreadable`, `tooMany`), never an
  error of the listing. Settings lists them under "Not read".
- Links: a file that is a symlink or any reparse point is not read (`link`). The agents folder
  itself is reached with `no_links::metadata(.., FollowLocal)`: a local link (a dotfiles setup)
  is followed, one to another computer and network or device paths are not.
- Project folders come from the window (start places and the sessions' working folders), must be
  absolute and are read only below `<folder>/.claude/agents`; the home folder counts as global.

### Names

`valid_name`: lowercase ASCII letters, digits and hyphens, starting and ending with a letter or
digit, at most 64. A definition with another name is `invalidName`. The same rule decides what
`--agent` may get, so the name never needs quoting on a command line or in a cmd.exe launcher.

> [!question] Unverified
> Claude Code's own rule for names is assumed from its documentation ("lowercase letters and
> hyphens"); the 2.1.325 binary names no rule for agents ([[claude-agent-flag-measurement]]).

### Same name in several places

The listing keeps every file. The window groups one name into one row (Global and each project
named) and, for a start in a folder, takes that project's definition before the global one
(`defsFor`). Claude Code is assumed to resolve the name the same way at runtime; Hangar only
passes the name.

> [!question] Unverified
> That a project definition outranks a global one of the same name in Claude Code.

### Project definitions are marked

A repository brings its own definitions. Wherever one is offered (menu, quick access, Settings,
the session's tooltip) it says "from project <folder>", and "replaces the global one" when a
global definition has its name. Figures belong to names and so to the global definitions: a
project definition that overrides a global one never wears that figure but its initial on a
neutral grey (`defFigure`), so a repository cannot pass its agent off as the trusted one. The
sub-agent tree and the crew column judge this per session folder (`runningFigure` with the
session's `cwd`): an override in one project leaves the figures in every other project alone.
Only without a folder does any override keep the global figure off.

### Fields beyond the prompt

Claude Code 2.1.325 applies an agent's own frontmatter `hooks` and `mcpServers` also to the
main-thread agent (`--agent`) and skips them when the definition's folder is not trusted; a
frontmatter `permissionMode` is ignored where it would widen the inherited mode
([[claude-agent-frontmatter-reading]]). The backend reports which of these fields a definition
sets (`extras`); the window names them for project definitions ("brings hooks, mcpServers").

### Writing definitions

- `create_sub_agent(text, replace)`: checks the text like a file (same parser, same name rule),
  writes `~/.claude/agents/<name>.md` atomically (temporary file and rename, `persist::write_atomic`),
  CRLF becomes LF and a last line break is added. A definition of that name, or a file
  `<name>.md`, is only replaced with `replace`; the window asks first ("exists" error). Replacing
  writes the existing definition's own file, whatever it is called. A link is never written
  through.
- `delete_sub_agent(name)`: global definitions only; project files belong to their repository.
- `open_sub_agent(name, folder)`: opens the file found by the backend's own listing in the default
  editor; the window never names a path.
- A file that merely holds the name (a broken file, another definition, another case of the
  name such as `Backend.md`) is never offered for replacing: the new definition takes the first
  free name (`<name>-2.md` and so on, compared without case). Only a definition of that name is
  replaced, in its own file, after the window asked.
- A new file never overwrites one: the text goes to a temporary file in the same folder (unique
  per process and call), which is hard linked to its name (fails with `AlreadyExists` when taken); without hard links (FAT,
  some network drives) the file is created with `create_new`. Replacing uses the atomic rename.
- Windows device names (`con`, `nul`, `aux`, `prn`, `com1` to `com9`, `lpt1` to `lpt9`) are
  refused when a definition is created (`reserved`); existing definitions of such a name stay
  listed and startable.
- Errors reach the window as `{ kind, message, problem }` (`tooLarge`, `invalid` with the problem,
  `exists`, `reserved`, `notFound`, `io`).

### Figures

- Choice: `agent-figures.json` in the config folder, `{ version: 1, figures: { <name>: { kind:
  "builtin", id } | { kind: "image", file } } }`, at most 256 entries. Unknown ids, invalid names
  and image file names that are not `<sha256>.<png|gif|webp|jpg>` are dropped when read, so the
  file can never point outside the image folder.
- Limits when read: `agent-figures.json` at most 256 KiB, each own image at most 2 MiB read from
  disk (never more, no link followed: `persist::read_regular_capped`), at most 32 MiB of own
  images in one answer; images past it fall back to the derived figure.
- Own images: `agent-figures/<sha256>.<ext>` in the app's data folder, never in the user's
  `.claude` folder. The type comes from the bytes (`avatar::sniff`: PNG, JPEG, GIF87a/89a, WebP);
  SVG is refused (it can carry script). Checked again when read; a damaged file is left out.
  Unused image files are removed after every change.
- To the window as `data:` URLs (`sub_agent_figures`); the CSP allows no other images.
- Upload: the window reads the chosen file and sends it as Base64 (`set_sub_agent_figure`); the
  backend decodes at most 2 MiB worth of Base64.
- Clipboard: `paste_sub_agent_figure` reads the clipboard image with the same code as the
  screenshot paste (`clipboard_image::read`), scales it down to 256 px on the longer side (box
  filter with premultiplied alpha) and saves it as PNG. An animated clipboard image arrives as one
  frame.
- Errors reach the window as text with a stable code first (`[no_image]`, `[not_an_image]`,
  `[too_large]`, `[too_many]`, `[unknown_figure]`, `[invalid_name]`), as `ManagerError` does; the
  window words the first three itself (`figureErrorCode`) and never matches the English text.

### Starting as a definition

- Catalog field `agent_args` (Claude Code: `["--agent", "{agent}"]`, [[claude-agent-flag-measurement]]),
  inherited by a user profile that starts the catalog's program, like `prompt_args`.
  `ProfileView.takesAgent` tells the window.
- `create_session(..., subAgent)`: the manager refuses a name `valid_name` refuses
  (`InvalidSubAgent`) and a program without `agent_args` (`NoSubAgentStart`). The arguments go
  right after the start arguments and before a positional first prompt, as an argv array.
- The session keeps the name (`SessionInfo.subAgent`) across restarts and resume fallbacks, the
  workspace stores it (`SavedSession.subAgent`, checked again on save and load), and a restore
  starts it as that definition again. The session is named after the definition.

> [!question] Unverified
> Whether `--agent` combined with `--resume <id>` keeps the agent in the resumed conversation.

## Bundled definitions (`src-tauri/src/bundled_agents.rs`)

Hangar ships twelve Claude Code definitions of its own (`hangar-backend`, `hangar-comfyui`,
`hangar-developer`, `hangar-frontend`, `hangar-manager`, `hangar-marketing`, `hangar-n8n`,
`hangar-researcher`, `hangar-sales`, `hangar-security`, `hangar-social-media`, `hangar-tester`)
as `src-tauri/bundled-agents/<name>.md`, embedded at compile time (`include_str!`), so nothing is
read from the installation folder at run time. A test keeps every file parsable with `parse_def`
and its `name` equal to its file name. The owner decided that every user gets them without doing
anything. The `hangar-` prefix keeps them apart from the user's own names (and gives them their
figures); they name each other with it (`hangar-frontend`). They set no `model`, so they take the
session's model: a fixed `opus` would cost users who never chose it. `color` and `tools` stay.

- **Switch:** "Provide Hangar's sub-agents" in Settings > Sub-agents, on by default, stored as
  `bundledOff` in `sub-agent-settings.json` (`main_store.rs`, main window only).
- **When:** at the app start before the copies in other programs, and in every copy run
  (`sync_all` in `commands.rs`: `sync_sub_agents`, after creating or deleting a definition, after
  a switch). The copies in Codex CLI follow in the same run, as they read `~/.claude/agents`. One
  run at a time (own lock, the manifest is read and written whole); the switch is read once the
  run holds the lock, so an older run cannot undo a newer switch. Nothing happens while no agent
  program that reads definitions is installed: Settings shows the switch only then, and Hangar
  creates no `~/.claude` on such a machine.
- **Manifest instead of a marker:** a marker line would end up in the prompt. Hangar keeps
  `bundled-agents.json` in its local app data folder: per name either `provided` with the SHA-256
  of the bytes it last wrote, or `removed`. A missing or damaged manifest is empty, so every
  existing file then counts as the user's. A manifest that exists but cannot be read (locked, a
  folder) stops the run without a change, so a passing error never forgets Hangar's copies.
- **Creating:** a name no global definition holds, by file name (`<name>.md` in any case, also a
  broken file) or by `name` in another file, gets `~/.claude/agents/<name>.md`: LF line ends,
  written to a temporary file and hard linked to its name, or created with `create_new` where hard
  links are missing (`agent_defs::write_exact`). A file that appears meanwhile wins and the name
  counts as taken.
- **Updating:** a `provided` copy whose bytes still match the manifest's hash is Hangar's; when the
  embedded text differs, it is replaced and the hash updated.
- **Move first, then check:** replacing and deleting never trust an earlier read. The copy is
  renamed to a temporary name in the same folder (`take_if_unchanged`), and the hash of the
  renamed file decides: equal, it is deleted or the new text is created under the name with the
  same never-replacing write as a new copy (a file that took the name meanwhile wins); not equal,
  it is put back with a hard link that fails on a taken name, so an edit made between the check
  and the change is never lost.
- **Left aside:** a crash or failure between moving and finishing leaves
  `.<name>.md.<pid>.<n>.old`. Every run looks for them first: put back when the name is free (so
  the copy is not taken for one the user deleted), deleted when they hold Hangar's unchanged copy,
  else left alone, as they may hold the user's text.
- **Edited:** a copy whose bytes differ from the hash (or that is no longer a plain file) belongs
  to the user from then on: it leaves the manifest and is never touched again, even if its text
  later matches Hangar's again.
- **Foreign file:** a file of a bundled name without a manifest entry is never touched.
- **Deleted:** a `provided` copy that is missing is marked `removed` and not created again.
- **Unreadable for now:** a copy another program holds locked is left as it is and stays in the
  manifest; the next run updates or removes it.
- **Switched off:** unchanged copies are deleted, edited ones stay, and the manifest forgets every
  name except a copy that could not be read or deleted. Switching on again therefore creates whatever is missing, also copies the user had
  deleted: turning the switch on is the user's deliberate choice.
- **Listing:** `sub_agent_library` names the global definitions that are unchanged bundled copies
  (`bundled`); Settings adds "From Hangar" to their place. A project file of a bundled name is the
  repository's and is never marked. `unchanged` walks the embedded list and only looks names up
  in the manifest, so a manifest key such as `../evil` never becomes a path; a run drops keys
  that name no bundled definition.
- **Copies in Codex CLI:** a name that only Hangar's unchanged copy holds goes to Codex CLI more
  carefully than the user's own definitions (`agent_provision::sync_with`, `bundled`): its copy
  only takes a free name in `~/.codex/agents` (or updates Hangar's own copy there, found by its
  marker), never sets a user's file of that name aside as `.bak`, and never reaches a project's
  `.codex/agents`. The user never asked for these agents, so they must not displace anything.

> [!question] Open: a read-only sandbox for read-only roles
> `hangar-researcher` and the other definitions whose `tools` hold no `Write`, `Edit` or
> `NotebookEdit` cannot be made read-only in Codex CLI from their copy: Codex CLI's documentation
> on custom agents says `sandbox_mode` and `approval_policy` in a custom agent file "don't override
> the parent configuration"; a sub-agent inherits the parent turn's sandbox (learn.chatgpt.com,
> docs/agent-configuration/subagents, read 2026-11-21, Codex CLI 0.171.0 installed). Hangar
> therefore writes no `sandbox_mode`. [UNVERIFIED] in Codex CLI's source code and at run time.

## Models (`src-tauri/src/agent_models.rs`)

Evidence: [[sub-agent-models-research]] (Claude Code 2.1.325, Codex CLI 0.169.0).

- **Program per catalog entry:** `agent_program = "claude" | "codex"` in `agent_catalog.toml`,
  inherited like `prompt_args`. It decides the definitions' format and folder and how a session
  takes the default model. `ProfileView.agentProgram` tells the window; the library answer names
  the installed ones (`installed`).
- **Values:** a model is `[A-Za-z0-9][A-Za-z0-9._:-]*`, at most 64 characters, never `inherit`;
  such a value needs no quoting in a frontmatter, a TOML string, a variable or a cmd.exe
  launcher. Claude Code offers the aliases `opus`, `sonnet`, `haiku`, `fable` (Fable marked as
  clearly more expensive) and "another id"; `opusplan` and the `[1m]` forms are not offered, as
  both change more than the model. Codex CLI takes a free model with suggestions (the main
  `model` of `~/.codex/config.toml`, the models its definitions name) and the efforts `minimal`,
  `low`, `medium`, `high`, `xhigh`. A value a file holds that is not offered is shown as it is
  and kept until the user changes it.
- **Per definition:** `set_sub_agent_model(program, name, folder, model, effort, confirmed)`.
  Claude Code: only the frontmatter's `model` line is replaced where it stands, added as the
  last field or removed with its continuation lines; BOM, line breaks and every other byte stay
  (`set_frontmatter_model`). Codex CLI: `model` and `model_reasoning_effort` through `toml_edit`,
  which keeps the rest of the file and the value's comment (`set_toml_model`); a copy Hangar
  keeps is written again whole with its marker. "Same as main session" removes the field rather
  than writing `inherit`: the file then reads as the user would write it, and the program's own
  order applies (field, then default, then the main session's model). A project's file belongs
  to its repository: the backend refuses it without `confirmed` (`confirm`), and the window asks
  inline first.
- **Default per program:** `sub-agent-settings.json` in the local app data folder, beside the
  other protected settings (`main_store.rs`, main window only, checked on reading and writing).
  "Not set" is the default. Applied at every agent process start (also restart and fallback):
  Claude Code gets `CLAUDE_CODE_SUBAGENT_MODEL=<model>` unless the profile sets the variable
  itself; Codex CLI gets `-c agents.default_subagent_model=<model>` and
  `-c agents.default_subagent_reasoning_effort=<effort>` as argv elements before the start
  arguments (before `resume --last`). The value goes unquoted when TOML would not parse it (Codex
  CLI then takes the raw text), quoted otherwise (`1.5`, `true`); a quoted one cannot pass a
  cmd.exe launcher (npm's `codex.cmd`) and is left out with a logged warning. A field in the
  definition beats the default in both programs; running sessions keep theirs until restarted.
  `tests/pty.rs` checks the variable in a real ConPTY session, also after a restart.

> [!question] Unverified
> - That a `-c agents.default_subagent_model` override reaches Codex CLI's sub-agents like the
>   same key in `config.toml` ([[sub-agent-models-research]]).
> - Effort values beyond the five offered for a given Codex model.

## Provision to other programs (`src-tauri/src/agent_provision.rs`, `codex_agents.rs`)

The owner wants the sub-agents added in Hangar to work in every agent CLI. Source: the global
Claude Code definitions (`~/.claude/agents/*.md`); project definitions stay with their
repository.

- **When:** at the app start (global folders and the stored start places, off the main thread),
  after creating or deleting a definition, after a switch in Settings, and whenever the window's
  set of folders changes or Settings opens (`sync_sub_agents(folders)`). One run at a time
  (`run_lock`, also held by a model change of a copy). No watcher.
- **Which folders:** the global `~/.codex/agents` and the `.codex/agents` of project folders the
  backend knows itself: the sessions' working folders and the stored start places (favorites,
  recent, `MainPrefs`). Folders the window names count only where they are among them
  (`permitted`); the same holds for a model change of a project's file, which is also refused
  when the project's `.claude`, `.codex` or their `agents` folder is a link.
- **Which programs:** every installed catalog program whose `agent_program` is not Claude Code
  and whose format is known: Codex CLI. A program that is not installed gets no folder and keeps
  its files. A switch per program ("Provide my sub-agents in Codex CLI"), on by default; off
  removes Hangar's copies and puts the replaced files back.
- **Format mapping, Codex CLI:** `name` is the definition's name, `description` its whole
  description, `developer_instructions` the Markdown prompt below the frontmatter (line breaks
  around it trimmed, CRLF as LF) as a multi-line literal string (`'''`), or as a multi-line basic
  string with escapes for backslash, quote and control characters when the prompt holds `'''`,
  ends with `'` or holds control characters. Not copied: `model` (Claude Code's aliases mean
  nothing there; the copy's own model comes from Settings), `tools` (Codex CLI names its tools
  differently), `color`, `hooks`, `mcpServers`, `permissionMode`.
- **Marker:** the first line `# Managed by Hangar from ~/.claude/agents/<name>.md.` (plus
  ` Replaced file kept as <backup>.` after a replacement) and ` sha256:<hex>`, the hash over every
  byte after that line. Only a file whose marker is intact is Hangar's; a copy edited by hand
  fails the hash and counts as the user's. The backup it names must be `<name>.toml.bak` or
  `<name>.toml.bak.<n>` of its own definition, else the marker is not Hangar's. Each run reads the
  marker again from the file it writes or removes, not from the earlier listing.
- **Same name (decided by the owner):** a user's file under a definition's name (its file name
  `<name>.toml` or its `name` field, also a broken file of that file name) is replaced: first set
  aside as `<name>.toml.bak`, or `.bak.2` up to `.bak.100` when taken (named after the
  definition, so the marker can always name it; hard linked or created anew, never overwriting
  one), then the copy is written atomically over it, naming the backup in its marker. Its model
  and effort move over to the copy. Other names are never touched.
- **A copy edited by hand:** set aside as the next backup, the new copy keeps naming the user's
  original backup, so removing the copy later brings the original back and the edited text stays
  as a backup. Settings names every backup of a copy (`backups`).
- **Global folder:** a missing copy is created (`<name>.toml`, never replacing a file that
  appeared meanwhile), and a copy is found by its marker as well. **Project folders:** only files
  that already hold a definition's name, by file name or `name` and never by the marker's source
  (a repository could forge a marker to steer a copy into any of its files), and only files git
  does not track (`git ls-files --error-unmatch`, argv; no repository: not tracked; a git that
  cannot answer: tracked). A tracked file stays and outranks the global copy in that project;
  Settings says "Project file in <folder> covers your copy". No new project file is created.
  `.codex` and `.codex/agents` of a project must not be links (checked from the project folder
  down), else the folder is skipped (`link`); the global folder follows local links like the
  definitions.
- **Removal:** a copy whose definition is gone is removed and its backup renamed back into its
  place; in a project only when git does not track it.
- **Issues:** each run reports what it left alone as `{ program, scope, folder, file, name, kind }`
  with a stable kind (`unreadable`, `io`, `offMachine`, `noBackupName`, `tracked`, `link`), file
  and folder names only. The newest run's issues come with the listing (`syncIssues`); Settings
  lists failures below the switch and names the reason in the definition's row; a failed
  `sync_sub_agents` call shows as the library's error.
- **Limits and safety:** names follow `valid_name`, Windows device names are skipped; paths are
  only `<home or known folder>/.codex/agents/<file>`; the same read limits as for definitions;
  writes are atomic.
- **State in the window:** derived from the listing (`codexDefs` with `managed`, `backup` and
  `backups`, and `syncIssues`): provided, replaced with the backups named, or why not.

> [!question] Open: other programs
> Gemini CLI (`~/.gemini/agents/*.md`), Qwen Code (`~/.qwen/agents/*.md`), GitHub Copilot CLI
> (`~/.copilot/agents/*.agent.md`) and OpenCode (`~/.config/opencode/agents/*.md`) are named in
> the owner's request but are not installed or not working here; their formats were not read
> ([[sub-agent-models-research]]). A new target needs its format measured, then a variant of
> `DefProgram`, a renderer and `agent_program` in its catalog entry.

## Window

- `subAgentModels.ts`: model rules, options, settings and copies (pure). `SubAgentModelField.tsx`:
  the model field. Settings shows the defaults on top ("Default model for all sub-agents", only
  installed programs), then one row per name with "Claude Code [model]" and "Codex CLI [model]
  [effort] · provided", and the user's own Codex CLI definitions with a badge.
- `subAgentLibrary.ts`: pure checks of the backend's answers, grouping, `defsFor`, default figure.
- `subAgentLibraryStore.ts`: one shared state; read on start, when the set of folders changes,
  when Settings opens and after a change.
- `AgentFigure.tsx`, `SessionLook.tsx`, `SubAgentSettings.tsx`, `agentStartMenu.tsx` and its
  chevron in `Sidebar.tsx`, quick access items in `launcherItems.ts`, the figure in
  `SubAgentTree.tsx`.

### Default figure

Only for a Hangar agent (`hangar-<role>`), without a chosen figure: the role figure a word after
the prefix points to (`backend`, `developer`, `frontend`, `manager`, `researcher`, `security`,
`tester`, `marketing`, `sales`, `social` to their figures, and so on), else a built-in figure
picked by a hash of the whole name, the same on every start (`hangar-n8n`, `hangar-comfyui`).
Any other name gets its initial (`initialFigure`), see "Figures" above. A project's own
definition never wears a figure, also under Hangar's prefix; a project copy with the global
definition's fingerprint is that agent and wears its figure.

### Built-in figures

Twelve chibi figures supplied by the owner (`backend-01`, `developer-01/02`, `frontend-01/02`,
`manager-01/02`, `researcher-01/02`, `security-01/02`, `tester-01`), shrunk from 1254 px PNGs to
portrait WebP with alpha, 240 x 360 px (2 : 3), by `tools/figures.py <in> <out>`: the empty margin
(the figures filled only about half the width of the square originals) is cut to the visible
pixels, the figure fills the height and stands on the bottom edge, so the head sits at the same
height in every file (Lanczos, quality 82, 15 to 28 KiB each).
Vite bundles them (`import.meta.glob`); the Rust list `agent_figures::BUILTIN`, the TypeScript
list `BUILTIN_FIGURES` and the files are kept equal by tests.

### Whole figure and head

`AgentFigure` draws the whole figure (`crop="full"`) in a 2 : 3 box, `size` being its height, and
for tight places the face (`crop="head"`) in a round box: the top 39 % of the file, centered
(`.agent-figure.head img`: width 170 %, left -35 %, top 0). An own image is always shown whole
(`object-fit: contain`), also in the round box. An initial on its color is a round badge in the
head form and a tall capsule in the full form.

### Motion

None. Figures stand still (the old breathing and bobbing, `busy` and the `still` flag are gone);
only the attention ring around a waiting session's figure in the rail pulses, like a badge's. An
own animated GIF or WebP keeps its own animation.
